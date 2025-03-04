#![allow(unknown_lints)]
#![warn(clippy::all)]

extern crate csv;
extern crate log;
extern crate num_traits;
extern crate oscoin_graph_api;
extern crate serde;
extern crate sprs;

use crate::adjacency::new_network_matrix;
use crate::algorithm::Normalised;
use crate::linalg::{DenseMatrix, SparseMatrix};
use crate::protocol_traits::ledger::LedgerView;
use crate::types::network::{Artifact, ArtifactType, Dependency, DependencyType};
use crate::types::Weight;
use core::fmt;
use num_traits::{Num, One, Zero};
use oscoin_graph_api::{Graph, GraphWriter};
use serde::Deserialize;
use sprs::{CsMat, TriMat, TriMatBase};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::rc::Rc;

//
// Types
//

type ProjectId = u32;
type ProjectName = String;
type Contributor = String;
// We need a way to map external indexes with an incremental index suitable
// for matrix manipulation, so we define this LocalMatrixIndex exactly for
// this purpose.
type LocalMatrixIndex = usize;

/// The `SparseMatrix` of the dependencies.
pub type DependencyMatrix<N> = SparseMatrix<N>;
/// The `SparseMatrix` of the contributions.
pub type ContributionMatrix<N> = SparseMatrix<N>;
/// The `SparseMatrix` of the maintainers.
pub type MaintenanceMatrix<N> = SparseMatrix<N>;

/// A single, deserialised row of the `{platform}_dependencies_meta.csv` file.
#[derive(Debug, Deserialize)]
pub struct DepMetaRow {
    pub id: u32,
    pub name: String,
    pub platform: String,
}

pub struct DependenciesMetadata {
    pub ids: HashSet<ProjectId>,
    pub labels: Vec<ProjectName>,
    pub project2index: HashMap<ProjectId, LocalMatrixIndex>,
}

impl DependenciesMetadata {
    pub fn new() -> Self {
        DependenciesMetadata {
            ids: Default::default(),
            labels: Default::default(),
            project2index: Default::default(),
        }
    }
}

impl Default for DependenciesMetadata {
    fn default() -> Self {
        DependenciesMetadata::new()
    }
}

pub struct ContributionsMetadata {
    pub rows: Vec<ContribRow>,
    pub contributors: HashSet<Rc<Contributor>>,
    pub contributor2index: HashMap<Rc<Contributor>, LocalMatrixIndex>,
}

impl ContributionsMetadata {
    pub fn new() -> Self {
        ContributionsMetadata {
            rows: Vec::default(),
            contributors: Default::default(),
            contributor2index: Default::default(),
        }
    }
}

impl Default for ContributionsMetadata {
    fn default() -> Self {
        ContributionsMetadata::new()
    }
}

/// A single, deserialised row of the `{platform}_contributors.csv` file.
#[derive(Debug, Deserialize)]
pub struct ContribRow {
    pub project_id: ProjectId,
    pub contributor: Contributor,
    pub repo: String,
    pub contributions: u32,
    pub project_name: ProjectName,
}

/// A single, deserialised row of the `{platform}_dependencies.csv` file.
#[derive(Debug, Deserialize)]
pub struct DepRow {
    pub from: ProjectId,
    pub to: ProjectId,
}

//
// Errors
//

/// Errors arising during the import process.
#[derive(Debug)]
pub enum CsvImportError {
    /// Returned in case of generic I/O error.
    IOError(std::io::Error),

    /// Returned when the CSV deserialisation failed.
    CsvDeserialisationError(csv::Error),
}

impl fmt::Display for CsvImportError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CsvImportError::IOError(e) => {
                write!(f, "i/o error when reading/writing on the CSV file {}", e)
            }
            CsvImportError::CsvDeserialisationError(e) => {
                write!(f, "Deserialisation failed on a CSV row {}", e)
            }
        }
    }
}

impl From<std::io::Error> for CsvImportError {
    fn from(err: std::io::Error) -> CsvImportError {
        CsvImportError::IOError(err)
    }
}

impl From<csv::Error> for CsvImportError {
    fn from(err: csv::Error) -> CsvImportError {
        CsvImportError::CsvDeserialisationError(err)
    }
}

//
// Utility Traits
//

pub trait DisplayAsF64 {
    fn to_f64(self) -> f64;
}

impl DisplayAsF64 for f64 {
    fn to_f64(self: f64) -> f64 {
        self
    }
}

impl DisplayAsF64 for Weight {
    fn to_f64(self: Weight) -> f64 {
        self.as_f64().unwrap()
    }
}

pub fn debug_sparse_matrix_to_csv<N>(
    matrix: &CsMat<N>,
    out_path: &str,
) -> Result<(), CsvImportError>
where
    N: DisplayAsF64 + Zero + Clone + Copy,
{
    debug_dense_matrix_to_csv(&matrix.to_dense(), out_path)
}

pub fn debug_dense_matrix_to_csv<N>(
    matrix: &DenseMatrix<N>,
    out_path: &str,
) -> Result<(), CsvImportError>
where
    N: DisplayAsF64 + Zero + Clone + Copy,
{
    let mut output_csv = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out_path)?;

    for row in matrix.genrows() {
        if let Some((last, els)) = row.as_slice().and_then(|e| e.split_last()) {
            for cell in els {
                output_csv.write_all(format!("{},", cell.to_f64()).as_str().as_bytes())?;
            }
            output_csv.write_all(format!("{}", last.to_f64()).as_str().as_bytes())?;
        }
        output_csv.write_all(b"\n")?;
    }

    Ok(())
}

/// Constructs a `Network` graph from a list of CSVs. The structure of each
/// csv is important, and is documented below.
///
/// #deps_csv_file
///
/// This must be a csv file in this format:
///
/// ```ignore,no_run
/// FROM_ID,TO_ID
/// 30742,31187
/// 30742,31296
/// [..]
/// ```
///
/// #deps_meta_csv_file
/// This must be a csv file in this format:
///
/// ```ignore,no_run
/// ID,NAME,PLATFORM
/// 30742,acacia,Cargo
/// 30745,aio,Cargo
/// 30746,advapi32-sys,Cargo
/// [..]
/// ```
///
/// #contrib_csv_file
/// This must be a csv file in this format:
///
/// ```ignore,no_run
/// ID,MAINTAINER,REPO,CONTRIBUTIONS,NAME
/// 30742,github@aepsil0n,https://github.com/aepsil0n/acacia,118,acacia
/// 30743,github@emk,https://github.com/emk/abort_on_panic-rs,32,abort_on_panic
/// 30745,github@reem,https://github.com/reem/rust-aio,35,aio
/// [..]
/// ```
///
/// # Using the import_network to load the Cargo ecosystem
///
/// To use the `import_network` to import a `Network` relative to the *whole*
/// Rust ecosystem, simply run
///
/// ```rust, no_run
/// use osrank::protocol_traits::ledger::MockLedger;
/// use osrank::importers::csv::import_network;
/// use osrank::types::network::Network;
/// use std::fs::File;
///
/// type MockNetwork = Network<f64>;
///
/// let mut deps_csv_file = File::open("data/cargo_dependencies.csv").unwrap();
/// let mut deps_meta_csv_file = File::open("data/cargo_dependencies_meta.csv").unwrap();
/// let mut contribs_csv_file = File::open("data/cargo_contributions.csv").unwrap();
/// let mock_ledger = MockLedger::default();
/// let network = import_network::<MockNetwork, MockLedger, File>( csv::Reader::from_reader(deps_csv_file)
///                             , csv::Reader::from_reader(deps_meta_csv_file)
///                             , csv::Reader::from_reader(contribs_csv_file)
///                             , None
///                             , &mock_ledger);
/// assert_eq!(network.is_ok(), true);
/// ```
///
pub fn import_network<G, L, R>(
    deps_csv: csv::Reader<R>,
    deps_meta_csv: csv::Reader<R>,
    mut contribs_csv: csv::Reader<R>,
    //TODO(and) We want to consider maintainers at some point.
    _maintainers_csv_file: Option<csv::Reader<R>>,
    ledger_view: &L,
) -> Result<Normalised<G>, CsvImportError>
where
    L: LedgerView,
    R: Read,
    G: Graph<
            Node = Artifact<String>,
            Edge = Dependency<usize, f64>,
            Weight = f64,
            NodeData = ArtifactType,
            EdgeData = DependencyType<f64>,
        > + GraphWriter,
{
    debug!("Starting to import a Graph from the CSV files...");

    let mut deps_meta = DependenciesMetadata::new();
    let mut contribs_meta = ContributionsMetadata::new();

    let mut graph = G::default();

    // Stores the global mapping between matrix indexes and node names,
    // which includes projects & accounts..
    let mut index2id: HashMap<usize, String> = HashMap::default();

    // Iterate once over the dependencies metadata and store the name and id.
    // We need to maintain some sort of mapping between the order of visit
    // (which will be used as the index in the matrix) and the project id.
    for result in deps_meta_csv.into_records().filter_map(|e| e.ok()) {
        let row: DepMetaRow = result.deserialize(None)?;
        let prj_id = row.name.clone();
        deps_meta.ids.insert(row.id);
        deps_meta.labels.push(row.name);
        deps_meta
            .project2index
            .insert(row.id, deps_meta.ids.len() - 1);
        index2id.insert(index2id.len(), prj_id.clone());

        // Add the projects as nodes in the graph.
        graph.add_node(
            prj_id,
            ArtifactType::Project {
                osrank: Zero::zero(),
            },
        );
    }

    debug!("Added all the projects as nodes to the graph..");

    // Iterate once over the contributions and build a matrix where
    // rows are the project names and columns the (unique) contributors.
    for result in contribs_csv.records().filter_map(|e| e.ok()) {
        let row: ContribRow = result.deserialize(None)?;
        let contributor = Rc::new(row.contributor.clone());

        if contribs_meta.contributors.get(&row.contributor).is_none() {
            contribs_meta.contributors.insert(Rc::clone(&contributor));
            contribs_meta.contributor2index.insert(
                Rc::clone(&contributor),
                contribs_meta.contributors.len() - 1,
            );

            index2id.insert(index2id.len(), Rc::clone(&contributor).to_string());

            graph.add_node(
                contributor.to_string(),
                ArtifactType::Account {
                    osrank: Zero::zero(),
                },
            );
        }

        contribs_meta.rows.push(row)
    }

    debug!("Added all the contributions as nodes to the graph..");

    let dep_adj_matrix = new_dependency_adjacency_matrix(&deps_meta, deps_csv)?;

    debug!("Generated dep_adj_matrix...");

    let con_adj_matrix =
        new_contribution_adjacency_matrix(&deps_meta, &contribs_meta, Box::new(f64::from))?;

    debug!("Generated con_adj_matrix...");

    //FIXME(adn) For now the maintenance matrix is empty.
    let maintainers_matrix = CsMat::zero((dep_adj_matrix.rows(), con_adj_matrix.cols()));

    let network_matrix = new_network_matrix(
        &dep_adj_matrix,
        &con_adj_matrix,
        &maintainers_matrix,
        &ledger_view.get_hyperparams(),
    );

    debug!("Generated the full graph adjacency matrix...");

    let mut current_edge_id = 0;

    //FIXME(adn) Here we have a precision problem: we _have_ to convert the
    //weights from fractions to f64 to avoid arithmetic overflows, but yet here
    //it's nice to work with fractions.
    for (&weight, (source, target)) in network_matrix.iter() {
        graph.add_edge(
            current_edge_id,
            &index2id.get(&source).unwrap(),
            &index2id.get(&target).unwrap(),
            weight,
            DependencyType::Influence(weight),
        );
        current_edge_id += 1;
    }

    // Build a graph out of the matrix.
    Ok(Normalised::new(graph))
}

/// Creates a (sparse) adjacency matrix for the dependencies.
/// Corresponds to the "cargo-dep-adj.csv" from the Python scripts.
pub fn new_dependency_adjacency_matrix<N, R>(
    deps_meta: &DependenciesMetadata,
    deps_csv: csv::Reader<R>,
) -> Result<DependencyMatrix<N>, CsvImportError>
where
    N: Num + Clone,
    R: Read,
{
    let mut dep_adj: TriMat<N> = TriMatBase::new((deps_meta.ids.len(), deps_meta.ids.len()));

    // Iterate through the dependencies, populating the matrix.
    for result in deps_csv.into_records().filter_map(|e| e.ok()) {
        let row: DepRow = result.deserialize(None)?;
        if let (Some(from_index), Some(to_index)) = (
            deps_meta.project2index.get(&row.from),
            deps_meta.project2index.get(&row.to),
        ) {
            dep_adj.add_triplet(*from_index as usize, *to_index as usize, One::one());
        }
    }

    Ok(dep_adj.to_csr())
}

/// Creates a (sparse) adjacency matrix for the contributions.
/// Corresponds to the "cargo-contrib-adj.csv" from the Python scripts.
pub fn new_contribution_adjacency_matrix<N>(
    deps_meta: &DependenciesMetadata,
    contribs_meta: &ContributionsMetadata,
    mk_contribution: Box<dyn Fn(u32) -> N>,
) -> Result<ContributionMatrix<N>, CsvImportError>
where
    N: Num + Clone,
{
    // Creates a sparse matrix of projects x contributors
    let mut contrib_adj: TriMat<N> =
        TriMatBase::new((deps_meta.ids.len(), contribs_meta.contributors.len()));

    for row in contribs_meta.rows.iter() {
        if let Some((row_ix, col_ix)) =
            deps_meta
                .project2index
                .get(&row.project_id)
                .and_then(|row_ix| {
                    contribs_meta
                        .contributor2index
                        .get(&row.contributor)
                        .map(|col| (*row_ix, *col))
                })
        {
            contrib_adj.add_triplet(row_ix, col_ix, mk_contribution(row.contributions))
        }
    }

    Ok(contrib_adj.to_csr())
}

#[cfg(test)]
mod tests {
    extern crate num_traits;
    extern crate tempfile;

    use crate::algorithm::Normalised;
    use crate::protocol_traits::graph::GraphExtras;
    use crate::protocol_traits::ledger::MockLedger;
    use crate::types::network::{ArtifactType, DependencyType, Network};
    use num_traits::Zero;
    use std::io::{Seek, Write};
    use tempfile::tempfile;

    #[test]
    /// This test setups the same example graph in the basic model PDF
    /// and check the result is what we expect.
    fn csv_network_import_works() {
        let deps_csv = String::from(
            r###"FROM_ID,TO_ID
0,1
2,0
2,1
"###,
        );

        let mut deps_file = tempfile().unwrap();
        deps_file.write_all(deps_csv.as_bytes()).unwrap();
        deps_file.seek(std::io::SeekFrom::Start(0)).unwrap();

        let deps_csv_meta = String::from(
            r###"ID,NAME,PLATFORM
0,foo,Cargo
1,bar,Cargo
2,baz,Cargo
"###,
        );

        let mut meta_file = tempfile().unwrap();
        meta_file.write_all(deps_csv_meta.as_bytes()).unwrap();
        meta_file.seek(std::io::SeekFrom::Start(0)).unwrap();

        let contribs_csv = String::from(
            r###"ID,MAINTAINER,REPO,CONTRIBUTIONS,NAME
0,github@john,https://github.com/foo/foo-rs,100,foo
1,github@tom,https://github.com/bar/bar-rs,30,bar
2,github@tom,https://github.com/baz/baz-rs,60,baz
2,github@alice,https://github.com/baz/baz-rs,20,baz
"###,
        );

        let mut contrib_file = tempfile().unwrap();
        contrib_file.write_all(contribs_csv.as_bytes()).unwrap();
        contrib_file.seek(std::io::SeekFrom::Start(0)).unwrap();

        let mock_ledger = MockLedger::default();

        let network: Normalised<Network<f64>> = super::import_network(
            csv::ReaderBuilder::new()
                .flexible(true)
                .from_reader(deps_file),
            csv::ReaderBuilder::new()
                .flexible(true)
                .from_reader(meta_file),
            csv::ReaderBuilder::new()
                .flexible(true)
                .from_reader(contrib_file),
            None,
            &mock_ledger,
        )
        .unwrap_or_else(|e| panic!("returned unexpected error: {}", e));

        assert_eq!(
            network.lookup_node_metadata(&String::from("foo")),
            Some(&ArtifactType::Project {
                osrank: Zero::zero()
            }),
        );

        assert_eq!(
            network.lookup_edge_metadata(&0),
            Some(&DependencyType::Influence(0.8)),
        )
    }
}
