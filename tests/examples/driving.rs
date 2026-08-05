//! Driving: a shell run one call at a time — built here, not provided.
//!
//! A REPL in full: bash that asks for work in a loop, a record family for what
//! each call did, and an answer that decides the next turn.

use mb_resolver::bash::rig::{
    converse, field, BashSrc, Capture, ExitStatus, FromRecord, Record, Reply, RigError, Setup,
    Turn,
};

use crate::{args, decoded, report, written};

/// Ask for work until told to stop. `BC_INSTR ask` returns whatever status the
/// answer's command produced, so `return 1` ends the loop.
const LOOP: &str = r#"
BC_REPL() {
    while BC_INSTR ask next; do
        :
    done
    return 0
}
"#;

/// What a dispatched call did. The step reports it before the next turn is
/// asked for, so the answer always sees the outcome of what it last chose.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    command: String,
    status: i32,
}

impl FromRecord for Ran {
    type Err = String;

    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        let words = record.behind("RAN")?;

        Some(match (field(words, "command"), field(words, "status").map(str::parse)) {
            (Some(command), Some(Ok(status))) => Ok(Self { command: command.into(), status }),
            _ => Err(format!("malformed RAN: {words:?}")),
        })
    }
}

/// The command one turn sends: run the call, then report what happened.
fn step(command: &str) -> BashSrc {
    let quoted = mb_resolver::bash::value::emit_scalar(command);

    BashSrc::raw(format!(
        "{command}\n__repl_status=$?\n\
         BC_INSTR say RAN command {quoted} status \"$__repl_status\""
    ))
}

/// Hands out `plan` in order, one per turn, then stops. The plan and how far
/// through it we are are both the closure's, so neither needs a type.
fn driving<'a>(
    plan: &'a [&'a str],
) -> impl FnMut(&Capture, &Turn) -> Result<Reply, RigError> + 'a {
    let mut handed_out = 0;

    move |_seen, asked| match plan.get(handed_out) {
        Some(command) => {
            handed_out += 1;
            asked.source(&step(command))
        }
        None => Ok(Reply::status(1)),
    }
}

#[test]
fn a_steering_function_runs_calls_and_sees_how_each_one_ended() {
    let (seen, status) = converse(
        Setup::new().bash(BashSrc::raw(LOOP)),
        &[written(&[("session.bash", "BC_REPL\nBC_INSTR say MARK after-repl\n")])],
        driving(&["BC_INSTR say MARK one", "false", "BC_INSTR say MARK two"]),
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    assert_eq!(args(&seen, "MARK"), ["one", "two", "after-repl"], "{}", report(&seen));

    // Every dispatched call reported its own status, so a later turn can
    // branch on an earlier failure.
    assert_eq!(
        decoded::<Ran>(&seen),
        [
            Ran { command: "BC_INSTR say MARK one".into(), status: 0 },
            Ran { command: "false".into(), status: 1 },
            Ran { command: "BC_INSTR say MARK two".into(), status: 0 },
        ],
        "{}",
        report(&seen)
    );
}

/// `exit` inside a dispatched call cannot be intercepted — the shell simply
/// goes. It is still detected exactly, as a call that was handed out and never
/// reported back.
#[test]
fn an_exit_inside_a_call_is_not_intercepted_but_is_detected() {
    let plan = ["BC_INSTR say MARK before", "exit 9"];
    let (seen, status) = converse(
        Setup::new().bash(BashSrc::raw(LOOP)),
        &[written(&[("session.bash", "BC_REPL\nBC_INSTR say MARK unreachable\n")])],
        driving(&plan),
    )
    .unwrap();

    // The shell went, so the loop never came back and nothing after it ran.
    assert_eq!(status, ExitStatus::Code(9));
    assert_eq!(args(&seen, "MARK"), ["before"], "{}", report(&seen));

    // Two calls were handed out; only the first ever completed. The one that
    // took the shell down is exactly the difference.
    let completed: Vec<String> =
        decoded::<Ran>(&seen).into_iter().map(|ran| ran.command).collect();
    assert_eq!(completed, ["BC_INSTR say MARK before"]);

    let abandoned: Vec<&str> = plan
        .iter()
        .copied()
        .filter(|call| !completed.iter().any(|done| done == call))
        .collect();
    assert_eq!(abandoned, ["exit 9"], "the call that took the shell down");
}
