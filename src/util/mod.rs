pub mod quickcheck;

use num_traits::Zero;
use oscoin_graph_api::{types, Edge, Graph, GraphWriter, Id, Node};

// General helper functions to build graphs slightly easily.

pub fn add_projects<G, E, W>(graph: &mut G, ns: impl Iterator<Item = (E, Option<u32>)>)
where
    <G as Graph>::Node: Node<types::NodeData<W>>,
    G: GraphWriter<NodeData = types::NodeData<W>>,
    W: Zero,
    E: Into<Id<G::Node>>,
{
    for (project_id, contribs) in ns.into_iter() {
        graph.add_node(
            project_id.into(),
            types::NodeData {
                node_type: types::NodeType::Project,
                total_contributions: contribs,
                rank: Zero::zero(),
            },
        )
    }
}

pub fn add_users<G, E, W>(graph: &mut G, ns: impl Iterator<Item = (E, Option<u32>)>)
where
    <G as Graph>::Node: Node<types::NodeData<W>>,
    G: GraphWriter<NodeData = types::NodeData<W>>,
    W: Zero,
    E: Into<Id<G::Node>>,
{
    for (user_id, contribs) in ns.into_iter() {
        graph.add_node(
            user_id.into(),
            types::NodeData {
                node_type: types::NodeType::User,
                total_contributions: contribs,
                rank: Zero::zero(),
            },
        )
    }
}

pub fn add_edges<G, S, T, W>(
    graph: &mut G,
    es: impl Iterator<Item = (Id<G::Edge>, S, T, types::EdgeData<W>)>,
) where
    <G as Graph>::Edge: Edge<W, Id<G::Node>, types::EdgeData<W>>,
    G: GraphWriter<EdgeData = types::EdgeData<W>>,
    <G as Graph>::Edge: Edge<<G as Graph>::Weight, Id<G::Node>, types::EdgeData<W>>,
    S: Into<Id<G::Node>>,
    T: Into<Id<G::Node>>,
{
    for (edge_id, source, target, data) in es.into_iter() {
        graph.add_edge(edge_id, &source.into(), &target.into(), data)
    }
}
