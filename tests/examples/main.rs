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
//! gets — its words, and the joins that connect them — how its shells are
//! reached (the run's environment closure: the usual pair, or less), and how a
//! reaction is built once a shell is there; and a reaction with whichever of
//! `hear`/`answer`/`finish` it cares about. The tests are `#[tokio::test]`: a
//! run is a future.
//!
//! All of them drive the run from Rust. `tests/joined/merging` is the other
//! way round — a fixture script that starts a session of its own, two shells
//! merged into one view, written into an array the client named. A script that
//! wants what a shipped tool already does starts that instead: `bashprof
//! serve`, covered in `tests/cli.rs`.
//!
//! `cargo test --test examples -- --nocapture <name>` — narration goes
//! through the env logger, `info` by default, `RUST_LOG` to filter.

mod answering;
mod listening;
mod profiling;
mod snapshotting;
mod streaming;

mod support {
    use std::ffi::OsString;

    use bash_interop::rig::Layout;
    pub use bash_interop::scratch::{Scripts, bash, sourcing};

    /// The tools' convention for the by-hand reach: the workspace directory
    /// under a name of the client's own — a spelling, consulted by nothing in the
    /// core. Scripts load the pieces and initiate by it.
    #[allow(dead_code)] // each example uses its own subset
    pub fn listening_session(at: &Layout) -> (OsString, OsString) {
        (
            OsString::from("LISTENING_SESSION"),
            OsString::from(at.text()),
        )
    }

    /// Test logging: `RUST_LOG` filters, `info` by default, captured per test.
    #[allow(dead_code)] // each example uses its own subset
    pub fn logging() {
        let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .is_test(true)
            .try_init();
    }
}

/// A script under `__fixtures/`, by path from the crate root.
pub fn fixture(relative: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("__fixtures")
        .join(relative)
}
