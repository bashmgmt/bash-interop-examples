//! Timing a tree of calls, in continuation-passing style.
//!
//! `BASHPROF_TIME_CPS <label> <command…>` wraps the call it is given, so a
//! measurement nests wherever the calls do. Nothing is timed in bash: the wire
//! already stamps every message with the sending shell's `$EPOCHREALTIME`, and
//! a span is the interval between two of them.
//!
//! What a run yields is the tree as recorded: every call that began, whether
//! or not it ended. Reading that as timings is a separate step, and the
//! caller's — a test bails on a run that died mid-call, a tool reporting what
//! it has need not.
//!
//! | | |
//! |---|---|
//! | [`recorded`] | the form a run yields, and how it shows itself |
//! | [`recording`] | building it from BEGIN and END |
//! | [`profile`] | reading it as timings — one hylic fold |
//! | [`render`] | hylic's tree formatter, for either tree |

mod profile;
mod recorded;
mod recording;
mod render;

use mb_resolver::bash::rig::{run, ExitStatus, Failure, Line, Rig, Startup};
use mb_resolver::bash::STACK;

use crate::support::{bash, Scripts};
use profile::{Profile, Span};
use recorded::Recorded;
use recording::Recording;

const BASHPROF_BASH: &str = include_str!("bash/bashprof.bash");

struct Profiling;

impl Rig for Profiling {
    type Session = Recording;

    fn startup(&self) -> Startup {
        Startup { bash: format!("{STACK}\n{BASHPROF_BASH}"), ..Default::default() }
    }

    fn open(&self) -> Result<Recording, Failure> {
        Ok(Recording::default())
    }

    fn hear(&self, recording: &mut Recording, said: Line) -> Result<(), Failure> {
        recording.hear(&said)
    }
}

/// Run a script under the profiler. What comes back is the tree as recorded —
/// every call that began, ended or not. Reading it as timings is the caller's,
/// which is what each test below does next.
fn profiled(script: &str) -> (Vec<Recorded>, ExitStatus) {
    let scripts = Scripts::of(&[("subject.bash", script)]);
    let (recording, status) =
        run(&Profiling, &bash(scripts.at("subject.bash"))).unwrap().whole().unwrap();

    (recording.recorded(), status)
}

/// The labels of the calls that never ended.
fn unended(forest: &[Recorded]) -> Vec<&str> {
    Recorded::unended(forest).iter().map(|call| call.label.as_str()).collect()
}

/// Follow labels down from a root. The tree's shape is what is under test, so
/// a path that does not exist is a failed assertion rather than a `None`.
fn at<'a>(root: &'a Span, path: &[&str]) -> &'a Span {
    path.iter().fold(root, |span, label| {
        span.child(label).unwrap_or_else(|| panic!("no {label:?} under {:?}", span.label))
    })
}

/// A → {B → {C, D}, E → F}, with unmeasured work between the measured calls
/// so that a span's own time is not just its children's.
const TREE: &str = r#"
    pause() { sleep "$1"; }

    f__A() {
        pause 0.02
        BASHPROF_TIME_CPS b f__B
        pause 0.02
        BASHPROF_TIME_CPS e f__E
    }

    f__B() {
        BASHPROF_TIME_CPS c f__C
        pause 0.01
        BASHPROF_TIME_CPS d f__D
    }

    f__C() { pause 0.03; }
    f__D() { pause 0.04; }

    f__E() {
        pause 0.01
        BASHPROF_TIME_CPS f f__F
    }

    f__F() { pause 0.05; }

    BASHPROF_TIME_CPS a f__A
    "#;

/// A call that completes inside one the shell then dies in.
const NESTED: &str = r#"
    set -e

    f__inner() { :; }
    f__outer() { BASHPROF_TIME_CPS inner f__inner; false; }

    BASHPROF_TIME_CPS outer f__outer
    "#;

/// A µs budget for scheduling and for the `sleep` each pause forks. Wide,
/// because the bound it guards only has to separate `a`'s own two pauses from
/// the whole tree's time — an order of magnitude apart.
const SLACK: u64 = 60_000;

#[test]
fn measurements_nest_the_way_the_calls_do() {
    let (recorded, status) = profiled(TREE);
    assert_eq!(status, ExitStatus::Code(0));

    // Every call ended, so reading the forest yields a profile and not an
    // error — and from here on no measurement is conditional.
    println!("as recorded:\n{}\n", Recorded::render(&recorded));

    let profile = Profile::of(&recorded).expect("every call that began also ended");
    println!("as timings:\n{profile}");

    // The tree fell out of the nesting; nothing on the wire identified a pair.
    assert_eq!(profile.roots.len(), 1, "one outermost measurement");
    let a = &profile.roots[0];
    assert_eq!(a.label, "a");

    let labels = |span: &Span| span.children.iter().map(|c| c.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(a), ["b", "e"]);
    assert_eq!(labels(at(a, &["b"])), ["c", "d"]);
    assert_eq!(labels(at(a, &["e"])), ["f"]);
    assert!(labels(at(a, &["b", "c"])).is_empty());

    let spans = a.all();
    assert_eq!(spans.len(), 6);
    assert!(spans.iter().all(|span| span.pid == a.pid), "one shell produced all of it");

    // Each span covers at least what it slept for, its children included.
    for (path, slept) in [
        (&["b", "c"][..], 30_000),
        (&["b", "d"][..], 40_000),
        (&["e", "f"][..], 50_000),
        (&["b"][..], 30_000 + 10_000 + 40_000),
        (&["e"][..], 10_000 + 50_000),
        (&[][..], 20_000 + 80_000 + 20_000 + 60_000),
    ] {
        let span = at(a, path);
        assert!(
            span.inclusive() >= slept,
            "{} took {} µs, less than the {slept} µs it slept\n{profile}",
            span.label,
            span.inclusive()
        );
    }

    // A span's own time is what it did outside its measured children — here,
    // the two unmeasured pauses in `f__A`.
    assert!(
        (40_000..40_000 + SLACK).contains(&a.exclusive()),
        "a's own time is its two 20 ms pauses, got {} µs\n{profile}",
        a.exclusive()
    );

    // A leaf spends everything on itself.
    let leaf = at(a, &["e", "f"]);
    assert_eq!(leaf.exclusive(), leaf.inclusive(), "nothing measured inside f");

    // The whole tree accounts for itself: every µs belongs to exactly one span.
    let total: u64 = spans.iter().map(|span| span.exclusive()).sum();
    assert_eq!(total, a.inclusive(), "exclusive times partition the root's\n{profile}");
}

/// The nesting says which spans contain which; the frame walk says *where* in
/// the parent each call was made. The two agree, and only the second can tell
/// two calls from one function apart.
#[test]
fn the_captured_stack_corroborates_the_tree() {
    let recorded = profiled(TREE).0;
    let profile = Profile::of(&recorded).expect("a complete profile");
    let a = &profile.roots[0];

    assert_eq!(a.at.funcname, "main", "the outermost call is in the script's own body");
    assert_eq!(at(a, &["b"]).at.funcname, "f__A");
    assert_eq!(at(a, &["e"]).at.funcname, "f__A");
    assert_eq!(at(a, &["b", "c"]).at.funcname, "f__B");
    assert_eq!(at(a, &["b", "d"]).at.funcname, "f__B");
    assert_eq!(at(a, &["e", "f"]).at.funcname, "f__E");

    assert_ne!(
        at(a, &["b", "c"]).at.lineno,
        at(a, &["b", "d"]).at.lineno,
        "two calls from one function, told apart by their line"
    );

    // Every parent's call site appears above every child's, which is the
    // nesting seen from the shell's own stack rather than from the pairing.
    fn agrees(span: &Span) {
        for child in &span.children {
            let above: Vec<&str> =
                child.outer.iter().map(|frame| frame.funcname.as_str()).collect();

            assert!(
                above.contains(&span.at.funcname.as_str()),
                "{:?} is inside {:?}, but {:?} is not above {:?} in {above:?}",
                child.label,
                span.label,
                span.at.funcname,
                child.at.funcname
            );
            agrees(child);
        }
    }
    agrees(a);
}

/// A measured call is run unguarded, so `set -e` ends the subject where it
/// would have without the instrument. The call that was open when it died
/// never becomes a measurement — and the ones that completed still are.
#[test]
fn a_call_the_shell_died_inside_is_an_error_carrying_the_rest() {
    let (recorded, status) = profiled(
        r#"
        set -e

        f__ok()   { :; }
        f__dies() { false; echo "RAN PAST ITS OWN FAILURE"; }

        BASHPROF_TIME_CPS ok f__ok
        BASHPROF_TIME_CPS doomed f__dies
        echo "REACHED THE END"
        "#,
    );

    assert_eq!(status, ExitStatus::Code(1), "the subject's own status, not the wrapper's");
    assert_eq!(unended(&recorded), ["doomed"], "the forest says so on its own");

    let unfinished = Profile::of(&recorded).expect_err("the shell died inside a call");

    // The measurement that completed is no less true for the run ending badly.
    let resolved = &unfinished.resolved;
    assert_eq!(resolved.roots.len(), 1);
    assert_eq!(resolved.roots[0].label, "ok");
}

/// The error's message is the tree as recorded, so what completed and what did
/// not are shown together and in place.
#[test]
fn the_error_renders_the_tree_it_was_recorded_as() {
    let recorded = profiled(NESTED).0;
    let unfinished = Profile::of(&recorded).expect_err("the outer call never ended");

    let shown = unfinished.to_string();
    println!("{shown}");

    // `outer` was called from the script body and never came back; `inner`
    // was called from inside it and did. Both are in the tree, in place.
    assert!(shown.contains("outer NEVER ENDED at main@"), "{shown}");
    assert!(shown.contains("inner "), "the completed call is shown too: {shown}");
    assert!(shown.contains("µs at f__outer@"), "with the duration it did have: {shown}");
    assert!(!shown.contains("inner NEVER"), "{shown}");
}

/// A call that completed inside one that did not is still a measurement, so it
/// resolves as a root: what encloses it is not a fact.
#[test]
fn a_completed_call_survives_an_enclosing_one_that_did_not() {
    let recorded = profiled(NESTED).0;
    assert_eq!(unended(&recorded), ["outer"]);

    let unfinished = Profile::of(&recorded).expect_err("the outer call never ended");
    assert_eq!(
        unfinished.resolved.roots.iter().map(|span| span.label.as_str()).collect::<Vec<_>>(),
        ["inner"],
        "the inner measurement is a fact; the one around it is not"
    );
}
