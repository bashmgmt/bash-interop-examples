//! One call, how it went, and where it sits relative to another.
//!
//! A record is flat. Where a call belongs in the tree is not stored here — it
//! is a fact about two records, and [`Call::encloses`] together with
//! [`Record::running_at`] is all of it.

use std::iter::once;

use mb_resolver::bash::rig::{Micros, Pid};
use mb_resolver::bash::stack::Frame;

/// The shell a call ran in.
///
/// A pid names a process, and a run long enough to wrap the pid space carries
/// two shells with one pid; when the shell first spoke tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shell {
    pub pid: Pid,
    pub joined_at: Micros,
}

/// A measured call, as its BEGIN reported it.
///
/// A call is made at a place in two nested structures at once — the function
/// stack and the tree of shells — and carries where it stood in each.
#[derive(Debug, Clone)]
pub struct Call {
    pub label: String,
    pub began: Micros,

    /// Where the call was made.
    pub at: Frame,

    /// The frames above that one, outermost last.
    pub outer: Vec<Frame>,

    /// The shell it ran in.
    pub shell: Shell,

    /// The shells that one was forked from, outermost last.
    pub forked_from: Vec<Shell>,
}

impl Call {
    /// The stack this call was made on, innermost first.
    fn site(&self) -> impl Iterator<Item = &Frame> {
        once(&self.at).chain(&self.outer)
    }

    pub fn depth(&self) -> usize {
        1 + self.outer.len()
    }

    /// Whether `inner` was called from somewhere inside this call. Both
    /// structures it sits in have to say so.
    pub fn encloses(&self, inner: &Call) -> bool {
        self.site_encloses(inner) && self.shell_encloses(inner)
    }

    /// This site is a suffix of `inner`'s, strictly — so no call encloses
    /// itself, and two made from one line enclose neither.
    fn site_encloses(&self, inner: &Call) -> bool {
        inner.outer.len() >= self.depth()
            && inner.outer[inner.outer.len() - self.depth()..].iter().eq(self.site())
    }

    /// `inner` ran in this call's shell or in one forked from it.
    ///
    /// A fork inherits the stack, which is what lets a call measured in a
    /// subshell belong to the call it was made from. It is also why the stack
    /// alone is not enough: two forks of one line report the same site, and
    /// only the shell says which of them a call inside belongs to.
    fn shell_encloses(&self, inner: &Call) -> bool {
        inner.shell == self.shell || inner.forked_from.contains(&self.shell)
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
    /// two turns of a loop apart, their sites and their shell being the same.
    pub fn running_at(&self, when: Micros) -> bool {
        self.call().began <= when
            && match self {
                Self::Ended { ended, .. } => when <= *ended,
                Self::Unended(_) => true,
            }
    }
}
