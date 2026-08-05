//! Asking: the shell stops, and what comes back is a command it runs.
//!
//! The question goes up as an ordinary message, so it appears in the capture
//! like anything else. There is no "refused" variant — a refusal is a command
//! that says so and returns non-zero.

use mb_resolver::bash::rig::{converse, BashSrc, ExitStatus, Pid, Reply, Setup};

use crate::{args, report, written};

const OPERATOR_BASH: &str = r#"
MARK() { BC_INSTR say MARK "$@"; }
REFUSE() { printf 'operator: %s\n' "$1" >&2; return "$2"; }
"#;

/// Remembers what it was asked. `answer` takes `&mut self`, so the state is
/// a field and needs no lock.
#[test]
fn one_answer_serves_every_question_and_a_refusal_fails_fast() {
    // The state is the closure's, so nothing here needs a type of its own.
    let mut asked_at: Vec<(Pid, String)> = Vec::new();

    let (seen, status) = converse(
        Setup::new().bash(BashSrc::raw(OPERATOR_BASH)),
        &[written(&[(
            "main.bash",
            r#"
            MARK before
            BC_INSTR ask at first                # answered with a command to run
            MARK between
            BC_INSTR ask at second || exit $?    # refused; 42 travels outward
            MARK unreachable
            "#,
        )])],
        |_seen, asked| {
            let place = asked.args().last().cloned().unwrap_or_default();
            asked_at.push((asked.stamp().pid, place.clone()));

            Ok(match place.as_str() {
                // A word this rig put in the prelude, called with an argument.
                "first" => Reply::of(["MARK", "operator-supplied-this"]),

                // The client's own error convention, expressed as a command.
                _ => Reply::of(["REFUSE", "no", "42"]),
            })
        },
    )
    .unwrap();

    assert_eq!(
        args(&seen, "MARK"),
        ["before", "operator-supplied-this", "between"],
        "{}",
        report(&seen)
    );
    assert_eq!(status, ExitStatus::Code(42));

    // Both questions reached the answer, from the one shell that asked.
    let places: Vec<&str> = asked_at.iter().map(|(_, at)| at.as_str()).collect();
    assert_eq!(places, ["first", "second"]);
    assert_eq!(asked_at[0].0, asked_at[1].0);
}
