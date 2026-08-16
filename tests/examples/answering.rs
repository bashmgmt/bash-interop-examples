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
    field, Answer, Driving, ExitStatus, Failure, Layout, Message, Reacting, Rig,
    Shell,
};

use crate::support::{bash, sourcing, Scripts};

const OPERATOR_BASH: &str = r#"
MARK() {
    BC_INSTR CHOOSE say MARK "$@";
}
REFUSE() {
    printf 'operator: %s\n' "$1" >&2
    return "$2"
}
BC_JOIN CHOOSE "$1"
"#;

struct Choosing;

/// What one shell has said, which is what each of its answers is made from.
struct Conversation {
    shell: Arc<Shell>,
    dir: PathBuf,
    heard: Vec<Message>,
    asked: usize,
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

    /// A file of this session's own to write an answer's bash into, one per
    /// question this shell has asked.
    fn step(&self) -> PathBuf {
        self.dir.join(format!("step.{}.{}.bash", self.shell.pid, self.asked))
    }
}

struct Candidate {
    name: String,
    weight: usize,
}

fn candidate(message: &Message) -> Option<Candidate> {
    let words = message.behind("CANDIDATE")?;
    let weight = field(words, "weight")?.parse().ok()?;

    Some(Candidate { name: field(words, "name")?.to_string(), weight })
}

impl Rig for Choosing {
    type Reaction = Conversation;

    fn bash(&self) -> String {
        OPERATOR_BASH.to_string()
    }

    async fn joined(&self, at: &Layout, shell: Arc<Shell>) -> Result<Conversation, Failure> {
        Ok(Conversation { shell, dir: at.dir.clone(), heard: Vec::new(), asked: 0 })
    }
}

impl Reacting for Conversation {
    type Kept = Vec<Message>;

    async fn hear(&mut self, said: Message) -> Result<(), Failure> {
        self.heard.push(said);

        Ok(())
    }

    async fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        let phase = asked.words.last().cloned().unwrap_or_default();
        self.asked += 1;
        let step = self.step();
        self.heard.push(asked);

        match phase.as_str() {
            // A sourced command runs in the asking shell, so what it defines
            // outlives the turn.
            "survey" => sourcing(
                &step,
                r#"
                inspect() { BC_INSTR CHOOSE say CANDIDATE name "$1" weight "${#1}"; }
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

    async fn finish(self) -> Result<Vec<Message>, Failure> {
        Ok(self.heard)
    }
}

impl Driving for Choosing {}

#[tokio::test]
async fn each_turn_is_computed_from_what_the_other_side_said() {
    let scripts = Scripts::of(&[(
        "session.bash",
        r#"
        BC_INSTR CHOOSE ask phase survey
        BC_INSTR CHOOSE ask phase choose
        MARK "settled on $picked"
        BC_INSTR CHOOSE ask phase nonsense || exit $?
        MARK unreachable
        "#,
    )]);

    let ran = Choosing
        .run(&bash(scripts.at("session.bash")), |at| vec![at.bc_session(), at.bash_env()])
        .await
        .unwrap()
        .whole()
        .unwrap();

    let marks: Vec<&[String]> =
        ran.shells[0].kept.iter().filter_map(|message| message.behind("MARK")).collect();
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
