use std::sync::Arc;

use cyclone::{
    CodeGenerationRequest, CodeGenerationResultSuccess, FunctionResult, OutputStream,
    QualificationCheckRequest, QualificationCheckResultSuccess, ResolverFunctionRequest,
    ResolverFunctionResultSuccess, ResourceSyncRequest, ResourceSyncResultSuccess,
    WorkflowResolveRequest, WorkflowResolveResultSuccess,
};
use futures::{StreamExt, TryStreamExt};
use serde::{de::DeserializeOwned, Serialize};
use si_data::NatsClient;
use telemetry::prelude::*;
use thiserror::Error;
use tokio::sync::mpsc;

use self::subscription::{Subscription, SubscriptionError};
use crate::{
    nats_code_generation_subject, nats_qualification_check_subject, nats_resolver_function_subject,
    nats_resource_sync_subject, nats_subject, nats_workflow_resolve_subject,
    reply_mailbox_for_output, reply_mailbox_for_result,
};

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("failed to serialize json message")]
    JSONSerialize(#[source] serde_json::Error),
    #[error("nats error")]
    Nats(#[from] si_data::NatsError),
    #[error("no function result from cyclone; bug!")]
    NoResult,
    #[error("result error")]
    Result(#[from] SubscriptionError),
}

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Clone, Debug)]
pub struct Client {
    nats: NatsClient,
    subject_prefix: Option<Arc<String>>,
}

impl Client {
    pub fn new(nats: NatsClient) -> Self {
        Self {
            nats,
            subject_prefix: None,
        }
    }

    pub fn with_subject_prefix(nats: NatsClient, subject_prefix: impl Into<String>) -> Self {
        Self {
            nats,
            subject_prefix: Some(Arc::new(subject_prefix.into())),
        }
    }

    #[instrument(name = "client.execute_qualification_check", skip_all)]
    pub async fn execute_qualification_check(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &QualificationCheckRequest,
    ) -> ClientResult<FunctionResult<QualificationCheckResultSuccess>> {
        self.execute_request(
            nats_qualification_check_subject(self.subject_prefix()),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_qualification_check_with_subject", skip_all)]
    pub async fn execute_qualification_check_with_subject(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &QualificationCheckRequest,
        subject_suffix: impl AsRef<str>,
    ) -> ClientResult<FunctionResult<QualificationCheckResultSuccess>> {
        self.execute_request(
            nats_subject(self.subject_prefix(), subject_suffix),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_resolver_function", skip_all)]
    pub async fn execute_resolver_function(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &ResolverFunctionRequest,
    ) -> ClientResult<FunctionResult<ResolverFunctionResultSuccess>> {
        self.execute_request(
            nats_resolver_function_subject(self.subject_prefix()),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_resolver_function_with_subject", skip_all)]
    pub async fn execute_resolver_function_with_subject(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &ResolverFunctionRequest,
        subject_suffix: impl AsRef<str>,
    ) -> ClientResult<FunctionResult<ResolverFunctionResultSuccess>> {
        self.execute_request(
            nats_subject(self.subject_prefix(), subject_suffix),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_code_generation", skip_all)]
    pub async fn execute_code_generation(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &CodeGenerationRequest,
    ) -> ClientResult<FunctionResult<CodeGenerationResultSuccess>> {
        self.execute_request(
            nats_code_generation_subject(self.subject_prefix()),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_code_generation_with_subject", skip_all)]
    pub async fn execute_code_generation_with_subject(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &CodeGenerationRequest,
        subject_suffix: impl AsRef<str>,
    ) -> ClientResult<FunctionResult<CodeGenerationResultSuccess>> {
        self.execute_request(
            nats_subject(self.subject_prefix(), subject_suffix),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_resource_sync", skip_all)]
    pub async fn execute_resource_sync(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &ResourceSyncRequest,
    ) -> ClientResult<FunctionResult<ResourceSyncResultSuccess>> {
        self.execute_request(
            nats_resource_sync_subject(self.subject_prefix()),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_resource_sync_with_subject", skip_all)]
    pub async fn execute_resource_sync_with_subject(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &ResourceSyncRequest,
        subject_suffix: impl AsRef<str>,
    ) -> ClientResult<FunctionResult<ResourceSyncResultSuccess>> {
        self.execute_request(
            nats_subject(self.subject_prefix(), subject_suffix),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_workflow_resolve", skip_all)]
    pub async fn execute_workflow_resolve(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &WorkflowResolveRequest,
    ) -> ClientResult<FunctionResult<WorkflowResolveResultSuccess>> {
        self.execute_request(
            nats_workflow_resolve_subject(self.subject_prefix()),
            output_tx,
            request,
        )
        .await
    }

    #[instrument(name = "client.execute_workflow_resolve_with_subject", skip_all)]
    pub async fn execute_workflow_resolve_with_subject(
        &self,
        output_tx: mpsc::Sender<OutputStream>,
        request: &WorkflowResolveRequest,
        subject_suffix: impl AsRef<str>,
    ) -> ClientResult<FunctionResult<WorkflowResolveResultSuccess>> {
        self.execute_request(
            nats_subject(self.subject_prefix(), subject_suffix),
            output_tx,
            request,
        )
        .await
    }

    async fn execute_request<R, S>(
        &self,
        subject: impl Into<String>,
        output_tx: mpsc::Sender<OutputStream>,
        request: &R,
    ) -> ClientResult<FunctionResult<S>>
    where
        R: Serialize,
        S: DeserializeOwned,
    {
        let msg = serde_json::to_vec(request).map_err(ClientError::JSONSerialize)?;
        let reply_mailbox_root = self.nats.new_inbox();

        // Construct a subscription stream for the result
        let result_subscription_subject = reply_mailbox_for_result(&reply_mailbox_root);
        trace!(
            messaging.destination = &result_subscription_subject.as_str(),
            "subscribing for result messages"
        );
        let mut result_subscription: Subscription<FunctionResult<S>> =
            Subscription::new(self.nats.subscribe(result_subscription_subject).await?);

        // Construct a subscription stream for output messages
        let output_subscription_subject = reply_mailbox_for_output(&reply_mailbox_root);
        trace!(
            messaging.destination = &output_subscription_subject.as_str(),
            "subscribing for output messages"
        );
        let output_subscription =
            Subscription::new(self.nats.subscribe(output_subscription_subject).await?);
        // Spawn a task to forward output to the sender provided by the caller
        tokio::spawn(forward_output_task(output_subscription, output_tx));

        // Submit the request message
        let subject = subject.into();
        trace!(
            messaging.destination = &subject.as_str(),
            "publishing message"
        );
        self.nats
            .publish_with_reply_or_headers(subject, Some(reply_mailbox_root.as_str()), None, msg)
            .await?;

        // Wait for one message on the result reply mailbox
        let result = result_subscription
            .try_next()
            .await?
            .ok_or(ClientError::NoResult)?;
        result_subscription.unsubscribe().await?;

        Ok(result)
    }

    /// Gets a reference to the client's subject prefix.
    pub fn subject_prefix(&self) -> Option<&str> {
        self.subject_prefix.as_deref().map(String::as_str)
    }
}

async fn forward_output_task(
    mut output_subscription: Subscription<OutputStream>,
    output_tx: mpsc::Sender<OutputStream>,
) {
    while let Some(msg) = output_subscription.next().await {
        match msg {
            Ok(output) => {
                if let Err(err) = output_tx.send(output).await {
                    warn!(error = ?err, "output forwarder failed to send message on channel");
                }
            }
            Err(err) => {
                warn!(error = ?err, "output forwarder received an error on its subscription")
            }
        }
    }
    if let Err(err) = output_subscription.unsubscribe().await {
        warn!(error = ?err, "error when unsubscribing from output subscription");
    }
}

mod subscription {
    use std::{
        marker::PhantomData,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures::{Stream, StreamExt};
    use futures_lite::FutureExt;
    use pin_project_lite::pin_project;
    use serde::de::DeserializeOwned;
    use si_data::nats;
    use telemetry::prelude::*;
    use thiserror::Error;

    use crate::FINAL_MESSAGE_HEADER_KEY;

    #[derive(Error, Debug)]
    pub enum SubscriptionError {
        #[error("failed to deserialize json message")]
        JSONDeserialize(#[source] serde_json::Error),
        #[error("nats io error when reading from subscription")]
        NatsIo(#[source] si_data::NatsError),
        #[error("failed to unsubscribe from nats subscription")]
        NatsUnsubscribe(#[source] si_data::NatsError),
        #[error("the nats subscription closed before seeing a final message")]
        UnexpectedNatsSubscriptionClosed,
    }

    pin_project! {
        #[derive(Debug)]
        pub struct Subscription<T> {
            #[pin]
            inner: nats::Subscription,
            _phantom: PhantomData<T>,
        }
    }

    impl<T> Subscription<T> {
        pub fn new(inner: nats::Subscription) -> Self {
            Subscription {
                inner,
                _phantom: PhantomData,
            }
        }

        pub async fn unsubscribe(self) -> Result<(), SubscriptionError> {
            self.inner
                .unsubscribe()
                .await
                .map_err(SubscriptionError::NatsUnsubscribe)
        }
    }

    impl<T> Stream for Subscription<T>
    where
        T: DeserializeOwned,
    {
        type Item = Result<T, SubscriptionError>;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let mut this = self.project();

            match this.inner.next().poll(cx) {
                // Convert this NATS message into the cyclone request type `T` and return any
                // errors for the caller to decide how to proceed (i.e. does the caller fail on
                // first error, ignore error items, etc.)
                Poll::Ready(Some(Ok(nats_msg))) => {
                    // If the NATS message has a final message header, then treat this as an
                    // end-of-stream marker and close our stream.
                    if let Some(headers) = nats_msg.headers() {
                        if headers.keys().any(|key| key == FINAL_MESSAGE_HEADER_KEY) {
                            trace!(
                                "{} header detected in NATS message, closing stream",
                                FINAL_MESSAGE_HEADER_KEY
                            );
                            return Poll::Ready(None);
                        }
                    }

                    let data = nats_msg.into_data();
                    match serde_json::from_slice::<T>(&data) {
                        // Deserializing from JSON into the target type was successful
                        Ok(msg) => Poll::Ready(Some(Ok(msg))),
                        // Deserializing failed
                        Err(err) => Poll::Ready(Some(Err(SubscriptionError::JSONDeserialize(err)))),
                    }
                }
                // A NATS error occurred (async error or other i/o)
                Poll::Ready(Some(Err(err))) => {
                    Poll::Ready(Some(Err(SubscriptionError::NatsIo(err))))
                }
                // We see no more messages on the subject, but we haven't seen a "final message"
                // yet, so this is an unexpected problem
                Poll::Ready(None) => Poll::Ready(Some(Err(
                    SubscriptionError::UnexpectedNatsSubscriptionClosed,
                ))),
                // Not ready, so...not ready!
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::env;

    use cyclone::{
        CodeGenerated, ComponentView, QualificationCheckComponent, ResolverFunctionComponent,
    };
    use deadpool_cyclone::{instance::cyclone::LocalUdsInstance, Instance};
    use indoc::indoc;
    use si_data::NatsConfig;
    use si_settings::StandardConfig;
    use test_log::test;
    use tokio::task::JoinHandle;
    use uuid::Uuid;

    use super::*;
    use crate::{ComponentKind, Config, CycloneSpec, Server, ServerError};

    fn nats_config() -> NatsConfig {
        let mut config = NatsConfig::default();
        if let Ok(value) = env::var("SI_TEST_NATS_URL") {
            config.url = value;
        }
        config
    }

    async fn nats() -> NatsClient {
        NatsClient::new(&nats_config())
            .await
            .expect("failed to connect to NATS")
    }

    fn nats_prefix() -> String {
        Uuid::new_v4().as_simple().to_string()
    }

    async fn veritech_server_for_uds_cyclone(subject_prefix: String) -> Server {
        let cyclone_spec = CycloneSpec::LocalUds(
            LocalUdsInstance::spec()
                .try_cyclone_cmd_path("../../target/debug/cyclone")
                .expect("failed to setup cyclone_cmd_path")
                .cyclone_decryption_key_path("../../lib/cyclone/src/dev.decryption.key")
                .try_lang_server_cmd_path("../../bin/lang-js/target/lang-js")
                .expect("failed to setup lang_js_cmd_path")
                .resolver()
                .build()
                .expect("failed to build cyclone spec"),
        );
        let config = Config::builder()
            .nats(nats_config())
            .subject_prefix(subject_prefix)
            .cyclone_spec(cyclone_spec)
            .build()
            .expect("failed to build spec");
        Server::for_cyclone_uds(config)
            .await
            .expect("failed to create server")
    }

    async fn client(subject_prefix: String) -> Client {
        Client::with_subject_prefix(nats().await, subject_prefix)
    }

    async fn run_veritech_server_for_uds_cyclone(
        subject_prefix: String,
    ) -> JoinHandle<Result<(), ServerError>> {
        tokio::spawn(veritech_server_for_uds_cyclone(subject_prefix).await.run())
    }

    #[test(tokio::test)]
    async fn executes_simple_resolver_function() {
        let prefix = nats_prefix();
        run_veritech_server_for_uds_cyclone(prefix.clone()).await;
        let client = client(prefix).await;

        // Not going to check output here--we aren't emitting anything
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(output) = rx.recv().await {
                info!("output: {:?}", output)
            }
        });

        let request = ResolverFunctionRequest {
            execution_id: "1234".to_string(),
            handler: "numberOfParents".to_string(),
            component: ResolverFunctionComponent {
                data: ComponentView {
                    properties: serde_json::json!({}),
                    system: None,
                    kind: ComponentKind::Standard,
                },
                parents: vec![
                    ComponentView {
                        properties: serde_json::json!({}),
                        system: None,
                        kind: ComponentKind::Standard,
                    },
                    ComponentView {
                        properties: serde_json::json!({}),
                        system: None,
                        kind: ComponentKind::Standard,
                    },
                ],
            },
            code_base64: base64::encode(
                "function numberOfParents(component) { return component.parents.length; }",
            ),
        };

        let result = client
            .execute_resolver_function(tx, &request)
            .await
            .expect("failed to execute resolver function");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "1234");
                assert_eq!(success.data, serde_json::json!(2));
                assert!(!success.unset);
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }
    }

    #[test(tokio::test)]
    async fn executes_simple_qualification_check() {
        let prefix = nats_prefix();
        run_veritech_server_for_uds_cyclone(prefix.clone()).await;
        let client = client(prefix).await;

        // Not going to check output here--we aren't emitting anything
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(output) = rx.recv().await {
                info!("output: {:?}", output)
            }
        });

        let mut request = QualificationCheckRequest {
            execution_id: "5678".to_string(),
            handler: "check".to_string(),
            component: QualificationCheckComponent {
                data: ComponentView {
                    properties: serde_json::json!({"image": "systeminit/whiskers"}),
                    system: None,
                    kind: ComponentKind::Standard,
                },
                codes: vec![CodeGenerated {
                    format: "yaml".to_owned(),
                    code: "generateName: asd\nname: kubernetes_deployment\napiVersion: apps/v1\nkind: Deployment\n".to_owned()
                }],
                parents: Vec::new(),
            },
            code_base64: base64::encode(indoc! {r#"
                async function check(component) {
                    const skopeoChild = await siExec.waitUntilEnd("skopeo", ["inspect", `docker://docker.io/${component.data.properties.image}`]);

                    const code = component.codes[0];
                    const file = path.join(os.tmpdir(), "veritech-kubeval-test.yaml");
                    fs.writeFileSync(file, code.code);

                    try {
                        const child = await siExec.waitUntilEnd("kubeval", [file]);

                        return {
                          qualified: skopeoChild.exitCode === 0 && child.exitCode === 0,
                          message: JSON.stringify({ skopeoStdout: skopeoChild.stdout, skopeoStderr: skopeoChild.stderr, kubevalStdout: child.stdout, kubevalStderr: child.stderr }),
                        };
                    } finally {
                        fs.unlinkSync(file);
                    }
                }
            "#}),
        };

        // Run a qualified check (i.e. qualification returns qualified == true)
        let result = client
            .execute_qualification_check(tx.clone(), &request)
            .await
            .expect("failed to execute qualification check");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "5678");
                // Note: this might be fragile, as skopeo stdout API might change (?)
                let message = success.message.expect("no message available");
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(
                        serde_json::from_str::<serde_json::Value>(&message,)
                            .expect("Message is not json")
                            .as_object()
                            .expect("Message isn't an object")
                            .get("skopeoStdout")
                            .expect("Key skopeoStdout wasn't found")
                            .as_str()
                            .expect("skopeoStdout is not a string")
                    )
                    .expect("skopeoStdout is not json")
                    .as_object()
                    .expect("skopeoStdout isn't an object")
                    .get("Name")
                    .expect("Key Name wasn't found")
                    .as_str(),
                    Some("docker.io/systeminit/whiskers")
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&message,)
                        .expect("Message is not json")
                        .as_object()
                        .expect("Message isn't an object")
                        .get("skopeoStderr")
                        .expect("Key skopeoStderr wasn't found")
                        .as_str(),
                    Some("")
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&message,)
                        .expect("Message is not json")
                        .as_object()
                        .expect("Message isn't an object")
                        .get("kubevalStdout")
                        .expect("Key kubevalStdout wasn't found")
                        .as_str(),
                    Some(
                        format!(
                            "PASS - {} contains a valid Deployment (unknown)",
                            std::env::temp_dir()
                                .join("veritech-kubeval-test.yaml")
                                .display()
                        )
                        .as_str()
                    )
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&message,)
                        .expect("Message is not json")
                        .as_object()
                        .expect("Message isn't an object")
                        .get("kubevalStderr")
                        .expect("Key kubevalStderr wasn't found")
                        .as_str(),
                    Some("")
                );
                assert!(success.qualified);
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }

        request.execution_id = "9012".to_string();
        request.component.data = ComponentView {
            properties: serde_json::json!({"image": "abc"}),
            system: None,
            kind: ComponentKind::Standard,
        };

        // Now update the request to re-run an unqualified check (i.e. qualification returning
        // qualified == false)
        let result = client
            .execute_qualification_check(tx, &request)
            .await
            .expect("failed to execute qualification check");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "9012");
                assert!(!success.qualified);
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }
    }

    #[test(tokio::test)]
    async fn executes_simple_resource_sync() {
        let prefix = nats_prefix();
        run_veritech_server_for_uds_cyclone(prefix.clone()).await;
        let client = client(prefix).await;

        // Not going to check output here--we aren't emitting anything
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(output) = rx.recv().await {
                info!("output: {:?}", output)
            }
        });

        let request = ResourceSyncRequest {
            execution_id: "7867".to_string(),
            handler: "syncItOut".to_string(),
            component: ComponentView {
                properties: serde_json::json!({"pkg": "cider"}),
                system: None,
                kind: ComponentKind::Standard,
            },
            code_base64: base64::encode("function syncItOut(component) { return {}; }"),
        };

        let result = client
            .execute_resource_sync(tx, &request)
            .await
            .expect("failed to execute resource sync");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "7867");
                // TODO(fnichol): add more asserts once resource is filled in
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }
    }

    #[test(tokio::test)]
    async fn executes_simple_code_generation() {
        let prefix = nats_prefix();
        run_veritech_server_for_uds_cyclone(prefix.clone()).await;
        let client = client(prefix).await;

        // Not going to check output here--we aren't emitting anything
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(output) = rx.recv().await {
                info!("output: {:?}", output)
            }
        });

        let request = CodeGenerationRequest {
            execution_id: "7868".to_string(),
            handler: "generateItOut".to_string(),
            component: ComponentView {
                properties: serde_json::json!({"pkg": "cider"}),
                system: None,
                kind: ComponentKind::Standard,
            },
            code_base64: base64::encode("function generateItOut(component) { return { format: 'yaml', code: YAML.stringify(component.properties) }; }"),
        };

        let result = client
            .execute_code_generation(tx, &request)
            .await
            .expect("failed to execute code generation");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "7868");
                assert_eq!(
                    success.data,
                    CodeGenerated {
                        format: "yaml".to_owned(),
                        code: "pkg: cider\n".to_owned(),
                    }
                );
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }
    }

    #[test(tokio::test)]
    async fn executes_simple_workflow_resolve() {
        let prefix = nats_prefix();
        run_veritech_server_for_uds_cyclone(prefix.clone()).await;
        let client = client(prefix).await;

        // Not going to check output here--we aren't emitting anything
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(output) = rx.recv().await {
                info!("output: {:?}", output)
            }
        });

        let request = WorkflowResolveRequest {
            execution_id: "112233".to_string(),
            handler: "workItOut".to_string(),
            // TODO(fnichol): rewrite this function once we settle on contract
            code_base64: base64::encode("function workItOut() { return { name: 'mc fioti', kind: 'vacina butantan - https://www.youtube.com/watch?v=yQ8xJHuW7TY', steps: [] }; }"),
        };

        let result = client
            .execute_workflow_resolve(tx, &request)
            .await
            .expect("failed to execute workflow resolve");

        match result {
            FunctionResult::Success(success) => {
                assert_eq!(success.execution_id, "112233");
                // TODO(fnichol): add more assertions as we add fields
            }
            FunctionResult::Failure(failure) => {
                panic!("function did not succeed and should have: {:?}", failure)
            }
        }
    }
}
