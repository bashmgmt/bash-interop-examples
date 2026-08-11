//! Reading a [`Recorded`] forest as timings — one hylic fold.
//!
//! A subtree reads as one measurement exactly when its call ended and every
//! call inside it did. That is a traverse: [`measured`] turns the children's
//! readings into their spans, or into `None` the moment one of them is not a
//! measurement. Nothing tracks whether something went wrong; the shape says.
//!
//! This is how `resolve::pipeline::outcome` reads a resolution tree of
//! `Either<Err, Valid>` as an outcome or the paths that prevent one.

use std::fmt;
use std::iter::once;

use either::Either::{Left, Right};
use hylic::prelude::{treeish, vec_fold, VecFold, VecHeap, FUSED};

use mb_resolver::bash::rig::{Micros, Pid};
use mb_resolver::bash::stack::Frame;

use super::nesting::Recorded;
use super::record::{Call, Record};
use super::render;

/// One measured call, and the ones made inside it. Every field is a fact: a
/// call that had not ended would not be here.
///
/// Where the call sits among the others is the tree; what it adds is where it
/// was made. The rest of the stack it was made on stays on its
/// [`Call`](super::record::Call), which the recorded forest still holds.
#[derive(Debug, Clone)]
pub struct Span {
    pub label: String,
    pub pid: Pid,
    pub began: Micros,
    pub ended: Micros,
    pub at: Frame,
    pub children: Vec<Span>,
}

impl Span {
    /// BEGIN to END: this span's own work and everything inside it.
    pub fn inclusive(&self) -> u64 {
        self.ended.0 - self.began.0
    }

    /// What was spent here rather than in a measured child.
    pub fn exclusive(&self) -> u64 {
        self.inclusive() - self.children.iter().map(Span::inclusive).sum::<u64>()
    }

    pub fn child(&self, label: &str) -> Option<&Span> {
        self.children.iter().find(|span| span.label == label)
    }

    /// This span and everything under it, outermost first.
    pub fn all(&self) -> Vec<&Span> {
        once(self).chain(self.children.iter().flat_map(Span::all)).collect()
    }

    fn of(call: &Call, ended: Micros, children: Vec<Span>) -> Self {
        Self {
            label: call.label.clone(),
            pid: call.pid,
            began: call.began,
            ended,
            at: call.at.clone(),
            children,
        }
    }
}

/// The measurements a recorded forest yields, outermost first.
#[derive(Debug, Clone)]
pub struct Profile {
    pub roots: Vec<Span>,
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&render::forest(
            &self.roots,
            |span: &Span| span.children.clone(),
            |span: &Span| {
                format!(
                    "{} {} µs ({} µs of its own) at {}",
                    span.label,
                    span.inclusive(),
                    span.exclusive(),
                    span.at
                )
            },
        ))
    }
}

/// The forest held calls the shell died inside, so it is not a whole profile.
///
/// The measurements that did complete come with this — they are no less true
/// for the run having ended badly — and the forest they were read from is the
/// caller's already, so this borrows it rather than carrying a second account.
#[derive(Debug)]
pub struct Unfinished<'a> {
    pub resolved: Profile,
    forest: &'a [Recorded],
}

impl Unfinished<'_> {
    pub fn unended(&self) -> Vec<&Call> {
        Recorded::unended(self.forest)
    }
}

impl fmt::Display for Unfinished<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels: Vec<&str> = self.unended().iter().map(|call| call.label.as_str()).collect();

        writeln!(f, "calls that never ended: {labels:?}")?;
        f.write_str(&Recorded::render(self.forest))
    }
}

impl std::error::Error for Unfinished<'_> {}

// ── the catamorphism ─────────────────────────────────────────────────

/// What one subtree reads as: a measurement, or — some call in it never having
/// ended — the measurements that survived it.
type Reading = Result<Span, Vec<Span>>;

/// These readings as spans, if every one of them is a measurement. `None` is
/// the only record that something below never ended, and it is the `collect`
/// that produces it.
fn measured(readings: &[Reading]) -> Option<Vec<Span>> {
    readings.iter().map(|reading| reading.as_ref().ok().cloned()).collect()
}

/// Every complete measurement in these readings, in the order they began.
fn salvage(readings: &[Reading]) -> Vec<Span> {
    let mut spans: Vec<Span> = readings
        .iter()
        .flat_map(|reading| match reading {
            Ok(span) => Left(once(span.clone())),
            Err(spans) => Right(spans.iter().cloned()),
        })
        .collect();

    spans.sort_by_key(|span| span.began);
    spans
}

/// A call that ended *around* one that did not is not a measurement either:
/// its own duration is known, but its exclusive time would count work it
/// cannot account for. So the rule is the whole subtree or none of it, which
/// is what pairing the node's own record with [`measured`] says.
fn reading() -> VecFold<Recorded, Reading> {
    vec_fold(|heap: &VecHeap<Recorded, Reading>| {
        match (&heap.node.record, measured(&heap.childresults)) {
            (Record::Ended { call, ended }, Some(children)) => {
                Ok(Span::of(call, *ended, children))
            }
            _ => Err(salvage(&heap.childresults)),
        }
    })
}

impl Profile {
    /// Read a recorded forest as measurements.
    ///
    /// Run fused: the tree is small and every node is a handful of moves, so
    /// there is nothing here a work-stealing pool would help with.
    pub fn of(forest: &[Recorded]) -> Result<Self, Unfinished<'_>> {
        let fold = reading();
        let shape = treeish(|node: &Recorded| node.children.to_vec());

        let readings: Vec<Reading> =
            forest.iter().map(|tree| FUSED.run(&fold, &shape, tree)).collect();
        let resolved = Profile { roots: salvage(&readings) };

        match measured(&readings) {
            Some(_) => Ok(resolved),
            None => Err(Unfinished { resolved, forest }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn call(label: &str, began: u64) -> Call {
        Call {
            label: label.into(),
            pid: Pid(1),
            began: Micros(began),
            at: Frame { funcname: "f".into(), source: "/x.bash".into(), lineno: 1, args: None },
            outer: Vec::new(),
        }
    }

    fn node(record: Record, children: Vec<Recorded>) -> Recorded {
        Recorded { record, children: Arc::from(children) }
    }

    /// Nesting cannot produce this — a shell that dies inside a call leaves
    /// every call it was made from open too — but the reading takes a tree,
    /// not that builder's output, and is total over one.
    ///
    /// The unended child has nothing under it, so it reads as `Err(vec![])`.
    /// Asking whether anything survived would lose it; asking whether every
    /// child is a measurement does not.
    #[test]
    fn a_call_that_ended_around_one_that_did_not_is_no_measurement_either() {
        let forest = [node(
            Record::Ended { call: call("outer", 0), ended: Micros(100) },
            vec![
                node(Record::Ended { call: call("done", 10), ended: Micros(20) }, Vec::new()),
                node(Record::Unended(call("open", 30)), Vec::new()),
            ],
        )];

        let unfinished = Profile::of(&forest).expect_err("something under it never ended");

        assert_eq!(unfinished.unended().len(), 1);
        assert_eq!(
            unfinished.resolved.roots.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
            ["done"],
            "`outer` knows its own duration but cannot account for it, so it is not a span"
        );
    }
}
