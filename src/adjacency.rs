#![allow(unknown_lints)]
#![warn(clippy::all)]

extern crate num_traits;
extern crate serde;
extern crate sprs;

use crate::linalg::{hadamard_mul, normalise_rows, transpose_storage_csr, SparseMatrix};
use crate::types::{HyperParams, Weight};
use num_traits::{Num, Signed};
use sprs::binop::scalar_mul_mat;
use sprs::{hstack, vstack, CsMat};

/// Builds a new (normalised) network graph adjacency matrix.
pub fn new_network_matrix<N>(
    dep_matrix: &SparseMatrix<N>,
    contrib_matrix: &SparseMatrix<N>,
    maintainer_matrix: &SparseMatrix<N>,
    hyperparams: &HyperParams,
) -> SparseMatrix<N>
where
    N: Num + Copy + Default + From<Weight> + PartialOrd + Signed,
{
    debug!("Generating contrib_t...");
    let contrib_t = transpose_storage_csr(&contrib_matrix);

    debug!("Generating contrib_t_norm...");
    let contrib_t_norm = normalise_rows(&contrib_t);

    debug!("Generating maintainer_t and maintainer_norm...");
    let maintainer_t = transpose_storage_csr(&maintainer_matrix);
    let maintainer_norm = normalise_rows(&maintainer_matrix);

    debug!("Generating project2project matrix...");
    let project_to_project = scalar_mul_mat(
        &normalise_rows(&dep_matrix),
        hyperparams.depend_factor.into(),
    );

    debug!("Generating project2account matrix...");
    let project_to_account = &scalar_mul_mat(&maintainer_norm, hyperparams.maintain_factor.into())
        + &scalar_mul_mat(
            &normalise_rows(&contrib_matrix),
            hyperparams.contrib_factor.into(),
        );

    debug!("Generating account2project matrix...");
    let a1 = scalar_mul_mat(&contrib_t_norm, hyperparams.contrib_prime_factor.into());
    let account_to_project = &hadamard_mul(
        &scalar_mul_mat(&maintainer_t, hyperparams.maintain_prime_factor.into()),
        &contrib_t_norm,
    ) + &a1;

    let account_to_account: SparseMatrix<N> =
        CsMat::zero((contrib_matrix.cols(), contrib_matrix.cols()));

    debug!("Joining the matrixes...");
    // Join the matrixes together
    let q1_q2 = hstack(&vec![project_to_project.view(), project_to_account.view()]);
    let q3_q4 = hstack(&vec![account_to_project.view(), account_to_account.view()]);

    debug!("Normalise everything...");
    normalise_rows(&vstack(&vec![q1_q2.view(), q3_q4.view()]))
}
