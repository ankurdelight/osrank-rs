#![allow(unknown_lints)]
#![warn(clippy::all)]

extern crate oscoin_graph_api;

use crate::exporters::csv::{export_rank_to_csv, CsvExporterError};
use crate::exporters::Exporter;
use crate::types::network::{Artifact, DependencyType, Network};
use crate::types::Osrank;
use crate::util::quickcheck::frequency;
use fraction::ToPrimitive;
use oscoin_graph_api::{Graph, GraphAnnotator, GraphObject, GraphWriter};
use quickcheck::{Arbitrary, Gen};
use rand::Rng;
use std::collections::HashMap;
use std::hash::Hash;

pub type MockNetwork = Network<f64>;

/// Equivalent to `newtype Mock a = Mock a` in Haskell.
///
/// Useful for defining some trait which operates over mocks implementation only.
pub struct Mock<A> {
    pub unmock: A,
}

#[derive(Debug)]
struct ArbitraryEdge<'a> {
    source: &'a String,
    target: &'a String,
    id: usize,
    weight: f64,
    data: DependencyType<f64>,
}

impl Arbitrary for MockNetwork {
    // Tries to generate an arbitrary Network.
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let mut graph = Network::default();
        let nodes: Vec<Artifact<String>> = Arbitrary::arbitrary(g);

        let edges = arbitrary_normalised_edges_from(g, &nodes);

        for n in &nodes {
            graph.add_node(n.id().clone(), n.data().clone())
        }

        for e in edges {
            graph.add_edge(e.id, e.source, e.target, e.weight, e.data)
        }

        graph
    }
}

#[derive(Clone)]
enum NewEdgeAction {
    SkipNode,
    UseNode,
}

impl Arbitrary for NewEdgeAction {
    // Tries to generate an arbitrary Network.
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let choices = vec![(80, NewEdgeAction::UseNode), (20, NewEdgeAction::SkipNode)];
        frequency(g, choices)
    }
}

/// Attempts to generate a vector of random edges that respect the osrank
/// invariant, i.e. that the sum of the weight of the outgoing ones from a
/// certain node is 1.
fn arbitrary_normalised_edges_from<'a, G: Gen + Rng>(
    g: &mut G,
    nodes: &'a [Artifact<String>],
) -> Vec<ArbitraryEdge<'a>> {
    let mut edges = Vec::new();
    let mut id_counter = 0;

    for node in nodes {
        let action: NewEdgeAction = Arbitrary::arbitrary(g);
        match action {
            NewEdgeAction::SkipNode => continue,
            NewEdgeAction::UseNode => {
                // Pick a set of random nodes (it can include this node as
                // well) and generate a bunch of edges between them.

                let edges_num = g.gen_range(1, 6); // Up to 5 outgoing edges
                let node_ixs = (0..edges_num)
                    .map(|_| g.gen_range(0, nodes.len()))
                    .collect::<Vec<usize>>();

                for ix in node_ixs {
                    let w = 1.0 / f64::from(edges_num);

                    edges.push(ArbitraryEdge {
                        id: id_counter,
                        source: &node.id(),
                        target: &nodes[ix].id(),
                        weight: w,
                        data: DependencyType::Influence(w),
                    });

                    id_counter += 1;
                }
            }
        }
    }

    edges
}

/// A mock `GraphAnnotator` that stores the state into a dictionary
/// (typically, an `HashMap`).
pub struct KeyValueAnnotator<K, V> {
    pub annotator: HashMap<K, V>,
}

impl<K, V> GraphAnnotator for KeyValueAnnotator<K, V>
where
    K: Eq + Hash,
{
    type Annotation = (K, V);
    fn annotate_graph(&mut self, note: Self::Annotation) {
        self.annotator.insert(note.0, note.1);
    }
}

/// A `MockAnnotator` monomorphic over a graph `G`.
pub type MockAnnotator<G> = KeyValueAnnotator<<<G as Graph>::Node as GraphObject>::Id, Osrank>;

impl Default for MockAnnotator<MockNetwork> {
    fn default() -> Self {
        KeyValueAnnotator {
            annotator: Default::default(),
        }
    }
}

pub struct MockAnnotatorCsvExporter<'a> {
    pub annotator: MockAnnotator<MockNetwork>,
    pub out_path: &'a str,
}

impl<'a> MockAnnotatorCsvExporter<'a> {
    pub fn new(annotator: MockAnnotator<MockNetwork>, out_path: &'a str) -> Self {
        MockAnnotatorCsvExporter {
            annotator,
            out_path,
        }
    }
}

impl<'a> Exporter for MockAnnotatorCsvExporter<'a> {
    type ExporterOutput = ();
    type ExporterError = CsvExporterError;
    fn export(self) -> Result<Self::ExporterOutput, Self::ExporterError> {
        export_rank_to_csv(
            self.annotator.annotator.into_iter(),
            Box::new(|v: Osrank| v.to_f64().unwrap_or(0.0)),
            self.out_path,
        )
    }
}
