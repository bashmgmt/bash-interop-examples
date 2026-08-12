//! Test to show how one drives an incremental "Sync" session in BashMgmt.
//! "Sync" is an aspect there that defines how modules are synced.
//! The difficulty is: rust drives module graph resolution because it's more efficient and
//! parallelizable to that end; but in bash, everything is defined (for flexibility). thus:
//!
//! - rust:
//!   - resolves the graph (e.g. from a `__topmodule__@Env` spec)
//!       - may notice that a dependency is missing and attempt dynamic resolution
//!       - to that end it would use the `Sync` aspect of all topmodules --> `__topmodule__@Sync`
//!         - it instruments then, a side session with "partial resolution": that means:
//!           - starts up that BashMgmt session with a non-default amended `mbdev` (resolver) call
//!             - mbdev will be the same rust mechanism recursively, that resolves modules
//!             - But: it knows to 'partially' resolve (with persistent state)
//!             - the underlying BashMgmt "Exec" session will be a multi-shot linearization:
//!               - where modules are present, the Sync aspect scope calls will be run
//!               - the information about more deeply nested modules will be available and funneled
//!               back by virtue of the bash counterpart calling to the `Cap`'s instrumented run.
//!               That Cap is aware (has configured! non-default amended mbdev!) of the partial
//!               resolution and channels the partial Sync info to the rust side of partial
//!               resolution, and "continues" it. the diff (bottom-growing layer of the graph) of
//!               the linearization is handed back. Sync as a regular uppercase aspect, only deals
//!               with aspect's (Sync's) scopes' regular MRO bash methods (sophisticated bash
//!               CPS-based Lisp-CLOS-style dispatch) -> the Sync aspect's scope is already on the
//!               stack, calling the Cap's channel -> can just run the backchanneled additional MRO
//!               methods (the diff).
//!               - what this gives, is, the rust continuous mbdev resolution session iteratively
//!               gains the missing Sync info,
//!                 - materializes missing modules if now present
//!                 - linearizes their Sync aspects continuedly and backchannels the aspect
//!                 sourcing plus a "give me back the result of that linearization (tail of the
//!                 Sync entries)" -> can resolve further or recognize failure/success.
//!                 - while doing so it can just also, channel back a "run this bash based Sync
//!                 call that is specified in a Sync entry to materialize a module", because as an
//!                 aspect, Sync can define and mix in, also, secondary aspects (e.g. GitSync,
//!                 RSync, etc, that define how such Sync entries are actually materialized;
//!                 checks/installs required tools, SSH connections, etc).
//!         - the cool thing is: we have Bash->Rust ICP completely built and solved! w.r.t. partial
//!         resolution, bash can just use a BC_INSTR "ask" call and as an answer, receive the
//!         required info from the Cap that instruments it, and that runs in the rust side of the
//!         partial resolution.
//! 
//!
//! An answer is a command the shell runs, so its expressiveness is bash's:
//! `return`, `declare -g`, `source`, `exit`, or any word the rig's own bash
//! defined. A refusal is a command that says so and returns non-zero.

use std::path::PathBuf;

use mb_resolver::bash::rig::{field, run, Answer, ExitStatus, Failure, Line, Rig, Startup};

use crate::support::{bash, sourcing, Scripts};

const OPERATOR_BASH: &str = r#"
MARK() {
    BC_INSTR say MARK "$@";
}
REFUSE() {
    printf 'operator: %s\n' "$1" >&2
    return "$2"
}
"#;

/// An answer names a path; this decides which. The core has no opinion.
struct Choosing {
    steps: PathBuf,
}

/// The session: what has been heard, which is what each decision is made from.
#[derive(Default)]
struct Conversation {
    heard: Vec<Line>,
}

impl Conversation {
    /// The rule: heaviest candidate wins.
    fn preferred(&self) -> Option<String> {
        self.heard
            .iter()
            .filter_map(candidate)
            .max_by_key(|found| found.weight)
            .map(|found| found.name)
    }
}

struct Candidate {
    name: String,
    weight: usize,
}

fn candidate(line: &Line) -> Option<Candidate> {
    let words = line.behind("CANDIDATE")?;
    let weight = field(words, "weight")?.parse().ok()?;

    Some(Candidate { name: field(words, "name")?.to_string(), weight })
}

impl Rig for Choosing {
    type Session = Conversation;

    fn startup(&self) -> Startup {
        Startup { bash: OPERATOR_BASH.to_string(), ..Default::default() }
    }

    fn open(&self) -> Result<Conversation, Failure> {
        Ok(Conversation::default())
    }

    fn hear(&self, session: &mut Conversation, said: Line) -> Result<(), Failure> {
        session.heard.push(said);

        Ok(())
    }

    fn answer(&self, session: &mut Conversation, asked: Line) -> Result<Answer, Failure> {
        // taken from the generic "answering" example unchanged for now

        let phase = asked.words.last().cloned().unwrap_or_default();
        let step = self.steps.join(format!("step.{}.{}.bash", asked.sent.pid, asked.sent.seq));
        session.heard.push(asked);

        match phase.as_str() {
            // A sourced command runs in the asking shell, so what it defines
            // outlives the turn.
            "survey" => sourcing(
                &step,
                r#"
                inspect() { BC_INSTR say CANDIDATE name "$1" weight "${#1}"; }
                for item in pear kiwi elderberry; do inspect "$item"; done
                "#,
            ),

            "choose" => match session.preferred() {
                Some(name) => sourcing(&step, &format!("picked={name}")),
                None => Ok(Answer::of("REFUSE", ["nothing to choose from", "3"])),
            },

            other => Ok(Answer::of("REFUSE", [format!("unknown question {other:?}"), "2".into()])),
        }
    }
}

#[test]
fn test_sync_protocol() {
    
    eprintln!("test_sync_protocol: start");
}

