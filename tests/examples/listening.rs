//! A session that keeps what a script says, and a decoder that reads it back.
//!
//! The rig contributes two words, `STEP` and `TOTAL`, each an alias over
//! `BC_SAY`; `behind` is how a decoder claims one. It contributes no reaction —
//! `Vec<Message>` is one already, and keeping every message is all this wants.
//! The run decides how its shells find the session, and each test below states
//! that as the run's environment closure.

use std::sync::Arc;

use bash_interop::rig::{Driving, ExitStatus, Failure, Layout, Message, Provision, Rig, Shell, field, heard};

use crate::support::{Scripts, bash};

struct Keeping;

impl Rig for Keeping {
    type Reaction = Vec<Message>;

    /// The two words this script speaks.
    fn bash(&self, _at: &Layout) -> String {
        crate::saying("STEP", "KEEP") + &crate::saying("TOTAL", "KEEP")
    }

    async fn joined(&self, _at: &Layout, _shell: Arc<Shell>) -> Result<Vec<Message>, Failure> {
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

/// Reached through `BASH_ENV`: the script is joined before its first line.
#[tokio::test]
async fn a_script_reports_as_it_goes_and_the_run_hands_back_the_series() {
    let scripts = Scripts::of(&[(
        "collect.bash",
        r#"
            declare -a found=()

            note() {
                found+=("$1")
                STEP name "$1" seen "${#found[@]}"
            }

            note alpha
            note "beta with spaces"

            # The accumulated array goes back in one message; word boundaries
            # survive because a message is a bash array, not a joined string.
            TOTAL "${found[@]}"
            "#,
    )]);

    let ran = Keeping
        .run(
            &bash(scripts.at("collect.bash")),
            |at| {
                Ok(vec![at.bash_env(
                    Provision::Joining(&Keeping.joining(at)),
                )?])
            },
        )
        .await
        .unwrap()
        .whole()
        .unwrap();

    assert_eq!(ran.subject, ExitStatus::Code(0));
    assert_eq!(
        ran.shells.len(),
        1,
        "provenance is the shape: one shell produced all of it"
    );

    // A run folds per shell, and `heard` puts those foldings back in the order
    // they were said. Each message comes with the shell that sent it.
    let said = heard(&ran.shells);

    let steps: Vec<Step> = said
        .iter()
        .filter_map(|said| step(said.message))
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        steps,
        [
            Step {
                name: "alpha".into(),
                seen: 1
            },
            Step {
                name: "beta with spaces".into(),
                seen: 2
            },
        ]
    );

    let total: Vec<&[String]> = said
        .iter()
        .filter_map(|said| said.message.behind("TOTAL"))
        .collect();
    assert_eq!(
        total,
        [["alpha", "beta with spaces"]],
        "a message nobody wrote a decoder for is still there, as raw words"
    );
}

/// Reached by hand: the environment carries the workspace and nothing
/// else — not even definitions — and the script loads the pieces and
/// initiates where it says so. A shell it started that never joined is not
/// a shell of the run.
#[tokio::test]
async fn a_script_joins_where_it_chooses_and_is_heard_from_there() {
    let scripts = Scripts::of(&[(
        "collect.bash",
        r#"
            declare -- workspace="${LISTENING_SESSION:?the workspace, from the run closure}"

            bash -c 'exit 0'                        # a shell of the subject's, not of the run's

            source "$workspace/prelude.bash"
            source "$workspace/rig.bash"
            BC_JOIN KEEP "$workspace"
            STEP name joined seen 1
            "#,
    )]);

    let ran = Keeping
        .run(
            &bash(scripts.at("collect.bash")),
            |at| {
                Ok(vec![crate::support::listening_session(
                    at,
                )])
            },
        )
        .await
        .unwrap()
        .whole()
        .unwrap();

    assert_eq!(ran.subject, ExitStatus::Code(0));
    assert_eq!(
        ran.shells.len(),
        1,
        "the one that sourced the address"
    );

    let steps: Vec<Step> = heard(&ran.shells)
        .iter()
        .filter_map(|said| step(said.message))
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        steps,
        [Step {
            name: "joined".into(),
            seen: 1
        }]
    );
}

impl Keeping {
    fn joining(&self, at: &Layout) -> String {
        format!(
            "BC_JOIN KEEP {}\n",
            bash_strings::emit_scalar(at.text())
        )
    }
}
