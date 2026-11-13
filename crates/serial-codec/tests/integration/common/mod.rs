//! Shared helpers for the integration tests: proptest configuration, read-chunking
//! helpers and the model-test runner.
#![allow(dead_code, unused_imports)]

pub use proptest::collection::vec;
pub use proptest::prelude::*;
pub use proptest::sample::select;
use proptest::test_runner::{Config, TestCaseResult, TestRunner};
use std::fmt::Debug;

/// `cases` cases, no regression files (shrunk failures are printed instead).
pub fn cases(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Run a model test: `check` gets one generated action sequence (and parameters). On
/// failure proptest shrinks the sequence; the panic message is the minimal scenario.
pub fn run_model<S>(n: u32, strategy: S, check: impl Fn(S::Value) -> TestCaseResult)
where
    S: Strategy,
    S::Value: Debug,
{
    let mut runner = TestRunner::new(Config {
        cases: n,
        failure_persistence: None,
        ..Config::default()
    });
    if let Err(e) = runner.run(&strategy, check) {
        panic!("{e}");
    }
}

/// Split `data` into reads whose sizes cycle through `sizes` (each at least 1).
pub fn chunks<'a>(data: &'a [u8], sizes: &[usize]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut rest = data;
    let mut i = 0;
    while !rest.is_empty() {
        let n = sizes
            .get(i % sizes.len().max(1))
            .copied()
            .unwrap_or(rest.len())
            .clamp(1, rest.len());
        out.push(&rest[..n]);
        rest = &rest[n..];
        i += 1;
    }
    out
}

/// Deterministic permutation of `0..n` from a seed.
pub fn permutation(n: usize, mut seed: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        v.swap(i, (seed >> 33) as usize % (i + 1));
    }
    v
}
