//! A session that answers, and decides from what it has heard.
//!
//! An answer is a command the shell runs, so its expressiveness is bash's:
//! `return`, `declare -g`, `source`, `exit`, or any word the rig's own bash
//! defined. A refusal is a command that says so and returns non-zero.

use std::path::PathBuf;

use mb_resolver::bash::rig::{field, run, Answer, ExitStatus, Failure, Line, Rig, Startup};

use crate::support::{bash, sourcing, Scripts};

const OPERATOR_BASH: &str = r#"
MARK() { BC_INSTR say MARK "$@"; }
REFUSE() { printf 'operator: %s\n' "$1" >&2; return "$2"; }
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

    /// The words this operator answers with, defined in every shell.
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
        let phase = asked.words.last().cloned().unwrap_or_default();
        let step = self.steps.join(format!("step.{}.{}.bash", asked.pid, asked.seq));
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
                None => Ok(Answer::of(["REFUSE", "nothing to choose from", "3"])),
            },

            other => Ok(Answer::of(["REFUSE", &format!("unknown question {other:?}"), "2"])),
        }
    }
}

#[test]
fn each_turn_is_computed_from_what_the_other_side_said() {
    let scripts = Scripts::of(&[(
        "session.bash",
        r#"
        BC_INSTR ask phase survey
        BC_INSTR ask phase choose
        MARK "settled on $picked"
        BC_INSTR ask phase nonsense || exit $?
        MARK unreachable
        "#,
    )]);

    let (session, status) =
        run(&Choosing { steps: scripts.dir().to_path_buf() }, &bash(scripts.at("session.bash")))
            .unwrap()
            .whole()
            .unwrap();

    // `picked` was set by a sourced command and is still in the script's own
    // scope a turn later.
    let marks: Vec<&[String]> =
        session.heard.iter().filter_map(|line| line.behind("MARK")).collect();
    assert_eq!(marks, [["settled on elderberry"]]);

    // The refusal is a command, so its status walks back out of the script.
    assert_eq!(status, ExitStatus::Code(2));
}
