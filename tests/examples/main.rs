//! Worked examples of building on `rig`, written against the public API only.
//!
//! Each file is a small, complete construction meant to be copied and changed:
//!
//! | | |
//! |---|---|
//! | [`speaking`] | a script reports; you decode what it said |
//! | [`own_tool`] | adding a tool: some bash, and one `FromRecord` |
//! | [`asking`] | the shell asks; the answer is a command it runs |
//! | [`dialogue`] | turn by turn, each computed from the last |
//! | [`driving`] | a REPL, built here rather than provided |
//! | [`nesting`] | processes and subshells, ordering and provenance |
//!
//! Run one with `cargo test --test examples -- --nocapture <name>`.

mod asking;
mod dialogue;
mod driving;
mod nesting;
mod own_tool;
mod speaking;

use std::fs;
use std::path::PathBuf;

use mb_resolver::bash::rig::{Capture, FromRecord, Rig};

/// Writes `files` to a scratch directory and returns the path of the first.
/// The scripts are read while bash runs and never after, so the directory may
/// go when this returns.
pub fn written(files: &[(&str, &str)]) -> String {
    let temp = tempfile::tempdir().unwrap();
    for (name, body) in files {
        fs::write(temp.path().join(name), body).unwrap();
    }
    temp.keep().join(files[0].0).to_string_lossy().into_owned()
}

/// A script that already lives under `__fixtures/`.
pub fn fixture(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("__fixtures").join(relative);
    assert!(path.is_file(), "missing fixture {}", path.display());

    path.to_string_lossy().into_owned()
}

/// Runs a rig of one's own.
pub fn run<R: Rig>(rig: &R, files: &[(&str, &str)]) -> R::Output {
    rig.run(&[written(files)]).unwrap_or_else(|error| panic!("{error}"))
}

/// The words behind `lead` in every message that begins with it, in global
/// time order.
pub fn args(capture: &Capture, lead: &str) -> Vec<String> {
    capture
        .chronological()
        .into_iter()
        .filter_map(|line| line.value.behind(lead))
        .map(|rest| rest.join(" "))
        .collect()
}

/// Every record of one family that decoded, in global time order.
pub fn decoded<T: FromRecord>(capture: &Capture) -> Vec<T> {
    capture.decoded::<T>().map(|entry| entry.value).collect()
}

/// Everything that happened, for an assertion message.
pub fn report(capture: &Capture) -> String {
    let lines: Vec<String> = capture
        .chronological()
        .into_iter()
        .map(|line| format!("  pid {:>7} | {}", line.stamp.pid, line.value.words.join(" ")))
        .collect();

    format!("\ncapture:\n{}", lines.join("\n"))
}
