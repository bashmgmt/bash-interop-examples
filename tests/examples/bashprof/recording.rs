//! The wire read as flat records, each already placed.
//!
//! The instrument's own shape does the placing. `BASHPROF_TIME_CPS` sends
//! BEGIN, runs the call, sends END — so within one shell the calls are a
//! stack, a BEGIN belongs to whatever that shell had open, and END closes it.
//! Nothing has to be searched for, and nothing can be ambiguous: a shell's
//! open calls are a chain, never a set.
//!
//! A fork is the one thing that stack does not cover. It inherits the frames
//! but begins a stack of its own, so a call it opens with attaches into the
//! shell it was forked from — and there the inherited frames say which call,
//! since a shell that ran on after forking has since begun others they do not
//! match.

use std::iter::successors;

use mb_resolver::bash::rig;
use mb_resolver::bash::rig::{field, Failure, Line, Micros};
use mb_resolver::bash::stack::Columns;

use super::record::{Call, Placed, Record};

/// The word this instrument's messages begin with.
const TAG: &str = "TIME_CPS";

/// Every call the run made, each paired with the call it was made inside of.
///
/// Shells are read in the order they joined, which is the order they can be
/// read in: a shell's parent spoke before forking it, so by the time a fork
/// asks what it belongs to, that shell has been read.
pub fn records(heard: &[Line]) -> Result<Vec<Placed>, Failure> {
    let shells = rig::shells(heard);
    let forked_from = rig::forked_from(&shells);
    let mut opened: Vec<Open> = Vec::new();

    for (index, shell) in shells.iter().enumerate() {
        read(shell, index, &forked_from, &mut opened)?;
    }

    Ok(opened.into_iter().map(Open::settled).collect())
}

/// A call while its shell is still being read. It gains an end when its END
/// arrives, and a shell that dies inside it never sends one.
struct Open {
    shell: usize,
    call: Call,
    inside: Option<usize>,
    ended: Option<Micros>,
}

impl Open {
    fn running_at(&self, when: Micros) -> bool {
        self.call.began <= when
            && match self.ended {
                Some(ended) => when <= ended,
                None => true,
            }
    }

    fn settled(self) -> Placed {
        let Open { call, inside, ended, .. } = self;

        Placed {
            record: match ended {
                Some(ended) => Record::Ended { call, ended },
                None => Record::Unended(call),
            },
            inside,
        }
    }
}

/// One shell's messages, appended to what has been read so far.
fn read(
    shell: &rig::Shell<'_>,
    index: usize,
    forked_from: &[Option<usize>],
    opened: &mut Vec<Open>,
) -> Result<(), Failure> {
    let mut stack: Vec<usize> = Vec::new();

    for line in &shell.lines {
        let Some(payload) = line.behind(TAG) else { continue };
        let Some((kind, rest)) = payload.split_first() else {
            return Err(reading("an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => {
                let call = began(line, rest)?;
                let inside = match stack.last() {
                    Some(&enclosing) => Some(enclosing),
                    None => forked_into(&call, index, forked_from, opened),
                };

                stack.push(opened.len());
                opened.push(Open { shell: index, call, inside, ended: None });
            }

            // An unbalanced END is a defect in the instrument, not a shape to
            // carry, so it ends the run rather than reaching a caller.
            "END" => {
                let unbalanced =
                    || reading(format!("an END from pid {} with no BEGIN", shell.pid));

                opened[stack.pop().ok_or_else(unbalanced)?].ended = Some(line.sent_at);
            }

            other => return Err(reading(format!("unknown kind {other:?}"))),
        }
    }

    Ok(())
}

/// Where a call that opens its shell's stack attaches: the innermost call it
/// was made inside of that was still running, in the nearest shell this one
/// was forked from that has one.
///
/// The frames are what pick it, and here rather than anywhere else. A shell
/// blocked on a fork has the same call open the whole time; one that forked in
/// the background may have begun another since, and a call begun after the
/// fork is not one this call's inherited site was ever made under.
fn forked_into(
    call: &Call,
    shell: usize,
    forked_from: &[Option<usize>],
    opened: &[Open],
) -> Option<usize> {
    successors(forked_from[shell], |&above| forked_from[above]).find_map(|ancestor| {
        // One shell's calls are read in the order they began, so the most
        // recent of them that is still running is the innermost.
        opened
            .iter()
            .enumerate()
            .rev()
            .find(|(_, open)| {
                open.shell == ancestor
                    && open.running_at(call.began)
                    && call.made_inside(&open.call)
            })
            .map(|(index, _)| index)
    })
}

fn began(line: &Line, rest: &[String]) -> Result<Call, Failure> {
    let label = field(rest, "label").ok_or_else(|| reading("no label"))?.to_string();

    let mut frames = Columns::of(rest)?.frames()?.into_iter();
    let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

    Ok(Call { label, pid: line.pid, began: line.sent_at, at, outer: frames.collect() })
}

fn reading(what: impl Into<String>) -> Failure {
    Failure::new("reading a span", what.into())
}
