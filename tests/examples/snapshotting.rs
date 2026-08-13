//! Reusing another tool's instrument: bashcap's bash and its decoder, with no
//! command line and no JSON in between.
//!
//! `bashcap run --into out.jsonl --trace-calls bash script.bash` is this rig
//! with a file for a session. Here the session is typed captures, so what is
//! reused is exactly the pair that matters — the bash that harvests a shell,
//! and the code that reads one back. The rendering comes with them: `Capture`
//! is `Display`, and `bashcap show` prints the same text.
//!
//! `cargo test --test examples -- --nocapture snapshotting`

use std::collections::HashSet;

use mb_resolver::bash::rig::{Doing, ExitStatus, Failure, Line, Master, Rig};
use mb_resolver::bashcap::{instrument, Capture, Tracing};

use crate::fixture;
use crate::support::bash;

/// The session: every snapshot, under the provenance the wire gave it.
struct Snapshots;

impl Rig for Snapshots {
    type Session = Vec<Capture>;

    /// bashcap's instrument, in every shell the subject starts, asking for
    /// the full stack. `Tracing::Calls` is not free: `extdebug` also makes
    /// `ERR`, `DEBUG` and `RETURN` traps inherited by functions and
    /// subshells, so a subject with traps of its own behaves differently
    /// under it. That is why it is asked for rather than assumed.
    fn bash(&self) -> String {
        instrument(Tracing::Calls)
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    /// Recognise, then decode: `None` is some other tool's message, and a
    /// snapshot that will not decode ends the run.
    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.sent.pid))?);

        Ok(())
    }
}

impl Master for Snapshots {}

#[test]
fn a_tools_instrument_is_reusable_without_its_command_line() {
    let (seen, status) =
        Snapshots.run(&bash(fixture("bashcap_demo/demo.bash"))).unwrap().whole().unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    for (at, capture) in seen.iter().enumerate() {
        println!("[{at}] {capture}");
    }

    // The fixture is meant to be edited: what follows holds for any script
    // that calls `BASHCAP`, and reads none of its lines, names or counts.
    assert!(!seen.is_empty(), "an instrumented script took at least one snapshot");

    let frames: Vec<_> = seen.iter().flat_map(|capture| capture.snapshot.stack.frames()).collect();
    for capture in &seen {
        assert!(!capture.snapshot.stack.frames().collect::<Vec<_>>().is_empty(), "pid {} says where it is", capture.sent.pid);
        assert!(capture.snapshot.state.contains_key("shlvl"), "and which shell it is");
    }

    let shells: HashSet<u32> = seen.iter().map(|capture| capture.sent.pid.0).collect();
    assert!(shells.len() > 1, "the fixture's subshell and child are shells of their own");

    // `BASH_ENV` reaches every shell; a command line would have reached only
    // the first.
    assert!(
        frames.iter().all(|frame| frame.args.is_some()),
        "asking for the full stack gets it in every shell, not just where the run started"
    );
    assert!(
        frames.iter().any(|frame| frame.args.as_deref().is_some_and(|args| !args.is_empty())),
        "and the arguments are the real ones, not empty lists"
    );
}
