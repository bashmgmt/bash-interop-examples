//! Timing a tree of calls, in continuation-passing style.
//!
//! `BASHPROF_TIME_CPS <label> <command…>` wraps the call it is given, so a
//! measurement nests wherever the calls do. Nothing is timed in bash: the wire
//! already stamps every message with the sending shell's `$EPOCHREALTIME`, and
//! a span is the interval between two of them.
//!
//! Nor is anything inferred. Each call names itself and hands that name to
//! everything it runs — through the one frame bash gives it, which a fork
//! inherits — so every BEGIN says which call it was made inside of and the
//! tree travels on the wire.
//!
//! What a run yields is that tree as recorded: every call that began, whether
//! or not it ended. Reading it as timings is a separate step, and the
//! caller's — a test bails on a run that died mid-call, a tool reporting what
//! it has need not.
//!
//! | | |
//! |---|---|
//! | [`record`] | one call, how it went, and the call it was made inside of |
//! | [`recording`] | the wire read as flat records — one pass and a map |
//! | [`nesting`] | those records read as a tree — one hylic unfold |
//! | [`profile`] | that tree read as timings — one hylic fold |
//! | [`render`] | hylic's tree formatter, for either tree |

mod nesting;
mod profile;
mod record;
mod recording;
mod render;

use std::collections::HashSet;

use mb_resolver::bash::rig::{run, ExitStatus, Failure, Line, Rig, Startup};
use mb_resolver::bash::STACK;

use crate::support::{bash, Scripts};
use nesting::{nest, Recorded};
use profile::{Profile, Span};
use record::Call;
use recording::records;

const BASHPROF_BASH: &str = include_str!("bash/bashprof.bash");

/// The session is what the run heard. Which shell said it, and when, is on
/// every message already, so reading is a pass over them rather than a machine
/// kept up as they arrive.
struct Profiling;

impl Rig for Profiling {
    type Session = Vec<Line>;

    fn startup(&self) -> Startup {
        Startup { bash: format!("{STACK}\n{BASHPROF_BASH}"), ..Default::default() }
    }

    fn open(&self) -> Result<Vec<Line>, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, heard: &mut Vec<Line>, said: Line) -> Result<(), Failure> {
        heard.push(said);
        Ok(())
    }
}

/// Run a script under the profiler. What comes back is the tree as recorded —
/// every call that began, ended or not. Reading it as timings is the caller's,
/// which is what each test below does next.
fn profiled(script: &str) -> (Vec<Recorded>, ExitStatus) {
    let scripts = Scripts::of(&[("subject.bash", script)]);
    let (heard, status) =
        run(&Profiling, &bash(scripts.at("subject.bash"))).unwrap().whole().unwrap();

    (records(&heard).map(nest).expect("the instrument's own messages"), status)
}

/// The labels of the calls that never ended.
fn unended(forest: &[Recorded]) -> Vec<&str> {
    Recorded::unended(forest).iter().map(|call| call.label.as_str()).collect()
}

/// Every call in a recorded forest, outermost first.
fn calls(forest: &[Recorded]) -> Vec<&Call> {
    forest
        .iter()
        .flat_map(|node| std::iter::once(node.call()).chain(calls(&node.children)))
        .collect()
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
    println!("as recorded:\n{}\n", Recorded::render(&recorded));

    let profile = Profile::of(&recorded).expect("every call that began also ended");
    println!("as timings:\n{profile}");

    assert_eq!(profile.roots.len(), 1, "one outermost measurement");
    let a = &profile.roots[0];
    assert_eq!(a.label, "a");

    let labels = |span: &Span| span.children.iter().map(|c| c.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(a), ["b", "e"]);
    assert_eq!(labels(at(a, &["b"])), ["c", "d"]);
    assert_eq!(labels(at(a, &["e"])), ["f"]);
    assert!(labels(at(a, &["b", "c"])).is_empty());

    assert_eq!(a.all().len(), 6);
    assert!(a.all().iter().all(|span| span.pid == a.pid), "one shell produced all of it");
}

#[test]
fn a_spans_time_covers_its_own_work_and_everything_it_called() {
    let recorded = profiled(TREE).0;
    let profile = Profile::of(&recorded).expect("a complete profile");
    let a = &profile.roots[0];

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

    assert!(
        (40_000..40_000 + SLACK).contains(&a.exclusive()),
        "a's own time is the two 20 ms pauses in f__A, got {} µs\n{profile}",
        a.exclusive()
    );

    let leaf = at(a, &["e", "f"]);
    assert_eq!(leaf.exclusive(), leaf.inclusive(), "nothing is measured inside f");

    let total: u64 = a.all().iter().map(|span| span.exclusive()).sum();
    assert_eq!(total, a.inclusive(), "exclusive times partition the root's\n{profile}");
}

/// A subshell forks the process but not the stack, so a call measured inside
/// one belongs to the call it was made from. The shell it ran in cannot say
/// that — its messages arrive in a lane of their own — and the stack can.
#[test]
fn a_call_measured_in_a_subshell_nests_where_it_was_made() {
    let (recorded, status) = profiled(
        r#"
        f__A() {
            BASHPROF_TIME_CPS plain true
            ( BASHPROF_TIME_CPS forked true )
        }
        BASHPROF_TIME_CPS a f__A
        "#,
    );

    assert_eq!(status, ExitStatus::Code(0));
    let profile = Profile::of(&recorded).expect("every call ended");

    assert_eq!(profile.roots.len(), 1, "one outermost measurement, not one per shell");
    let a = &profile.roots[0];

    let labels = a.children.iter().map(|span| span.label.as_str()).collect::<Vec<_>>();
    assert_eq!(labels, ["plain", "forked"]);
    assert_ne!(at(a, &["forked"]).pid, a.pid, "and it did run in a shell of its own");
}

/// Two forks of one line report identical frames and their windows overlap,
/// and neither costs anything: a fork's calls are its own shell's stack, so
/// what was made inside each `turn` was settled where it happened. What the
/// overlap does reach is the arithmetic — together they last longer than the
/// call that made them.
#[test]
fn concurrent_forks_of_one_line_keep_their_own_calls() {
    let (recorded, status) = profiled(
        r#"
        f__work() {
            sleep "$1"
            BASHPROF_TIME_CPS inner true
            sleep 0.1
        }

        f__A() {
            for delay in 0.05 0.01; do
                ( BASHPROF_TIME_CPS turn f__work "$delay" ) &
            done
            wait
        }

        BASHPROF_TIME_CPS a f__A
        "#,
    );

    assert_eq!(status, ExitStatus::Code(0));
    println!("as recorded:\n{}\n", Recorded::render(&recorded));

    let profile = Profile::of(&recorded).expect("every call ended");
    let a = &profile.roots[0];
    assert_eq!(a.children.len(), 2, "one measurement per fork\n{profile}");

    assert_ne!(a.children[0].id, a.children[1].id, "one line, two calls, two names");

    for turn in &a.children {
        assert_eq!(turn.label, "turn");
        assert_eq!(turn.children.len(), 1, "the call made in its own shell\n{profile}");
        assert_eq!(turn.children[0].pid, turn.pid, "and no other's\n{profile}");
    }

    let together: u64 = a.children.iter().map(Span::inclusive).sum();
    assert!(together > a.inclusive(), "the two ran at once, so their windows overlap\n{profile}");
    assert!(a.exclusive() < a.inclusive(), "and what neither covered is a's own\n{profile}");
}

/// A fork inherits the name in scope where it was made, and so does a fork of
/// that fork. `deep` belongs to `a` although two process boundaries and a
/// finished call of the middle shell's lie between them.
#[test]
fn a_name_is_inherited_through_two_levels_of_forking() {
    let (recorded, status) = profiled(
        r#"
        f__A() {
            (
                BASHPROF_TIME_CPS middle true
                ( BASHPROF_TIME_CPS deep true )
            )
        }

        BASHPROF_TIME_CPS a f__A
        "#,
    );

    assert_eq!(status, ExitStatus::Code(0));
    let profile = Profile::of(&recorded).expect("every call ended");

    assert_eq!(profile.roots.len(), 1, "one outermost measurement\n{profile}");
    let a = &profile.roots[0];

    let labels = a.children.iter().map(|span| span.label.as_str()).collect::<Vec<_>>();
    assert_eq!(labels, ["middle", "deep"], "both under a, neither under the other\n{profile}");

    let pids = [a.pid, at(a, &["middle"]).pid, at(a, &["deep"]).pid];
    assert_eq!(
        pids.iter().collect::<HashSet<_>>().len(),
        3,
        "three shells, so the name really crossed two forks\n{profile}"
    );
}

/// Two calls of one line differ in nothing a reader can see except the name
/// their shell gave them — and that name is what the tree was built from.
#[test]
fn every_measurement_has_a_name_of_its_own() {
    let recorded = profiled(TREE).0;
    let profile = Profile::of(&recorded).expect("a complete profile");
    let a = &profile.roots[0];

    let names: HashSet<&str> = a.all().iter().map(|span| span.id.0.as_str()).collect();
    assert_eq!(names.len(), 6, "six calls, six names\n{profile}");
}

/// The layers are aliases, so the instrument is one frame and the walk points
/// at the subject's own call site. What is above it is the subject's stack
/// with one frame of ours per enclosing measurement — where that measurement
/// is executing, and nothing else.
#[test]
fn a_call_carries_the_whole_stack_it_was_made_on() {
    let recorded = profiled(TREE).0;
    let c = calls(&recorded).into_iter().find(|call| call.label == "c").expect("c was measured");

    assert_eq!(c.at.funcname, "f__B", "where the call was made");
    assert_eq!(
        c.outer.iter().map(|frame| frame.funcname.as_str()).collect::<Vec<_>>(),
        ["BASHPROF_TIME_CPS", "f__A", "BASHPROF_TIME_CPS", "main"],
        "and everything above it"
    );
}

/// A caller that wrapped the public word in a word of its own says how far
/// past itself the walk should reach. Without the shift, `leaf` would be
/// recorded as made in `f__measured`, which is nobody's call site.
#[test]
fn a_wrapper_can_move_the_walk_past_itself() {
    let recorded = profiled(
        r#"
        f__leaf() { :; }

        f__measured() {
            local __BASHPROF_STACK_SHIFT=1
            BASHPROF_TIME_CPS "$@"
        }

        f__A() { f__measured leaf f__leaf; }

        BASHPROF_TIME_CPS a f__A
        "#,
    )
    .0;

    let profile = Profile::of(&recorded).expect("every call ended");
    let a = &profile.roots[0];

    assert_eq!(at(a, &["leaf"]).at.funcname, "f__A", "the subject's site, not the wrapper's");
    assert_eq!(a.at.funcname, "main", "and the unwrapped call is unaffected\n{profile}");
}

/// Where each call was made, which the nesting alone does not say: two calls
/// from one function are told apart by their line.
#[test]
fn a_span_says_where_its_call_was_made() {
    let recorded = profiled(TREE).0;
    let profile = Profile::of(&recorded).expect("a complete profile");
    let a = &profile.roots[0];

    assert_eq!(a.at.funcname, "main", "the outermost call is in the script's own body");
    assert_eq!(at(a, &["b"]).at.funcname, "f__A");
    assert_eq!(at(a, &["e"]).at.funcname, "f__A");
    assert_eq!(at(a, &["b", "c"]).at.funcname, "f__B");
    assert_eq!(at(a, &["b", "d"]).at.funcname, "f__B");
    assert_eq!(at(a, &["e", "f"]).at.funcname, "f__E");

    assert_ne!(at(a, &["b", "c"]).at.lineno, at(a, &["b", "d"]).at.lineno);
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
    let resolved = &unfinished.resolved;

    assert_eq!(resolved.roots.len(), 1);
    assert_eq!(resolved.roots[0].label, "ok", "no less true for the run ending badly");
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
        ["inner"]
    );
}

/// The error's message is the tree as recorded, so what completed and what did
/// not are shown together and in place.
#[test]
fn the_error_renders_the_tree_it_was_recorded_as() {
    let recorded = profiled(NESTED).0;
    let shown = Profile::of(&recorded).expect_err("the outer call never ended").to_string();
    println!("{shown}");

    assert!(shown.contains("outer NEVER ENDED at main@"), "{shown}");
    assert!(shown.contains("µs at f__outer@"), "the completed one, with its duration: {shown}");
    assert!(!shown.contains("inner NEVER"), "{shown}");
}
