//! Annotating a function must never be the thing that breaks it.
//!
//! Separate binary from `macros.rs` so that no tracker is ever installed here:
//! this is the state a user is in before calling `install`, and every generated
//! call has to evaporate.

#![cfg(all(feature = "viz", feature = "macros"))]

use pipeline_viz::{track_job, track_node};

#[track_node(kind = Transform, inputs = ["fetcher"])]
#[track_job(node = "decoder", id = number, job_type = "Block", meta(size = 4))]
fn decode(number: u64) -> Result<u64, &'static str> {
    if number == 0 {
        return Err("nothing to decode");
    }
    Ok(number * 2)
}

#[track_job(node = "committer", id = number)]
async fn commit(number: u64) -> u64 {
    number
}

#[tokio::test]
async fn annotated_functions_behave_normally_with_no_tracker_installed() {
    assert!(pipeline_viz::global().is_none(), "nothing installs here");

    assert_eq!(decode(21), Ok(42));
    assert_eq!(decode(0), Err("nothing to decode"));
    assert_eq!(commit(9_355).await, 9_355);
}
