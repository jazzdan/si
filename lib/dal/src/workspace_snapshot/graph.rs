use petgraph::algo;
use petgraph::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use ulid::Ulid;

use crate::workspace_snapshot::{
    change_set::ChangeSet,
    content_hash::ContentHash,
    edge_weight::EdgeWeight,
    node_weight::{NodeWeight, NodeWeightError, NodeWeightKind},
    WorkspaceSnapshotError, WorkspaceSnapshotResult,
};

#[derive(Debug, Error)]
pub enum WorkspaceSnapshotGraphError {
    #[error("NodeWeight error: {0}")]
    NodeWeight(#[from] NodeWeightError),
}

pub type WorkspaceSnapshotGraphResult<T> = Result<T, WorkspaceSnapshotGraphError>;

#[derive(Default, Deserialize, Serialize, Clone)]
pub struct WorkspaceSnapshotGraph {
    root_index: NodeIndex,
    graph: StableDiGraph<NodeWeight, EdgeWeight>,
}

impl std::fmt::Debug for WorkspaceSnapshotGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceSnapshotGraph")
            .field("root_index", &self.root_index)
            .field("graph", &self.graph)
            .finish()
    }
}

impl WorkspaceSnapshotGraph {
    pub fn new(change_set: &ChangeSet) -> WorkspaceSnapshotGraphResult<Self> {
        let mut graph: StableDiGraph<NodeWeight, EdgeWeight> = StableDiGraph::with_capacity(1, 0);
        let root_index = graph.add_node(NodeWeight::new_with_seen_vector_clock(
            change_set,
            NodeWeightKind::Root,
        )?);

        Ok(Self { root_index, graph })
    }

    fn is_acyclic_directed(&self) -> bool {
        // Using this because "is_cyclic_directed" is recursive.
        algo::toposort(&self.graph, None).is_ok()
    }

    pub fn cleanup(&mut self) {
        self.graph.retain_nodes(|frozen_graph, current_node| {
            // We cannot use "has_path_to_root" because we need to use the Frozen<StableGraph<...>>.
            algo::has_path_connecting(&*frozen_graph, self.root_index, current_node, None)
        });
    }

    fn dot(&self) {
        // NOTE(nick): copy the output and execute this on macOS. It will create two files in the
        // process and open a new tab in your browser.
        // ```
        // pbpaste | dot -Tsvg -o foo.svg && open foo.svg
        // ```
        let current_root_weight = self.get_node_weight(self.root_index).unwrap();
        println!(
            "Root Node Weight: {current_root_weight:?}\n{:?}",
            petgraph::dot::Dot::with_config(&self.graph, &[petgraph::dot::Config::EdgeNoLabel])
        );
    }

    fn add_node(&mut self, node: NodeWeight) -> WorkspaceSnapshotResult<NodeIndex> {
        let new_node_index = self.graph.add_node(node);
        self.update_merkle_tree_hash(new_node_index)?;

        Ok(new_node_index)
    }

    pub fn add_edge(
        &mut self,
        change_set: &ChangeSet,
        from_node_index: NodeIndex,
        edge_weight: EdgeWeight,
        to_node_index: NodeIndex,
    ) -> WorkspaceSnapshotResult<EdgeIndex> {
        // Temporarily add the edge to the existing tree to see if it would create a cycle.
        let temp_edge = self
            .graph
            .add_edge(from_node_index, to_node_index, edge_weight);
        let would_create_a_cycle = !self.is_acyclic_directed();
        self.graph.remove_edge(temp_edge);
        if would_create_a_cycle {
            return Err(WorkspaceSnapshotError::CreateGraphCycle);
        }

        let new_from_node_index = self.copy_node_index(change_set, from_node_index)?;

        // Add the new edge to the new version of the "from" node.
        let new_edge_index = self
            .graph
            .add_edge(new_from_node_index, to_node_index, edge_weight);
        self.update_merkle_tree_hash(new_from_node_index)?;

        // Update the rest of the graph to reflect the new node/edge.
        self.replace_references(change_set, from_node_index, new_from_node_index)?;

        Ok(new_edge_index)
    }

    fn get_node_index_by_id(&self, id: Ulid) -> WorkspaceSnapshotResult<NodeIndex> {
        for node_index in self.graph.node_indices() {
            if self.has_path_to_root(node_index) {
                let node_weight = self.get_node_weight(node_index)?;
                if node_weight.id == id {
                    return Ok(node_index);
                }
            }
        }

        Err(WorkspaceSnapshotError::NodeWithIdNotFound(id))
    }

    fn copy_node_index(
        &mut self,
        change_set: &ChangeSet,
        node_index_to_copy: NodeIndex,
    ) -> WorkspaceSnapshotResult<NodeIndex> {
        let new_node_index = self.graph.add_node(
            self.get_node_weight(node_index_to_copy)?
                .new_with_incremented_vector_clocks(change_set)?,
        );

        Ok(new_node_index)
    }

    pub fn update_content(
        &mut self,
        change_set: &ChangeSet,
        id: Ulid,
        new_content_hash: ContentHash,
    ) -> WorkspaceSnapshotResult<()> {
        let original_node_index = self.get_node_index_by_id(id)?;
        let new_node_index = self.copy_node_index(change_set, original_node_index)?;
        let node_weight = self.get_node_weight_mut(new_node_index)?;
        node_weight.new_content_hash(new_content_hash)?;

        self.replace_references(change_set, original_node_index, new_node_index)
    }

    fn has_path_to_root(&self, node: NodeIndex) -> bool {
        algo::has_path_connecting(&self.graph, self.root_index, node, None)
    }

    fn is_on_path_between(&self, start: NodeIndex, end: NodeIndex, node: NodeIndex) -> bool {
        algo::has_path_connecting(&self.graph, start, node, None)
            && algo::has_path_connecting(&self.graph, node, end, None)
    }

    fn get_node_weight(&self, node_index: NodeIndex) -> WorkspaceSnapshotResult<&NodeWeight> {
        self.graph
            .node_weight(node_index)
            .ok_or(WorkspaceSnapshotError::NodeWeightNotFound)
    }

    fn get_node_weight_mut(
        &mut self,
        node_index: NodeIndex,
    ) -> WorkspaceSnapshotResult<&mut NodeWeight> {
        self.graph
            .node_weight_mut(node_index)
            .ok_or(WorkspaceSnapshotError::NodeWeightNotFound)
    }

    fn replace_references(
        &mut self,
        change_set: &ChangeSet,
        original_node_index: NodeIndex,
        new_node_index: NodeIndex,
    ) -> WorkspaceSnapshotResult<()> {
        let mut old_to_new_node_indices: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        old_to_new_node_indices.insert(original_node_index, new_node_index);

        let mut dfspo = DfsPostOrder::new(&self.graph, self.root_index);
        while let Some(old_node_index) = dfspo.next(&self.graph) {
            // All nodes that exist between the root and the `original_node_index` are affected by the replace, and only
            // those nodes are affected, because the replacement affects their merkel tree hashes.
            if self.is_on_path_between(self.root_index, original_node_index, old_node_index) {
                // Copy the node if we have not seen it or grab it if we have. Only the first node in DFS post order
                // traversal should already exist since it was created before we entered `replace_references`, and
                // is the reason we're updating things in the first place.
                let new_node_index = match old_to_new_node_indices.get(&old_node_index) {
                    Some(found_new_node_index) => *found_new_node_index,
                    None => {
                        let new_node_index = self.copy_node_index(change_set, old_node_index)?;
                        old_to_new_node_indices.insert(old_node_index, new_node_index);
                        new_node_index
                    }
                };

                // Find all outgoing edges. From those outgoing edges and find their destinations.
                // If they do not have destinations, then there is no work to do (i.e. stale edge
                // reference, which should only happen if an edge was removed after we got the
                // edge ref, but before we asked about the edge's endpoints).
                let mut edges_to_create: Vec<(EdgeWeight, NodeIndex)> = Vec::new();
                for edge_reference in self.graph.edges_directed(old_node_index, Outgoing) {
                    let edge_weight = edge_reference.weight();
                    if let Some((_, destination_node_index)) =
                        self.graph.edge_endpoints(edge_reference.id())
                    {
                        edges_to_create.push((*edge_weight, destination_node_index));
                    }
                }

                // Make copies of these edges where the source is the new node index and the
                // destination is one of the following...
                // - If an entry exists in `old_to_new_node_indicies` for the destination node index,
                //   use the value of the entry (the destination was affected by the replacement,
                //   and needs to use the new node index to reflect this).
                // - There is no entry in `old_to_new_node_indicies`; use the same destination node
                //   index as the old edge (the destination was *NOT* affected by the replacemnt,
                //   and does not have any new information to reflect).
                for (edge_weight, destination_node_index) in edges_to_create {
                    // Need to directly add the edge, without going through `self.add_edge` to avoid
                    // infinite recursion, and because we're the place doing all the book keeping
                    // that we'd be interested in happening from `self.add_edge`.
                    self.graph.add_edge(
                        new_node_index,
                        *old_to_new_node_indices
                            .get(&destination_node_index)
                            .unwrap_or(&destination_node_index),
                        edge_weight,
                    );
                }

                self.update_merkle_tree_hash(new_node_index)?;

                // Use the new version of the old root node as our root node.
                if let Some(new_root_node_index) = old_to_new_node_indices.get(&self.root_index) {
                    self.root_index = *new_root_node_index;
                }
            }
        }

        Ok(())
    }

    fn update_merkle_tree_hash(
        &mut self,
        node_index_to_update: NodeIndex,
    ) -> WorkspaceSnapshotResult<()> {
        let mut hasher = ContentHash::hasher();
        hasher.update(
            self.get_node_weight(node_index_to_update)?
                .content_hash()
                .to_string()
                .as_bytes(),
        );

        // Need to make sure the neighbors are added to the hash in a stable order to ensure the
        // merkle tree hash is identical for identical trees.
        let mut ordered_neighbors = Vec::new();
        for neighbor_node in self
            .graph
            .neighbors_directed(node_index_to_update, Outgoing)
        {
            ordered_neighbors.push(neighbor_node);
        }
        ordered_neighbors.sort();

        for neighbor_node in ordered_neighbors {
            hasher.update(
                self.graph
                    .node_weight(neighbor_node)
                    .ok_or(WorkspaceSnapshotError::NodeWeightNotFound)?
                    .merkle_tree_hash()
                    .to_string()
                    .as_bytes(),
            );
        }

        let new_node_weight = self
            .graph
            .node_weight_mut(node_index_to_update)
            .ok_or(WorkspaceSnapshotError::NodeWeightNotFound)?;
        new_node_weight.set_merkle_tree_hash(hasher.finalize());

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        workspace_snapshot::content_hash::ContentHash, ComponentId, FuncId, PropId, SchemaId,
        SchemaVariantId,
    };

    #[test]
    fn new() {
        let change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let change_set = &change_set;
        let graph = WorkspaceSnapshotGraph::new(&change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");
        assert!(graph.is_acyclic_directed());
    }

    #[test]
    fn add_nodes_and_edges() {
        let change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let change_set = &change_set;
        let mut graph = WorkspaceSnapshotGraph::new(change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");

        let schema_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_id,
                    NodeWeightKind::Schema(ContentHash::new(
                        SchemaId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema");
        let schema_variant_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_variant_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_variant_id,
                    NodeWeightKind::SchemaVariant(ContentHash::new(
                        SchemaVariantId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema variant");
        let component_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let component_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    component_id,
                    NodeWeightKind::Component(ContentHash::new(
                        ComponentId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add component");

        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                component_index,
            )
            .expect("Unable to add root -> component edge");
        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                schema_index,
            )
            .expect("Unable to add root -> schema edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                schema_variant_index,
            )
            .expect("Unable to add schema -> schema variant edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add component -> schema variant edge");

        let func_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let func_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    func_id,
                    NodeWeightKind::Func(ContentHash::new(
                        FuncId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add func");
        let prop_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let prop_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    prop_id,
                    NodeWeightKind::Prop(ContentHash::new(
                        PropId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add prop");

        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                func_index,
            )
            .expect("Unable to add root -> func edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                prop_index,
            )
            .expect("Unable to add schema variant -> prop edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(prop_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(func_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add prop -> func edge");

        assert!(graph.is_acyclic_directed());
    }

    #[test]
    fn cyclic_failure() {
        let change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let change_set = &change_set;
        let mut graph = WorkspaceSnapshotGraph::new(change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");

        let schema_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let initial_schema_node_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_id,
                    NodeWeightKind::Schema(ContentHash::new(
                        SchemaId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema");
        let schema_variant_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let initial_schema_variant_node_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_variant_id,
                    NodeWeightKind::SchemaVariant(ContentHash::new(
                        SchemaVariantId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema variant");
        let component_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let initial_component_node_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    component_id,
                    NodeWeightKind::Component(ContentHash::new(
                        ComponentId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add component");

        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                initial_component_node_index,
            )
            .expect("Unable to add root -> component edge");
        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                initial_schema_node_index,
            )
            .expect("Unable to add root -> schema edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_id)
                    .expect("Cannot find NodeIndex"),
                EdgeWeight::default(),
                initial_schema_variant_node_index,
            )
            .expect("Unable to add schema -> schema variant edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot find NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot find NodeIndex"),
            )
            .expect("Unable to add component -> schema variant edge");

        let pre_cycle_root_index = graph.root_index;

        // This should cause a cycle.
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot find NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot find NodeIndex"),
            )
            .expect_err("Created a cycle");

        assert_eq!(pre_cycle_root_index, graph.root_index,);
    }

    #[test]
    fn update_content() {
        let change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let change_set = &change_set;
        let mut graph = WorkspaceSnapshotGraph::new(change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");

        let schema_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_id,
                    NodeWeightKind::Schema(ContentHash::new(
                        SchemaId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema");
        let schema_variant_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_variant_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_variant_id,
                    NodeWeightKind::SchemaVariant(ContentHash::new(
                        SchemaVariantId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema variant");
        let component_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let component_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    component_id,
                    NodeWeightKind::Component(ContentHash::new(
                        component_id.to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add component");

        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                component_index,
            )
            .expect("Unable to add root -> component edge");
        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                schema_index,
            )
            .expect("Unable to add root -> schema edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                schema_variant_index,
            )
            .expect("Unable to add schema -> schema variant edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add component -> schema variant edge");

        graph.dot();

        // TODO: This is meant to simulate "modifying" the existing component, instead of swapping in a completely independent component.
        graph
            .update_content(
                change_set,
                component_id.into(),
                ContentHash::new("new_content".as_bytes()),
            )
            .expect("Unable to update Component content hash");

        graph.dot();

        graph.cleanup();

        graph.dot();

        panic!();

        // TODO(nick,jacob): do something here
    }

    #[test]
    fn update_content_from_new_change_set() {
        let change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let change_set = &change_set;
        let mut graph = WorkspaceSnapshotGraph::new(change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");

        let schema_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_id,
                    NodeWeightKind::Schema(ContentHash::new(
                        SchemaId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema");
        let schema_variant_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let schema_variant_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    schema_variant_id,
                    NodeWeightKind::SchemaVariant(ContentHash::new(
                        SchemaVariantId::generate().to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add schema variant");
        let component_id = change_set.generate_ulid().expect("Cannot generate Ulid");
        let component_index = graph
            .add_node(
                NodeWeight::new(
                    change_set,
                    component_id,
                    NodeWeightKind::Component(ContentHash::new(
                        component_id.to_string().as_bytes(),
                    )),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add component");

        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                component_index,
            )
            .expect("Unable to add root -> component edge");
        graph
            .add_edge(
                change_set,
                graph.root_index,
                EdgeWeight::default(),
                schema_index,
            )
            .expect("Unable to add root -> schema edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(schema_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add schema -> schema variant edge");
        graph
            .add_edge(
                change_set,
                graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add component -> schema variant edge");

        graph.dot();

        let update_change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        graph
            .update_content(
                &update_change_set,
                component_id.into(),
                ContentHash::new("new_content".as_bytes()),
            )
            .expect("Unable to update Component content hash");

        graph.dot();

        graph.cleanup();

        graph.dot();

        panic!();

        // TODO(nick,jacob): do something here
    }

    #[test]
    fn compare_snapshots_purely_new_content() {
        let initial_change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let initial_change_set = &initial_change_set;
        let mut initial_graph = WorkspaceSnapshotGraph::new(initial_change_set)
            .expect("Unable to create WorkspaceSnapshotGraph");

        let schema_id = initial_change_set
            .generate_ulid()
            .expect("Cannot generate Ulid");
        let schema_index = initial_graph
            .add_node(
                NodeWeight::new(
                    initial_change_set,
                    schema_id,
                    NodeWeightKind::Schema(ContentHash::new("Schema A".as_bytes())),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add Schema A");
        let schema_variant_id = initial_change_set
            .generate_ulid()
            .expect("Cannot generate Ulid");
        let schema_variant_index = initial_graph
            .add_node(
                NodeWeight::new(
                    initial_change_set,
                    schema_variant_id,
                    NodeWeightKind::SchemaVariant(ContentHash::new("Schema Variant A".as_bytes())),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add Schema Variant A");

        initial_graph
            .add_edge(
                initial_change_set,
                initial_graph.root_index,
                EdgeWeight::default(),
                schema_index,
            )
            .expect("Unable to add root -> schema edge");
        initial_graph
            .add_edge(
                initial_change_set,
                initial_graph
                    .get_node_index_by_id(schema_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                schema_variant_index,
            )
            .expect("Unable to add schema -> schema variant edge");

        initial_graph.dot();

        let new_change_set = ChangeSet::new().expect("Unable to create ChangeSet");
        let new_change_set = &new_change_set;
        let mut new_graph = initial_graph.clone();

        let component_id = new_change_set
            .generate_ulid()
            .expect("Cannot generate Ulid");
        let component_index = new_graph
            .add_node(
                NodeWeight::new(
                    new_change_set,
                    component_id,
                    NodeWeightKind::Schema(ContentHash::new("Component A".as_bytes())),
                )
                .expect("Unable to create NodeWeight"),
            )
            .expect("Unable to add Component A");
        new_graph
            .add_edge(
                new_change_set,
                new_graph.root_index,
                EdgeWeight::default(),
                component_index,
            )
            .expect("Unable to add root -> component edge");
        new_graph
            .add_edge(
                new_change_set,
                new_graph
                    .get_node_index_by_id(component_id)
                    .expect("Cannot get NodeIndex"),
                EdgeWeight::default(),
                new_graph
                    .get_node_index_by_id(schema_variant_id)
                    .expect("Cannot get NodeIndex"),
            )
            .expect("Unable to add component -> schema variant edge");

        new_graph.dot();

        panic!();
    }
}
