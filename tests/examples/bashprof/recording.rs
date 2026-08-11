//! The wire read as flat records.
//!
//! Pairing only. Within one shell a call either returns or takes the shell
//! down, so END closes the innermost call still open in that shell — a stack,
//! and nothing more. Where a call belongs relative to the others is
//! [`nesting`](super::nesting)'s, and it reads the stacks rather than this
//! order.

use std::collections::HashMap;

use mb_resolver::bash::rig::{field, Failure, Line, Micros, Pid};
use mb_resolver::bash::stack::Columns;

use super::record::{Call, Record};

/// The word this instrument's messages begin with.
const TAG: &str = "TIME_CPS";

#[derive(Default)]
pub struct Recording {
    /// Per shell, the calls that have begun and not ended, outermost first.
    open: HashMap<Pid, Vec<Call>>,

    settled: Vec<Record>,
}

impl Recording {
    /// One message. Anything not this instrument's is someone else's.
    pub fn hear(&mut self, said: &Line) -> Result<(), Failure> {
        let Some(payload) = said.behind(TAG) else { return Ok(()) };
        let Some((kind, rest)) = payload.split_first() else {
            return Err(reading("an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => self.begin(said, rest),
            "END" => self.end(said.pid, said.sent_at),
            other => Err(reading(format!("unknown kind {other:?}"))),
        }
    }

    fn begin(&mut self, said: &Line, rest: &[String]) -> Result<(), Failure> {
        let label = field(rest, "label").ok_or_else(|| reading("no label"))?.to_string();

        let mut frames = Columns::of(rest)?.frames()?.into_iter();
        let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

        self.open.entry(said.pid).or_default().push(Call {
            label,
            pid: said.pid,
            began: said.sent_at,
            at,
            outer: frames.collect(),
        });
        Ok(())
    }

    /// An unbalanced END is a defect in the instrument, not a shape to carry,
    /// so it ends the run rather than reaching a caller.
    fn end(&mut self, pid: Pid, ended: Micros) -> Result<(), Failure> {
        let unbalanced = || reading(format!("an END from pid {pid} with no BEGIN"));
        let call = self.open.get_mut(&pid).and_then(Vec::pop).ok_or_else(unbalanced)?;

        self.settled.push(Record::Ended { call, ended });
        Ok(())
    }

    /// Every call that began, in that order. Whatever is still open belongs to
    /// a shell that died inside it.
    pub fn records(self) -> Vec<Record> {
        let Recording { open, mut settled } = self;

        settled.extend(open.into_values().flatten().map(Record::Unended));
        settled.sort_by_key(|record| (record.call().began, record.call().pid.0));
        settled
    }
}

fn reading(what: impl Into<String>) -> Failure {
    Failure::new("reading a span", what.into())
}
