//! Adding a tool: bash for the subject, one `FromRecord` for the Rust side.
//!
//! A CPS timer, the shape a profiler takes. No code generation: a tool's bash
//! is bash, and it speaks through `BC_INSTR say`.

use mb_resolver::bash::rig::{
    field, listen, BashSrc, ExitStatus, FromRecord, Record, Setup,
};

use crate::{decoded, written};

const TAG: &str = "TIMEIT";

/// `TIMEIT <command> [args…]`: run the continuation, then report how long it
/// took and how it ended.
///
/// The two things easy to get wrong in a `continuation == "$@"` framework are
/// both visible in five lines: `$?` is read as the *first* statement after the
/// continuation, and it is what the wrapper returns.
const TIMEIT_BASH: &str = r#"
TIMEIT() {
    local started=${EPOCHREALTIME/./}
    "$@"
    local rc=$?
    BC_INSTR say TIMEIT \
        elapsed_us "$(( ${EPOCHREALTIME/./} - started ))" \
        status "$rc" command "$1"
    return "$rc"
}
"#;

#[derive(Debug, PartialEq, Eq)]
struct Timing {
    elapsed_us: u64,
    status: i32,
    command: String,
}

impl FromRecord for Timing {
    type Err = String;

    fn from_record(record: &Record) -> Option<Result<Self, Self::Err>> {
        Some(Self::decode(record.behind(TAG)?))
    }
}

impl Timing {
    fn decode(words: &[String]) -> Result<Self, String> {
        let at =
            |key: &str| field(words, key).ok_or_else(|| format!("{TAG} record is missing {key:?}"));

        Ok(Self {
            elapsed_us: at("elapsed_us")?.parse().map_err(|_| "elapsed_us")?,
            status: at("status")?.parse().map_err(|_| "status")?,
            command: at("command")?.to_string(),
        })
    }
}

#[test]
fn a_cps_wrapper_times_its_continuation_and_preserves_status() {
    let (seen, status) = listen(
        Setup::new().bash(BashSrc::raw(TIMEIT_BASH)),
        &[written(&[(
            "work.bash",
            r#"
            slow() { sleep 0.05; }
            fails() { return 3; }

            TIMEIT slow
            TIMEIT fails
            TIMEIT echo hello
            exit 0
            "#,
        )])],
    )
    .unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    let timings = decoded::<Timing>(&seen);

    assert_eq!(timings.len(), 3);
    assert_eq!(timings[0].command, "slow");
    assert!(timings[0].elapsed_us >= 50_000, "{:?}", timings[0]);
    assert_eq!(timings[1].status, 3, "the continuation's status, not the wrapper's");
    assert_eq!(timings[2].command, "echo");
}

/// A record that will not decode surfaces as an error rather than vanishing:
/// `of` reports every attempt, `decoded` keeps only what succeeded.
#[test]
fn decode_failures_are_visible() {
    let (seen, _) = listen(
        Setup::new().bash(BashSrc::raw(TIMEIT_BASH)),
        &[written(&[("main.bash", "BC_INSTR say TIMEIT status 0\n")])],
    )
    .unwrap();

    let outcomes: Vec<Result<Timing, String>> =
        seen.of::<Timing>().map(|entry| entry.value).collect();

    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].as_ref().unwrap_err().contains("elapsed_us"));
    assert_eq!(decoded::<Timing>(&seen).len(), 0);
}
