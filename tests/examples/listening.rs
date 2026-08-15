//! A session that keeps what a script says, and a decoder that reads it back.
//!
//! The rig contributes no bash: `STEP` is a word this script chose, and
//! `behind` is how a decoder claims it. Nor does it contribute a reaction —
//! `Vec<Message>` is one already, and keeping every message is all this wants.

use std::sync::Arc;

use mb_resolver::bash::rig::{
    field, heard, Driving, ExitStatus, Failure, Layout, Message, Rig, Shell, Workspace,
};

use crate::support::{bash, Scripts};

struct Keeping;

impl Rig for Keeping {
    type Reaction = Vec<Message>;

    /// No words of its own in the subject's shells.
    fn bash(&self) -> String {
        String::new()
    }

    fn workspace(&self) -> Workspace {
        Workspace::Temporary
    }

    fn joined(&self, _at: &Layout, _shell: Arc<Shell>) -> Result<Vec<Message>, Failure> {
        Ok(Vec::new())
    }
}

impl Driving for Keeping {}

#[derive(Debug, PartialEq, Eq)]
struct Step {
    name: String,
    seen: usize,
}

/// Recognise, then decode. `None` means some other tool's message, `Some(Err)`
/// means ours and malformed — which is what lets several tools share one wire
/// while a decode failure stays visible.
fn step(message: &Message) -> Option<Result<Step, String>> {
    let words = message.behind("STEP")?;
    let at = |key: &str| field(words, key).ok_or_else(|| format!("no {key:?}"));

    Some((|| {
        Ok(Step {
            name: at("name")?.to_string(),
            seen: at("seen")?.parse().map_err(|_| "seen is not a number")?,
        })
    })())
}

#[test]
fn a_script_reports_as_it_goes_and_the_run_hands_back_the_series() {
    let scripts = Scripts::of(&[(
        "collect.bash",
        r#"
            declare -a found=()

            note() {
                found+=("$1")
                BC_INSTR say STEP name "$1" seen "${#found[@]}"
            }

            note alpha
            note "beta with spaces"

            # The accumulated array goes back in one message; word boundaries
            # survive because a message is a bash array, not a joined string.
            BC_INSTR say TOTAL "${found[@]}"
            "#,
    )]);

    let ran = Keeping.run(&bash(scripts.at("collect.bash"))).unwrap().whole().unwrap();

    assert_eq!(ran.subject, ExitStatus::Code(0));
    assert_eq!(ran.shells.len(), 1, "provenance is the shape: one shell produced all of it");

    // A run folds per shell, and `heard` puts those foldings back in the order
    // the run read them. Each message comes with the shell that sent it.
    let said = heard(&ran.shells);

    let steps: Vec<Step> = said.iter().filter_map(|said| step(said.message)).map(Result::unwrap).collect();
    assert_eq!(
        steps,
        [
            Step { name: "alpha".into(), seen: 1 },
            Step { name: "beta with spaces".into(), seen: 2 },
        ]
    );

    let total: Vec<&[String]> = said.iter().filter_map(|said| said.message.behind("TOTAL")).collect();
    assert_eq!(
        total,
        [["alpha", "beta with spaces"]],
        "a message nobody wrote a decoder for is still there, as raw words"
    );
}
