use crate::{
    func::backend::js_workflow::FuncBackendJsWorkflowArgs, func::backend::FuncDispatchContext,
    func::binding::FuncBindingId, func::execution::FuncExecution, DalContext, Func,
    FuncBackendKind, FuncBinding, FuncBindingError, StandardModel, StandardModelError,
};
use async_recursion::async_recursion;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use strum_macros::{AsRefStr, Display, EnumIter, EnumString};
use thiserror::Error;
use tokio::sync::mpsc;
use veritech::OutputStream;

#[derive(Error, Debug)]
pub enum WorkflowError {
    #[error(transparent)]
    StandardModel(#[from] StandardModelError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    FuncBinding(#[from] FuncBindingError),
    #[error("missing workflow {0}")]
    MissingWorkflow(String),
    #[error("missing command {0}")]
    MissingCommand(String),
    #[error("command not prepared {0}")]
    CommandNotPrepared(FuncBindingId),
}

pub type WorkflowResult<T> = Result<T, WorkflowError>;

#[derive(
    Deserialize,
    Serialize,
    Debug,
    Display,
    AsRefStr,
    PartialEq,
    Eq,
    EnumIter,
    EnumString,
    Clone,
    Copy,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum WorkflowKind {
    Conditional,
    Exceptional,
    Parallel,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum WorkflowStep {
    Workflow {
        workflow: String,
        #[serde(default)]
        args: Vec<serde_json::Value>,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<serde_json::Value>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct WorkflowView {
    name: String,
    kind: WorkflowKind,
    steps: Vec<WorkflowStep>,
    args: Vec<serde_json::Value>,
}

impl WorkflowView {
    pub fn new(
        name: String,
        kind: WorkflowKind,
        steps: Vec<WorkflowStep>,
        args: Option<Vec<serde_json::Value>>,
    ) -> Self {
        Self {
            name,
            kind,
            steps,
            args: args.unwrap_or_default(),
        }
    }

    pub async fn resolve(ctx: &DalContext<'_, '_>, name: &str) -> WorkflowResult<WorkflowTree> {
        // TODO: add args
        let args = vec![];
        Self::resolve_inner(ctx, name, args, HashSet::new(), &mut HashMap::new()).await
    }

    async fn veritech_run(
        ctx: &DalContext<'_, '_>,
        func: Func,
        args: FuncBackendJsWorkflowArgs,
    ) -> WorkflowResult<Self> {
        assert_eq!(func.backend_kind(), &FuncBackendKind::JsWorkflow);
        let (_func_binding, func_binding_return_value) =
            FuncBinding::find_or_create_and_execute(ctx, serde_json::to_value(args)?, *func.id())
                .await?;
        Ok(Self::deserialize(
            func_binding_return_value
                .value()
                .unwrap_or(&serde_json::Value::Null),
        )?)
    }

    #[async_recursion]
    async fn resolve_inner(
        ctx: &DalContext<'_, '_>,
        name: &str,
        _args: Vec<serde_json::Value>,
        mut recursion_marker: HashSet<String>,
        workflows_cache: &mut HashMap<String, WorkflowTree>,
    ) -> WorkflowResult<WorkflowTree> {
        recursion_marker.insert(name.to_owned());

        // TODO: name is not necessarily enough
        let func = Func::find_by_attr(ctx, "name", &name)
            .await?
            .pop()
            .ok_or_else(|| WorkflowError::MissingWorkflow(name.to_owned()))?;
        let view = Self::veritech_run(ctx, func, FuncBackendJsWorkflowArgs).await?;

        let mut steps = Vec::with_capacity(view.steps.len());
        for step in view.steps {
            match step {
                WorkflowStep::Workflow { workflow, args } => {
                    if recursion_marker.contains(&workflow) {
                        panic!("Recursive workflow found: {}", workflow);
                    }

                    let key = format!("{workflow}-{}", serde_json::to_string(&args)?);
                    match workflows_cache.get(&key) {
                        Some(workflow) => steps.push(WorkflowTreeStep::Workflow(workflow.clone())),
                        None => {
                            let tree = Self::resolve_inner(
                                ctx,
                                &workflow,
                                args,
                                recursion_marker.clone(),
                                workflows_cache,
                            )
                            .await?;

                            steps.push(WorkflowTreeStep::Workflow(tree.clone()));
                            workflows_cache.insert(key, tree);
                        }
                    }
                }
                WorkflowStep::Command { command, args } => {
                    let func = Func::find_by_attr(ctx, "name", &command)
                        .await?
                        .pop()
                        .ok_or(WorkflowError::MissingCommand(command))?;
                    assert_eq!(func.backend_kind(), &FuncBackendKind::JsCommand);
                    let (func_binding, _) = FuncBinding::find_or_create(
                        ctx,
                        serde_json::to_value(args)?,
                        *func.id(),
                        *func.backend_kind(),
                    )
                    .await?;
                    // TODO: cache this
                    steps.push(WorkflowTreeStep::Command { func_binding })
                }
            }
        }
        Ok(WorkflowTree {
            name: view.name,
            kind: view.kind,
            steps,
        })
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum WorkflowTreeStep {
    Workflow(WorkflowTree),
    Command { func_binding: FuncBinding },
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkflowTree {
    name: String,
    kind: WorkflowKind,
    steps: Vec<WorkflowTreeStep>,
}

#[derive(Debug, Clone)]
pub struct FuncToExecute {
    func_binding: FuncBinding,
    func: Func,
    execution: FuncExecution,
    context: FuncDispatchContext,
    value: (Option<serde_json::Value>, Option<serde_json::Value>),
}

impl WorkflowTree {
    pub async fn run(&self, ctx: &DalContext<'_, '_>) -> WorkflowResult<()> {
        let (map, rxs) = self.prepare(ctx).await?;
        let map = self.clone().execute(map).await?;
        self.postprocess(ctx, map, rxs).await?;
        Ok(())
    }

    #[async_recursion]
    async fn prepare(
        &self,
        ctx: &DalContext<'_, '_>,
    ) -> WorkflowResult<(
        HashMap<FuncBindingId, FuncToExecute>,
        HashMap<FuncBindingId, mpsc::Receiver<OutputStream>>,
    )> {
        let mut map = HashMap::new();
        let mut rxs = HashMap::new();
        for step in &self.steps {
            match step {
                WorkflowTreeStep::Command { func_binding } => {
                    let id = *func_binding.id();
                    let func_binding = func_binding.clone();
                    let (func, execution, context, rx) =
                        func_binding.prepare_execution(ctx).await?;
                    map.insert(
                        id,
                        FuncToExecute {
                            func_binding,
                            func,
                            execution,
                            context,
                            value: (None, None),
                        },
                    );
                    rxs.insert(id, rx);
                }
                WorkflowTreeStep::Workflow(workflow) => {
                    let (m, r) = workflow.prepare(ctx).await?;
                    map.extend(m);
                    rxs.extend(r);
                }
            }
        }
        Ok((map, rxs))
    }

    // Note: too damn many clones
    #[async_recursion]
    async fn execute(
        self,
        mut map: HashMap<FuncBindingId, FuncToExecute>,
    ) -> WorkflowResult<HashMap<FuncBindingId, FuncToExecute>> {
        match self.kind {
            WorkflowKind::Conditional => {
                for step in self.steps {
                    match step {
                        WorkflowTreeStep::Command { func_binding } => {
                            let mut prepared = map.get_mut(func_binding.id()).ok_or_else(|| {
                                WorkflowError::CommandNotPrepared(*func_binding.id())
                            })?;
                            prepared.value = func_binding
                                .execute_critical_section(
                                    prepared.func.clone(),
                                    prepared.context.clone(),
                                )
                                .await?;
                        }
                        WorkflowTreeStep::Workflow(workflow) => {
                            map.extend(workflow.clone().execute(map.clone()).await?)
                        }
                    }
                }
            }
            WorkflowKind::Parallel => {
                let mut commands = tokio::task::JoinSet::new();
                let mut workflows = tokio::task::JoinSet::new();
                for step in self.steps {
                    match step {
                        WorkflowTreeStep::Command { func_binding } => {
                            let func_binding = func_binding.clone();
                            let prepared = map.get(func_binding.id()).ok_or_else(|| {
                                WorkflowError::CommandNotPrepared(*func_binding.id())
                            })?;
                            let (func, context) = (prepared.func.clone(), prepared.context.clone());
                            commands.spawn(async move {
                                func_binding
                                    .clone()
                                    .execute_critical_section(func, context)
                                    .await
                                    .map(|value| (func_binding, value))
                            });
                        }
                        WorkflowTreeStep::Workflow(workflow) => {
                            let map = map.clone();
                            workflows.spawn(async move { workflow.execute(map).await });
                        }
                    }
                }

                fn join<T>(res: Result<T, tokio::task::JoinError>) -> T {
                    match res {
                        Ok(t) => t,
                        Err(err) => {
                            assert!(!err.is_cancelled(), "Task got cancelled but shouldn't");
                            let any = err.into_panic();
                            // Note: Technically panics can be of any form, but most should be &str or String
                            match any.downcast::<String>() {
                                Ok(msg) => panic!("{}", msg),
                                Err(any) => match any.downcast::<&str>() {
                                    Ok(msg) => panic!("{}", msg),
                                    Err(any) => panic!(
                                        "Panic message downcast failed of {:?}",
                                        any.type_id()
                                    ),
                                },
                            }
                        }
                    }
                }

                // TODO: poll both in the same future

                while let Some(res) = commands.join_next().await {
                    let (func_binding, value) = join(res)?;
                    let mut prepared = map.get_mut(func_binding.id()).ok_or_else(move || {
                        WorkflowError::CommandNotPrepared(*func_binding.id())
                    })?;
                    prepared.value = value;
                }

                while let Some(res) = workflows.join_next().await {
                    map.extend(join(res)?);
                }
            }
            WorkflowKind::Exceptional => todo!(),
        }
        Ok(map)
    }

    async fn postprocess(
        &self,
        ctx: &DalContext<'_, '_>,
        map: HashMap<FuncBindingId, FuncToExecute>,
        mut rxs: HashMap<FuncBindingId, mpsc::Receiver<OutputStream>>,
    ) -> WorkflowResult<()> {
        // Do we have a problem here, if the same func_binding gets executed twice?
        for (_, mut prepared) in map {
            let id = *prepared.func_binding.id();

            // Drops tx so rx won't wait for it
            let (mut tx, _) = mpsc::channel(1);
            std::mem::swap(&mut prepared.context.output_tx, &mut tx);
            std::mem::drop(tx);

            let rx = rxs
                .remove(&id)
                .ok_or(WorkflowError::CommandNotPrepared(id))?;
            prepared
                .func_binding
                .postprocess_execution(ctx, rx, &prepared.func, prepared.value, prepared.execution)
                .await?;
        }
        Ok(())
    }
}
