//! What the shells reported — the form a run yields.
//!
//! Every call that began is here, whether or not it ended, with its children
//! either way. Nothing has been decided about it: reading it as timings is
//! [`profile`](super::profile), and a run that went badly is shown by
//! rendering this rather than by carrying a second account of it.

use std::sync::Arc;

use either::Either::{self, Left, Right};

use mb_resolver::bash::rig::{Micros, Pid};
use mb_resolver::bash::stack::Frame;

use super::render;

/// One measured call, as its BEGIN reported it.
#[derive(Debug, Clone)]
pub struct Call {
    pub label: String,
    pub pid: Pid,
    pub began: Micros,

    /// Where the call was made.
    pub at: Frame,

    /// The frames above that one, outermost last.
    pub outer: Vec<Frame>,
}

/// A call whose END arrived.
#[derive(Debug, Clone)]
pub struct Ended {
    pub call: Call,
    pub ended: Micros,
}

impl Ended {
    pub fn took(&self) -> u64 {
        self.ended.0 - self.call.began.0
    }
}

/// A call and everything called inside it.
///
/// `Left` is a call the shell died inside — no measurement comes out of it,
/// though what completed within it is still here.
///
/// Children are `Arc` because [`vec_fold`](hylic::prelude::vec_fold) clones
/// each node into its heap, so a node that owned its subtree would make every
/// walk quadratic. `resolve::pipeline::resolution::Resolution` is `Arc` for
/// the same reason.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub call: Either<Call, Ended>,
    pub children: Arc<[Recorded]>,
}

impl Recorded {
    pub fn begun(&self) -> &Call {
        match &self.call {
            Left(call) => call,
            Right(ended) => &ended.call,
        }
    }

    /// The calls in this forest that never ended, outermost first.
    pub fn unended(forest: &[Recorded]) -> Vec<&Call> {
        forest.iter().flat_map(Recorded::unended_here).collect()
    }

    fn unended_here(&self) -> Vec<&Call> {
        self.call
            .as_ref()
            .left()
            .into_iter()
            .chain(self.children.iter().flat_map(Recorded::unended_here))
            .collect()
    }

    /// The forest as it stands, ended and unended alike.
    pub fn render(forest: &[Recorded]) -> String {
        render::forest(
            forest,
            |node: &Recorded| node.children.to_vec(),
            |node: &Recorded| match &node.call {
                Right(ended) => format!("{} {} µs at {}", ended.call.label, ended.took(), ended.call.at),
                Left(call) => format!("{} NEVER ENDED at {}", call.label, call.at),
            },
        )
    }
}
