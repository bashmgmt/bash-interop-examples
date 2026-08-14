//! A session that answers, and decides from what it has heard.
//!
//! An answer is a command the shell runs, so its expressiveness is bash's:
//! `return`, `declare -g`, `source`, `exit`, or any word the rig's own bash
//! defined. A refusal is a command that says so and returns non-zero.
//!
//! One reaction per shell, so the conversation each answer is computed from is
//! that shell's own — and where an answer's bash goes is the session's own
//! workspace, which every reaction is handed at construction.

use std::path::PathBuf;
use std::sync::Arc;

use mb_resolver::bash::rig::{
    field, Answer, ExitStatus, Failure, Laid, Line, Master, Reacting, Rig, Shell,
};

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

struct Choosing;

/// What one shell has said, which is what each of its answers is made from.
struct Conversation {
    shell: Arc<Shell>,
    dir: PathBuf,
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

    /// A file of this session's own to write an answer's bash into.
    fn step(&self, seq: u32) -> PathBuf {
        self.dir.join(format!("step.{}.{seq}.bash", self.shell.pid))
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
    type Attending = Conversation;

    fn bash(&self) -> String {
        OPERATOR_BASH.to_string()
    }

    fn joined(&self, at: &Laid, shell: Arc<Shell>) -> Result<Conversation, Failure> {
        Ok(Conversation { shell, dir: at.dir.clone(), heard: Vec::new() })
    }
}

impl Reacting for Conversation {
    type Kept = Vec<Line>;

    fn hear(&mut self, said: Line) -> Result<(), Failure> {
        self.heard.push(said);

        Ok(())
    }

    fn answer(&mut self, asked: Line) -> Result<Answer, Failure> {
        let phase = asked.words.last().cloned().unwrap_or_default();
        let step = self.step(asked.sent.seq);
        self.heard.push(asked);

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

            "choose" => match self.preferred() {
                Some(name) => sourcing(&step, &format!("picked={name}")),
                None => Ok(Answer::of("REFUSE", ["nothing to choose from", "3"])),
            },

            other => Ok(Answer::of("REFUSE", [format!("unknown question {other:?}"), "2".into()])),
        }
    }

    fn finish(self) -> Result<Vec<Line>, Failure> {
        Ok(self.heard)
    }
}

impl Master for Choosing {}

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

    let ran = Choosing.run(&bash(scripts.at("session.bash"))).unwrap().whole().unwrap();

    let marks: Vec<&[String]> =
        ran.shells[0].kept.iter().filter_map(|line| line.behind("MARK")).collect();
    assert_eq!(
        marks,
        [["settled on elderberry"]],
        "a sourced answer set `picked` in the script's own scope, a turn earlier"
    );

    assert_eq!(
        ran.subject,
        ExitStatus::Code(2),
        "a refusal is a command, so its status walks out"
    );
}
