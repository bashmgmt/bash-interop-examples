//! Worked examples of building on `rig`, against the public API only.
//!
//! | | |
//! |---|---|
//! | [`listening`] | keeping what a script says, and decoding it |
//! | [`answering`] | answering questions from what one shell has said |
//! | [`streaming`] | keeping nothing, and holding a resource every shell shares |
//! | [`snapshotting`] | reusing another tool's instrument and decoder |
//! | [`profiling`] | timing a tree of calls, and the two shapes a run can have |
//!
//! Every one of them is the same pieces: a rig saying what bash the subject
//! gets and how a reaction is built once a shell is there, and a reaction with
//! whichever of `hear`/`answer`/`finish` it cares about.
//!
//! All of them drive the run from Rust. `tests/joined/merging` is the other
//! way round — a fixture script that starts a session of its own, two shells
//! merged into one view, written into an array the client named. A script that
//! wants what a shipped tool already does starts that instead: `bashprof
//! serve`, covered in `tests/cli.rs`.
//!
//! `cargo test --test examples -- --nocapture <name>`

mod answering;
mod listening;
mod snapshotting;
mod streaming;
mod profiling;

#[path = "../support/mod.rs"]
mod support;

/// A script under `__fixtures/`, by path from the crate root.
pub fn fixture(relative: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("__fixtures").join(relative)
}
