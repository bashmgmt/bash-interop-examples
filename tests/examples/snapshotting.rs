//! Reusing another tool's instrument: bashcap's bash and its decoder, with no
//! command line and no JSON in between.
//!
//! `bashcap run --into out.jsonl script.bash` is this rig with a file for a
//! session. Here the session is typed captures, so what is reused is exactly
//! the pair that matters — the bash that harvests a shell, and the code that
//! reads one back. The rendering comes with them: `Capture` is `Display`, and
//! `bashcap show` prints the same text.

use std::collections::HashSet;

use mb_resolver::bash::rig::{run, Doing, ExitStatus, Failure, Line, Rig};
use mb_resolver::bashcap::{Capture, BASH};

use crate::fixture;

/// The session: every snapshot, under the provenance the wire gave it.
struct Snapshots;

impl Rig for Snapshots {
    type Session = Vec<Capture>;

    /// bashcap's instrument, in every shell the subject starts.
    fn bash(&self) -> String {
        BASH.to_string()
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    /// Recognise, then decode: `None` is some other tool's message, and a
    /// snapshot that will not decode ends the run.
    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.pid))?);

        Ok(())
    }
}

#[test]
fn a_tools_instrument_is_reusable_without_its_command_line() {
    let (seen, status) = run(&Snapshots, &[fixture("bashcap_demo/demo.bash")]).unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    for (at, capture) in seen.iter().enumerate() {
        println!("[{at}] {capture}");
    }

    // The fixture is meant to be edited, so nothing here reads its line
    // numbers, its variable names, or how many snapshots it takes. What is
    // asserted holds for any script that calls `BASHCAP`.
    assert!(!seen.is_empty(), "an instrumented script took at least one snapshot");
    for capture in &seen {
        assert!(!capture.snapshot.frames.is_empty(), "pid {} says where it is", capture.pid);
        assert!(capture.snapshot.state.contains_key("shlvl"), "and which shell it is");
    }

    let shells: HashSet<u32> = seen.iter().map(|capture| capture.pid).collect();
    assert!(shells.len() > 1, "the fixture's subshell and child are shells of their own");

    // The child turns `extdebug` on itself, so its frames carry the arguments
    // they were called with. Nothing else in the fixture does.
    let (traced, bare): (Vec<_>, Vec<_>) = seen
        .iter()
        .flat_map(|capture| &capture.snapshot.frames)
        .partition(|frame| frame.args.is_some());

    assert!(!traced.is_empty(), "the traced child reported its call arguments");
    assert!(!bare.is_empty(), "and an ordinary shell reports none, rather than empty ones");
}
