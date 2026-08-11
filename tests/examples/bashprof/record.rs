//! One call, how it went, and where it sits relative to another.
//!
//! A record is flat. Where a call belongs in the tree is not stored here — it
//! is a fact about two records, and [`Call::encloses`] together with
//! [`Record::running_at`] is all of it.

use std::iter::once;

use mb_resolver::bash::rig::{Micros, Pid};
use mb_resolver::bash::stack::Frame;

/// A measured call, as its BEGIN reported it.
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

impl Call {
    /// The stack this call was made on, innermost first.
    fn site(&self) -> impl Iterator<Item = &Frame> {
        once(&self.at).chain(&self.outer)
    }

    pub fn depth(&self) -> usize {
        1 + self.outer.len()
    }

    /// Whether `inner` was called from somewhere inside this call: this site
    /// is a suffix of `inner`'s, strictly, so no call encloses itself and two
    /// made from one line enclose neither.
    ///
    /// A stack outlives a fork, so this holds across a subshell — which is why
    /// nesting reads the stack rather than the order messages arrived in.
    pub fn encloses(&self, inner: &Call) -> bool {
        inner.outer.len() >= self.depth()
            && inner.outer[inner.outer.len() - self.depth()..].iter().eq(self.site())
    }
}

/// A call, and how it went.
///
/// This is what `Either<ParseOrCanonErr, ModAspectCanon>` is to the resolver,
/// with one difference that shapes everything downstream: which of the two a
/// record is says nothing about whether it has children. A module that failed
/// to parse has no knowable dependencies; a call the shell died inside has
/// perfectly knowable insides, and they are in the stack.
#[derive(Debug, Clone)]
pub enum Record {
    /// The shell died inside this call.
    Unended(Call),

    Ended { call: Call, ended: Micros },
}

impl Record {
    pub fn call(&self) -> &Call {
        match self {
            Self::Unended(call) => call,
            Self::Ended { call, .. } => call,
        }
    }

    /// Whether this call had begun and not yet returned at `when`. What tells
    /// two turns of a loop apart, their sites being identical.
    pub fn running_at(&self, when: Micros) -> bool {
        self.call().began <= when
            && match self {
                Self::Ended { ended, .. } => when <= *ended,
                Self::Unended(_) => true,
            }
    }
}
