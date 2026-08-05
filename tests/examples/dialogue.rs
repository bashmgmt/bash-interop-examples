//! A dialogue: several turns, in both directions, each computed from the last.
//!
//! ```text
//!   shell  BC_INSTR ask phase survey  ──ask──▶  the answer
//!   shell  ◀──a command──                       "report what you find"
//!   shell  BC_INSTR say CANDIDATE …   ──say──▶  (three of them)
//!   shell  BC_INSTR ask phase choose  ──ask──▶  reads the CANDIDATEs it was
//!   shell  ◀──a command──                       just sent, and picks one
//!   shell  BC_INSTR say CHOSEN …      ──say──▶
//!   shell  BC_INSTR ask phase verify  ──ask──▶  checks CHOSEN against its own
//!   shell  ◀──a status──                        decision and lets the script go
//! ```
//!
//! Two properties make it work. The rig keeps what it hears, so its state
//! *is* the conversation. And a command that sources a file runs in the
//! shell, so what it sets outlives the turn.

use mb_resolver::bash::rig::{
    converse, field, BashSrc, Capture, ExitStatus, FromRecord, Record, Reply, RigError, Setup, Turn,
};

use crate::{args, decoded, report, written};

const OPERATOR_BASH: &str = r#"
REFUSE() { printf 'operator: %s\n' "$1" >&2; return "$2"; }
"#;

/// What the shell found. Reported by the code the answer sent in round one.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    name: String,
    weight: usize,
}

impl FromRecord for Candidate {
    type Err = String;

    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        let words = record.behind("CANDIDATE")?;

        Some(match (field(words, "name"), field(words, "weight").map(str::parse)) {
            (Some(name), Some(Ok(weight))) => Ok(Self { name: name.into(), weight }),
            _ => Err(format!("malformed CANDIDATE: {words:?}")),
        })
    }
}

/// What the shell did about it, reported by the code sent in round two.
#[derive(Debug, PartialEq, Eq)]
struct Chosen {
    name: String,
}

impl FromRecord for Chosen {
    type Err = String;

    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        let words = record.behind("CHOSEN")?;

        Some(match field(words, "name") {
            Some(name) => Ok(Self { name: name.to_string() }),
            None => Err("no name".into()),
        })
    }
}

/// The rule the answer applies: heaviest candidate wins.
fn preferred(seen: &Capture) -> Option<String> {
    seen.decoded::<Candidate>().max_by_key(|entry| entry.value.weight).map(|entry| entry.value.name)
}

/// One answer, three turns. Which turn it is comes from the question; what to
/// say comes from what has been heard so far, so nothing is remembered
/// between turns and no type is needed to hold it.
fn operate(seen: &Capture, asked: &Turn) -> Result<Reply, RigError> {
    match asked.args() {
        // Round one: ask the shell to look around. `inspect` is defined here
        // and survives into the next round, because a sourced command runs in
        // the shell rather than in a subshell.
        [_, phase] if phase == "survey" => asked.source(&BashSrc::raw(
            r#"
            inspect() { BC_INSTR say CANDIDATE name "$1" weight "${#1}"; }
            for item in pear kiwi elderberry; do inspect "$item"; done
            "#,
        )),

        // Round two: decide from what round one reported, and send back a
        // command that acts on the decision.
        [_, phase] if phase == "choose" => match preferred(seen) {
            Some(name) => asked.source(&BashSrc::raw(format!(
                "picked={name}\nBC_INSTR say CHOSEN name \"$picked\""
            ))),
            None => Ok(Reply::of(["REFUSE", "nothing to choose from", "3"])),
        },

        // Round three: check the shell did what was decided — both halves
        // read back out of the capture.
        [_, phase] if phase == "verify" => {
            let chosen = seen.decoded::<Chosen>().next().map(|entry| entry.value.name);

            Ok(match chosen == preferred(seen) {
                true => Reply::status(0),
                false => Reply::of(["REFUSE", &format!("shell chose {chosen:?}"), "4"]),
            })
        }

        other => Ok(Reply::of(["REFUSE", &format!("unknown question {other:?}"), "2"])),
    }
}

const SESSION: &str = r#"
    BC_INSTR ask phase survey
    BC_INSTR ask phase choose
    BC_INSTR ask phase verify || exit $?
    BC_INSTR say NOTE "settled on $picked"
"#;

#[test]
fn each_turn_is_computed_from_what_the_other_side_said() {
    let (seen, status) = converse(
        Setup::new().bash(BashSrc::raw(OPERATOR_BASH)),
        &[written(&[("session.bash", SESSION)])],
        operate,
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Code(0), "{}", report(&seen));

    // Round one: the shell reported what the answer's command told it to.
    assert_eq!(
        decoded::<Candidate>(&seen),
        [
            Candidate { name: "pear".into(), weight: 4 },
            Candidate { name: "kiwi".into(), weight: 4 },
            Candidate { name: "elderberry".into(), weight: 10 },
        ],
        "{}",
        report(&seen)
    );

    // Round two: chosen from that, and the shell acted on it.
    assert_eq!(decoded::<Chosen>(&seen), [Chosen { name: "elderberry".into() }]);

    // `picked` was set by a sourced command two rounds earlier and is still
    // there, in the script's own scope.
    assert_eq!(args(&seen, "NOTE"), ["settled on elderberry"]);
}

/// The same answer, the same script — but the shell finds nothing, so the
/// decision cannot be made and the refusal walks back out as a status.
#[test]
fn a_turn_that_cannot_be_answered_refuses_and_the_script_carries_it_out() {
    let (seen, status) = converse(
        Setup::new().bash(BashSrc::raw(OPERATOR_BASH)),
        &[written(&[(
            "session.bash",
            "BC_INSTR ask phase survey\n\
             BC_INSTR ask phase choose || exit $?\n\
             BC_INSTR say NOTE unreachable\n",
        )])],
        |seen, asked| match asked.args().last().map(String::as_str) {
            Some("survey") => Ok(Reply::status(0)),
            _ => match preferred(seen) {
                Some(name) => asked.source(&BashSrc::raw(format!("picked={name}"))),
                None => Ok(Reply::of(["REFUSE", "nothing to choose from", "3"])),
            },
        },
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Code(3));
    assert!(args(&seen, "NOTE").is_empty(), "{}", report(&seen));
}
