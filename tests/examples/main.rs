//! Worked examples of building on `rig`, against the public API only.
//!
//! | | |
//! |---|---|
//! | [`listening`] | a session that keeps what a script says, and decodes it |
//! | [`answering`] | a session that answers questions from what it has heard |
//! | [`streaming`] | a session that keeps nothing and holds a resource |
//! | [`snapshotting`] | reusing another tool's instrument and decoder |
//! | [`bashprof`] | timing a tree of calls, sharing the frame walk |
//!
//! Every one of them is the same pieces: a session type, `open`, whichever of
//! `hear`/`answer`/`end` it cares about, and `bash` if it needs a word of its
//! own in the subject's shells.
//!
//! `cargo test --test examples -- --nocapture <name>`

mod answering;
mod listening;
mod snapshotting;
mod streaming;
mod sync_protocol;
mod bashprof;

#[path = "../support/mod.rs"]
mod support;

/// A script under `__fixtures/`, by path from the crate root.
pub fn fixture(relative: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("__fixtures").join(relative)
}
