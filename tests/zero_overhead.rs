//! Verifies the crate's headline claim instead of asserting it in prose.
//!
//! With the `viz` feature off, none of the implementation's machinery may reach
//! a user's binary — no async runtime, no serialization, and later no web
//! server. Runs only in the default (feature-off) configuration.

#![cfg(not(feature = "viz"))]

use std::process::Command;

/// Crates that must never appear in a production dependency tree.
const FORBIDDEN: &[&str] = &[
    "tokio",
    "serde",
    "serde_json",
    "axum",
    "futures-util",
    "tokio-tungstenite",
    "rust-embed",
];

#[test]
fn no_implementation_dependencies_reach_a_production_build() {
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--no-default-features",
            "--edges",
            "normal",
            "--prefix",
            "none",
        ])
        // A separate target directory keeps this from contending with the
        // cargo invocation that is running the test.
        .env("CARGO_TARGET_DIR", env!("CARGO_TARGET_TMPDIR"))
        .output()
        .expect("cargo tree runs");

    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|crate_name| {
            tree.lines()
                .any(|line| line.split_whitespace().next() == Some(crate_name))
        })
        .collect();

    assert!(
        found.is_empty(),
        "these must be compiled out when the \"viz\" feature is off, but appear \
         in the dependency tree: {found:?}\n\n{tree}"
    );
}

#[test]
fn the_public_api_still_compiles_and_does_nothing() {
    let tracker = pipeline_viz::PipelineTracker::builder()
        .bind_port(9999)
        .start_background()
        .expect("the no-op tracker always starts");

    let mut job = tracker.job("committer").id(1).job_type("Block").start();
    job.hold("Waiting for finality");
    job.update_reason("Writing to PostgreSQL");
    job.complete();

    let snapshot = tracker.snapshot();
    assert!(snapshot.nodes.is_empty());
    assert!(snapshot.jobs.is_empty());
    assert_eq!(tracker.dropped_events(), 0);
}
