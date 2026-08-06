//! Reusing another tool's instrument: bashcap's bash and its decoder, with no
//! command line and no JSON in between.
//!
//! `bashcap run --into out.jsonl script.bash` is this rig with a file for a
//! session. Here the session is typed snapshots, so what is reused is exactly
//! the pair that matters — the bash that harvests a shell, and the code that
//! reads one back.

use std::collections::HashSet;

use mb_resolver::bash::rig::{run, Doing, ExitStatus, Failure, Line, Pid, Rig};
use mb_resolver::bashcap::{Snapshot, Value, BASH};

use crate::fixture;

/// The session: every snapshot, with the shell that took it. Provenance comes
/// from the wire, so nothing about it is bashcap's to supply.
struct Snapshots;

impl Rig for Snapshots {
    type Session = Vec<(Pid, Snapshot)>;

    /// bashcap's instrument, in every shell the subject starts.
    fn bash(&self) -> String {
        BASH.to_string()
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    /// Recognise, then decode: `None` is some other tool's message, and a
    /// snapshot that will not decode ends the run.
    fn hear(&self, snaps: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Snapshot::of(&said) else { return Ok(()) };

        snaps.push((said.pid, decoded.doing(|| format!("a snapshot from pid {}", said.pid))?));

        Ok(())
    }
}

#[test]
fn a_tools_instrument_is_reusable_without_its_command_line() {
    let (snaps, status) = run(&Snapshots, &[fixture("bashcap_demo/demo.bash")]).unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    println!("{}", render(&snaps));

    // The fixture is meant to be edited, so nothing here reads its line
    // numbers, its variable names, or how many snapshots it takes. What is
    // asserted holds for any script that calls `BASHCAP`.
    assert!(!snaps.is_empty(), "an instrumented script took at least one snapshot");
    for (pid, snapshot) in &snaps {
        assert!(!snapshot.frames.is_empty(), "pid {pid} says where it is");
        assert!(snapshot.state.contains_key("shlvl"), "pid {pid} says which shell it is");
    }

    let shells: HashSet<Pid> = snaps.iter().map(|(pid, _)| *pid).collect();
    assert!(shells.len() > 1, "the fixture's subshell and child are shells of their own");
}

/// The trace the shipped binary renders from its JSON, from the typed values
/// instead.
fn render(snaps: &[(Pid, Snapshot)]) -> String {
    let lines = snaps.iter().enumerate().flat_map(|(at, (pid, snapshot))| {
        let shlvl = snapshot.state.get("shlvl").map_or("?", String::as_str);

        [format!("[{at}] pid {pid} shlvl {shlvl}"), format!("    {}", stack(snapshot))]
            .into_iter()
            .chain(snapshot.notes.iter().map(|note| format!("    note  {note}")))
            .chain(snapshot.vars.iter().map(|(name, var)| {
                let attrs = if var.attrs.is_empty() { "--" } else { &var.attrs };
                format!("    var   {name} [{attrs}] {}", shown(&var.value))
            }))
            .chain(
                (!snapshot.rematch.is_empty())
                    .then(|| format!("    regex {}", snapshot.rematch.join(" | "))),
            )
    });

    lines.collect::<Vec<_>>().join("\n")
}

/// `innermost@file:line <- … <- main@file:line`.
fn stack(snapshot: &Snapshot) -> String {
    snapshot
        .frames
        .iter()
        .map(|frame| {
            let file = frame.source.rsplit('/').next().unwrap_or(&frame.source);
            format!("{}@{file}:{}", frame.funcname, frame.lineno)
        })
        .collect::<Vec<_>>()
        .join(" <- ")
}

fn shown(value: &Value) -> String {
    match value {
        Value::Scalar(text) => text.clone(),
        Value::Indexed(items) => format!("{items:?}"),
        Value::Assoc(items) => format!("{items:?}"),
    }
}
