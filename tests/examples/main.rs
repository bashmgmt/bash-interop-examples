//! Worked examples of building on `rig`, against the public API only.
//!
//! | | |
//! |---|---|
//! | [`listening`] | a session that keeps what a script says, and decodes it |
//! | [`answering`] | a session that answers questions from what it has heard |
//! | [`streaming`] | a session that keeps nothing and holds a resource |
//! | [`snapshotting`] | reusing another tool's instrument and decoder |
//! | [`profiling`] | timing a tree of calls, and the two shapes a run can have |
//!
//! Every one of them is the same pieces: a session type, `open`, whichever of
//! `hear`/`answer`/`end` it cares about, and `bash` if it needs a word of its
//! own in the subject's shells.
//!
//! All of them drive the run from Rust. `tests/joining.rs` is the other way
//! round: a fixture script that starts a session of its own and asks it
//! questions.
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
