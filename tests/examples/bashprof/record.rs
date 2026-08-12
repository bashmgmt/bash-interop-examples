//! One call, how it went, and the call it was made inside of.

use std::fmt;

use mb_resolver::bash::rig::{Micros, Pid};
use mb_resolver::bash::stack::Frame;

/// The name a shell gave one of its calls.
///
/// Opaque here — it is the shell's word, and nothing reads into it. What the
/// shell puts in it is `$BASHPID` and a count only that shell advances, which
/// is what makes it unique across a run's process tree; see
/// `tests/examples/bash/bashprof.bash` for why neither half can be dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Id(pub String);

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A measured call, as its BEGIN reported it.
#[derive(Debug, Clone)]
pub struct Call {
    pub id: Id,

    /// The call this one was made inside of, as the shell that made it said
    /// so. `None` where nothing measured encloses it.
    pub inside: Option<Id>,

    pub label: String,
    pub pid: Pid,
    pub began: Micros,

    /// Where the call was made.
    pub at: Frame,

    /// The frames above that one, outermost last. Nothing places a call by
    /// these — the shell said where it belongs — so this is what it is: one
    /// definite stack per node, whatever the tree does with it.
    pub outer: Vec<Frame>,
}

/// A call, and how it went.
///
/// This is what `Either<ParseOrCanonErr, ModAspectCanon>` is to the resolver,
/// with one difference that shapes everything downstream: which of the two a
/// record is says nothing about whether it has children. A module that failed
/// to parse has no knowable dependencies; a call the shell died inside has
/// perfectly knowable insides, and the calls made in it named it themselves.
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
