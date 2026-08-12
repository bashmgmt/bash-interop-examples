//! One call, how it went, and the call it was made inside of.

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

    fn depth(&self) -> usize {
        1 + self.outer.len()
    }

    /// Whether this call was made somewhere inside `outer`: `outer`'s site is
    /// a strict suffix of this one's, so no call is made inside itself and two
    /// made from one line are made inside neither.
    ///
    /// A fork inherits the frames of the shell that made it, so this holds
    /// across one — which is what lets a call measured in a subshell find the
    /// call it belongs to.
    pub fn made_inside(&self, outer: &Call) -> bool {
        self.outer.len() >= outer.depth()
            && self.outer[self.outer.len() - outer.depth()..].iter().eq(outer.site())
    }
}

/// A call, and how it went.
///
/// This is what `Either<ParseOrCanonErr, ModAspectCanon>` is to the resolver,
/// with one difference that shapes everything downstream: which of the two a
/// record is says nothing about whether it has children. A module that failed
/// to parse has no knowable dependencies; a call the shell died inside has
/// perfectly knowable insides, and the calls made in it said so themselves.
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
}

/// A record, and the call it was made inside of — an index into the list it
/// came in, or `None` where nothing measured encloses it.
#[derive(Debug, Clone)]
pub struct Placed {
    pub record: Record,
    pub inside: Option<usize>,
}
