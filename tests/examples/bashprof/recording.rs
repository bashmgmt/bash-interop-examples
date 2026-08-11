//! The wire read as flat records.
//!
//! The run's messages sort themselves into the shells that sent them, and each
//! shell's are paired on their own. Within one shell a call either returns or
//! takes the shell down, so END closes the innermost call still open there — a
//! stack, and nothing more — and whatever is left open belongs to a shell that
//! died inside it.
//!
//! Where a call belongs relative to the others is
//! [`nesting`](super::nesting)'s, and it reads the stacks and shells recorded
//! here rather than the order messages arrived in.

use std::iter::successors;

use mb_resolver::bash::rig;
use mb_resolver::bash::rig::{field, Failure, Line};
use mb_resolver::bash::stack::Columns;

use super::record::{Call, Record, Shell};

/// The word this instrument's messages begin with.
const TAG: &str = "TIME_CPS";

/// Every call the run made, in the order they began.
pub fn records(heard: &[Line]) -> Result<Vec<Record>, Failure> {
    let shells = rig::shells(heard);
    let lineage = lineage(&shells);

    let mut records: Vec<Record> = shells
        .iter()
        .zip(&lineage)
        .map(|(shell, forked_from)| calls(shell, forked_from))
        .collect::<Result<Vec<_>, _>>()?
        .concat();

    records.sort_by_key(|record| (record.call().began, record.call().shell.pid.0));
    Ok(records)
}

/// A shell that spoke, as a call records having run in it.
fn joined(shell: &rig::Shell<'_>) -> Shell {
    Shell { pid: shell.pid, joined_at: shell.opened_at }
}

/// For each shell, the shells it was forked from, innermost first. The fork
/// relation points strictly backwards, so following it up terminates.
fn lineage(shells: &[rig::Shell<'_>]) -> Vec<Vec<Shell>> {
    let forked_from = rig::forked_from(shells);

    forked_from
        .iter()
        .map(|&of| {
            successors(of, |&parent| forked_from[parent])
                .map(|index| joined(&shells[index]))
                .collect()
        })
        .collect()
}

/// One shell's messages paired into calls.
fn calls(shell: &rig::Shell<'_>, forked_from: &[Shell]) -> Result<Vec<Record>, Failure> {
    let ran_in = joined(shell);
    let mut open: Vec<Call> = Vec::new();
    let mut ended: Vec<Record> = Vec::new();

    for line in &shell.lines {
        let Some(payload) = line.behind(TAG) else { continue };
        let Some((kind, rest)) = payload.split_first() else {
            return Err(reading("an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => open.push(began(line, rest, ran_in, forked_from)?),
            "END" => {
                let unbalanced =
                    || reading(format!("an END from pid {} with no BEGIN", shell.pid));

                ended.push(Record::Ended {
                    call: open.pop().ok_or_else(unbalanced)?,
                    ended: line.sent_at,
                });
            }
            other => return Err(reading(format!("unknown kind {other:?}"))),
        }
    }

    ended.extend(open.into_iter().map(Record::Unended));
    Ok(ended)
}

fn began(
    line: &Line,
    rest: &[String],
    shell: Shell,
    forked_from: &[Shell],
) -> Result<Call, Failure> {
    let label = field(rest, "label").ok_or_else(|| reading("no label"))?.to_string();

    let mut frames = Columns::of(rest)?.frames()?.into_iter();
    let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

    Ok(Call {
        label,
        began: line.sent_at,
        at,
        outer: frames.collect(),
        shell,
        forked_from: forked_from.to_vec(),
    })
}

fn reading(what: impl Into<String>) -> Failure {
    Failure::new("reading a span", what.into())
}
