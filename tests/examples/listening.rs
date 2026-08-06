//! A session that keeps what a script says, and a decoder that reads it back.
//!
//! The rig contributes no bash: `STEP` is a word this script chose, and
//! `behind` is how a decoder claims it.

use mb_resolver::bash::rig::{field, run, shells, ExitStatus, Failure, Line, Rig};

use crate::support::Scripts;

/// The session: everything the subject said, in the order it arrived.
struct Keeping;

impl Rig for Keeping {
    type Session = Vec<Line>;

    fn open(&self) -> Result<Vec<Line>, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, heard: &mut Vec<Line>, said: Line) -> Result<(), Failure> {
        heard.push(said);

        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Step {
    name: String,
    seen: usize,
}

/// Recognise, then decode. `None` means some other tool's message, `Some(Err)`
/// means ours and malformed — which is what lets several tools share one wire
/// while a decode failure stays visible.
fn step(line: &Line) -> Option<Result<Step, String>> {
    let words = line.behind("STEP")?;
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

    let (heard, status) = run(&Keeping, &[scripts.at("collect.bash")]).unwrap();

    assert_eq!(status, ExitStatus::Code(0));

    let steps: Vec<Step> = heard.iter().filter_map(step).map(Result::unwrap).collect();
    assert_eq!(
        steps,
        [
            Step { name: "alpha".into(), seen: 1 },
            Step { name: "beta with spaces".into(), seen: 2 },
        ]
    );

    // A message nobody wrote a decoder for is still there, as raw words.
    let total: Vec<&[String]> = heard.iter().filter_map(|line| line.behind("TOTAL")).collect();
    assert_eq!(total, [["alpha", "beta with spaces"]]);

    // Provenance rides along: one shell produced all of it.
    assert_eq!(shells(&heard).unwrap().len(), 1);
}
