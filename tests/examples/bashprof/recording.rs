//! Building [`Recorded`] from what the shells say.
//!
//! The only stateful step, and the pairing is the whole of it. Spans nest
//! strictly within one shell — `"$@"` either returns to its caller or takes the
//! shell down — so a stack per shell pairs BEGIN with END, and nothing on the
//! wire identifies a pair.

use std::collections::HashMap;
use std::sync::Arc;

use either::Either::{Left, Right};

use mb_resolver::bash::rig::{field, Failure, Line, Micros, Pid};
use mb_resolver::bash::stack::Columns;

use super::recorded::{Call, Ended, Recorded};

/// The word this instrument's messages begin with.
const TAG: &str = "TIME_CPS";

/// A call that has begun. Its completed children accumulate here until its own
/// END arrives, or the run ends without one.
struct Building {
    call: Call,
    children: Vec<Recorded>,
}

impl Building {
    fn ended(self, ended: Micros) -> Recorded {
        Recorded { call: Right(Ended { call: self.call, ended }), children: Arc::from(self.children) }
    }

    fn unended(self) -> Recorded {
        Recorded { call: Left(self.call), children: Arc::from(self.children) }
    }
}

#[derive(Default)]
pub struct Recording {
    /// Per shell, the calls that have begun and not ended, outermost first.
    open: HashMap<Pid, Vec<Building>>,

    /// Trees whose outermost call had no open parent.
    done: Vec<Recorded>,
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

        // A call has a site. Establishing that here, once, is what lets every
        // reader of a `Call` have a `Frame` rather than a maybe.
        let mut frames = Columns::of(rest)?.frames()?.into_iter();
        let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

        self.open.entry(said.pid).or_default().push(Building {
            call: Call { label, pid: said.pid, began: said.sent_at, at, outer: frames.collect() },
            children: Vec::new(),
        });
        Ok(())
    }

    /// An unbalanced END is a defect in the instrument, not a shape to carry,
    /// so it ends the run rather than reaching a caller.
    fn end(&mut self, pid: Pid, ended: Micros) -> Result<(), Failure> {
        let unbalanced = || reading(format!("an END from pid {pid} with no BEGIN"));

        let stack = self.open.get_mut(&pid).ok_or_else(unbalanced)?;
        let node = stack.pop().ok_or_else(unbalanced)?.ended(ended);

        match stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => self.done.push(node),
        }
        Ok(())
    }

    /// The forest, in the order its calls began.
    ///
    /// A call still open when the run ended is one whose shell died inside it.
    /// Its stack becomes a chain of unended nodes, each under the one that
    /// opened it, keeping whatever completed along the way.
    pub fn recorded(self) -> Vec<Recorded> {
        let Recording { open, mut done } = self;

        let mut shells: Vec<(Pid, Vec<Building>)> = open.into_iter().collect();
        shells.sort_by_key(|(pid, _)| pid.0);

        // Each shell's leftover stack folds inward-out into one chain, and an
        // empty stack folds to nothing.
        done.extend(shells.into_iter().filter_map(|(_, stack)| {
            stack.into_iter().rev().fold(None, |inner, mut open| {
                open.children.extend(inner);
                Some(open.unended())
            })
        }));

        done.sort_by_key(|node| node.begun().began);
        done
    }
}

fn reading(what: impl Into<String>) -> Failure {
    Failure::new("reading a span", what.into())
}
