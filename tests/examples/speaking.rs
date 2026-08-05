//! Speaking: a script reports as it goes, and the run hands back the series.
//!
//! The rig contributes no bash here. `STEP` is a word this script chose.

use mb_resolver::bash::rig::{field, listen, ExitStatus, FromRecord, Record, Setup};

use crate::{args, decoded, report, written};

#[derive(Debug, PartialEq, Eq)]
struct Step {
    name: String,
    seen: usize,
}

impl FromRecord for Step {
    type Err = String;

    /// Recognise, then decode. `behind` is the one line that opts into the
    /// leading-word convention; `None` means some other tool's record.
    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        Some(Self::decode(record.behind("STEP")?))
    }
}

impl Step {
    fn decode(words: &[String]) -> Result<Self, String> {
        let at = |key: &str| field(words, key).ok_or_else(|| format!("no {key:?}"));

        Ok(Self {
            name: at("name")?.to_string(),
            seen: at("seen")?.parse().map_err(|_| "seen is not a number")?,
        })
    }
}

#[test]
fn a_script_accumulates_and_the_run_hands_back_the_whole_series() {
    let (seen, status) = listen(
        Setup::new(),
        &[written(&[(
            "collect.bash",
            r#"
            declare -a found=()

            note() {
                found+=("$1")
                BC_INSTR say STEP name "$1" seen "${#found[@]}"
            }

            note alpha
            note beta
            note "gamma with spaces"

            # The accumulated array goes back in one message; word boundaries
            # survive because a message is a bash array, not a joined string.
            BC_INSTR say TOTAL "${found[@]}"
            "#,
        )])],
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    assert_eq!(
        decoded::<Step>(&seen),
        [
            Step { name: "alpha".into(), seen: 1 },
            Step { name: "beta".into(), seen: 2 },
            Step { name: "gamma with spaces".into(), seen: 3 },
        ],
        "{}",
        report(&seen)
    );

    // A record nobody wrote a decoder for is still there, as raw words.
    assert_eq!(args(&seen, "TOTAL"), ["alpha beta gamma with spaces"]);

    // Provenance rides along: one shell produced all of it.
    assert_eq!(seen.shells().len(), 1);
}
