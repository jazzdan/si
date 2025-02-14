use object_tree::{Hash, HashedNode};
use petgraph::prelude::*;

use super::{PkgResult, SiPkgAttrFuncInput, SiPkgError, Source};

use crate::{node::PkgNode, FuncUniqueId, SocketSpec, SocketSpecArity, SocketSpecKind};

#[derive(Clone, Debug)]
pub struct SiPkgSocket<'a> {
    func_unique_id: Option<FuncUniqueId>,
    kind: SocketSpecKind,
    name: String,
    arity: SocketSpecArity,
    ui_hidden: bool,

    hash: Hash,
    source: Source<'a>,
}

impl<'a> SiPkgSocket<'a> {
    pub fn from_graph(
        graph: &'a Graph<HashedNode<PkgNode>, ()>,
        node_idx: NodeIndex,
    ) -> PkgResult<Self> {
        let hashed_node = &graph[node_idx];
        let node = match hashed_node.inner() {
            PkgNode::Socket(node) => node.clone(),
            unexpected => {
                return Err(SiPkgError::UnexpectedPkgNodeType(
                    PkgNode::SOCKET_KIND_STR,
                    unexpected.node_kind_str(),
                ))
            }
        };

        Ok(Self {
            func_unique_id: node.func_unique_id,
            arity: node.arity,
            kind: node.kind,
            name: node.name,
            ui_hidden: node.ui_hidden,
            hash: hashed_node.hash(),
            source: Source::new(graph, node_idx),
        })
    }

    pub fn inputs(&self) -> PkgResult<Vec<SiPkgAttrFuncInput>> {
        let mut inputs = vec![];

        for idx in self
            .source
            .graph
            .neighbors_directed(self.source.node_idx, Outgoing)
        {
            inputs.push(SiPkgAttrFuncInput::from_graph(self.source.graph, idx)?);
        }

        Ok(inputs)
    }

    pub fn arity(&self) -> SocketSpecArity {
        self.arity
    }

    pub fn func_unique_id(&self) -> Option<FuncUniqueId> {
        self.func_unique_id
    }

    pub fn kind(&self) -> SocketSpecKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hash(&self) -> Hash {
        self.hash
    }

    pub fn ui_hidden(&self) -> bool {
        self.ui_hidden
    }

    pub fn source(&self) -> &Source<'a> {
        &self.source
    }
}

impl<'a> TryFrom<SiPkgSocket<'a>> for SocketSpec {
    type Error = SiPkgError;

    fn try_from(value: SiPkgSocket<'a>) -> Result<Self, Self::Error> {
        let mut builder = SocketSpec::builder();

        builder
            .kind(value.kind)
            .name(value.name())
            .func_unique_id(value.func_unique_id)
            .arity(value.arity)
            .ui_hidden(value.ui_hidden);

        for input in value.inputs()? {
            builder.input(input.try_into()?);
        }

        Ok(builder.build()?)
    }
}
