//! Timing a tree of calls, in continuation-passing style.
//!
//! `BASHPROF_TIME_CPS <label> <command…>` wraps the call it is given, so a
//! measurement nests wherever the calls do.
//!
//! Two facts do the work. Spans nest strictly within one shell, because `"$@"`
//! either returns to its caller or takes the shell down — so pushing on BEGIN
//! and popping on END *is* the tree, with no identifier on the wire. And a
//! frame walk comes with each BEGIN, which says where in the parent the call
//! was made, across however many unmeasured lines lie between.
//!
//! A [`Span`] is a measurement that completed: no field of it is conditional,
//! because one that had not completed would not be a `Span`. What was still
//! open when the run ended is [`Unfinished`], which carries the [`Profile`]
//! anyway — so a caller that can work with what resolved does, and one that
//! cannot returns the error.

use std::collections::HashMap;
use std::fmt;

use mb_resolver::bash::rig::{field, run, ExitStatus, Failure, Line, Micros, Pid, Rig, Startup};
use mb_resolver::bash::stack::{Columns, Frame};
use mb_resolver::bash::STACK;

use crate::support::{bash, Scripts};

const BASHPROF_BASH: &str = include_str!("bash/bashprof.bash");

// ── what a run produced ──────────────────────────────────────────────

/// One measured call, and the ones made inside it.
#[derive(Debug)]
struct Span {
    label: String,
    pid: Pid,
    began: Micros,
    ended: Micros,

    /// Where the call was made.
    at: Frame,

    /// The frames above that one, outermost last.
    outer: Vec<Frame>,

    children: Vec<Span>,
}

impl Span {
    /// BEGIN to END: this span's own work and everything inside it.
    fn inclusive(&self) -> u64 {
        self.ended.0 - self.began.0
    }

    /// What was spent here rather than in a measured child.
    fn exclusive(&self) -> u64 {
        self.inclusive() - self.children.iter().map(Span::inclusive).sum::<u64>()
    }

    fn child(&self, label: &str) -> Option<&Span> {
        self.children.iter().find(|span| span.label == label)
    }

    /// This span and everything under it, outermost first.
    fn walk<'a>(&'a self, each: &mut impl FnMut(&'a Span)) {
        each(self);
        for child in &self.children {
            child.walk(each);
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:>6} µs ({:>6} µs of its own)  at {}",
            self.label,
            self.inclusive(),
            self.exclusive(),
            self.at
        )
    }
}

/// The measurements a run produced: outermost spans, in the order they began.
#[derive(Debug)]
struct Profile {
    roots: Vec<Span>,
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn outline(span: &Span, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            writeln!(f, "{:indent$}{span}", "", indent = depth * 2)?;
            span.children.iter().try_for_each(|child| outline(child, depth + 1, f))
        }

        self.roots.iter().try_for_each(|root| outline(root, 0, f))
    }
}

/// Spans whose shell never reached their END: it died inside the call.
///
/// The measurements that did complete travel with this, since they are no less
/// true for the run having ended badly. Whether that is fatal is the caller's:
/// a test treats it as a failure, a tool reporting what it has might not.
#[derive(Debug)]
struct Unfinished {
    /// Per shell that left one, the labels innermost first.
    left_open: Vec<(Pid, Vec<String>)>,

    resolved: Profile,
}

impl fmt::Display for Unfinished {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "spans left open: ")?;
        for (pid, labels) in &self.left_open {
            write!(f, "pid {pid} {labels:?} ")?;
        }
        write!(f, "({} measurements resolved)", self.resolved.roots.len())
    }
}

impl std::error::Error for Unfinished {}

// ── the session ──────────────────────────────────────────────────────

/// A span that has begun. Its completed children accumulate here until its own
/// END arrives, which is the only thing that can make it a [`Span`].
struct Opening {
    label: String,
    pid: Pid,
    began: Micros,
    at: Frame,
    outer: Vec<Frame>,
    children: Vec<Span>,
}

impl Opening {
    fn close(self, ended: Micros) -> Span {
        Span {
            label: self.label,
            pid: self.pid,
            began: self.began,
            ended,
            at: self.at,
            outer: self.outer,
            children: self.children,
        }
    }
}

/// Spans still open per shell, innermost last, and the roots that have closed.
#[derive(Default)]
struct Timing {
    open: HashMap<Pid, Vec<Opening>>,
    roots: Vec<Span>,
}

impl Timing {
    fn begin(&mut self, span: Opening) {
        self.open.entry(span.pid).or_default().push(span);
    }

    /// An unbalanced END is a defect in the instrument, not a shape to carry,
    /// so it ends the run rather than reaching a caller.
    fn close(&mut self, pid: Pid, ended: Micros) -> Result<(), Failure> {
        let unbalanced =
            || Failure::new("closing a span", format!("an END from pid {pid} with no BEGIN"));

        let stack = self.open.get_mut(&pid).ok_or_else(unbalanced)?;
        let span = stack.pop().ok_or_else(unbalanced)?.close(ended);

        match stack.last_mut() {
            Some(parent) => parent.children.push(span),
            None => self.roots.push(span),
        }
        Ok(())
    }

    /// Everything that completed, and whether anything did not.
    ///
    /// A span that completed inside one that did not becomes a root: the
    /// measurement is a fact, the enclosing one is not.
    fn finish(self) -> Result<Profile, Unfinished> {
        let Timing { open, mut roots } = self;
        let mut left_open = Vec::new();

        for (pid, opening) in open {
            if opening.is_empty() {
                continue;
            }

            let mut labels = Vec::new();
            for span in opening {
                labels.push(span.label);
                roots.extend(span.children);
            }
            labels.reverse();
            left_open.push((pid, labels));
        }

        roots.sort_by_key(|span| span.began);
        left_open.sort_by_key(|(pid, _)| pid.0);

        let resolved = Profile { roots };
        match left_open.is_empty() {
            true => Ok(resolved),
            false => Err(Unfinished { left_open, resolved }),
        }
    }
}

struct Profiling;

impl Rig for Profiling {
    type Session = Timing;

    fn startup(&self) -> Startup {
        Startup { bash: format!("{STACK}\n{BASHPROF_BASH}"), ..Default::default() }
    }

    fn open(&self) -> Result<Timing, Failure> {
        Ok(Timing::default())
    }

    fn hear(&self, timing: &mut Timing, said: Line) -> Result<(), Failure> {
        let Some(payload) = said.behind("TIME_CPS") else { return Ok(()) };
        let reading = |what: &str| Failure::new("reading a span", what.to_string());

        let Some((kind, rest)) = payload.split_first() else {
            return Err(reading("an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => {
                let label = field(rest, "label").ok_or_else(|| reading("no label"))?.to_string();

                // A call has a site. Establishing that here once is what lets
                // every reader of a `Span` have a `Frame` rather than a maybe.
                let mut frames = Columns::of(rest)?.frames()?.into_iter();
                let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

                timing.begin(Opening {
                    label,
                    pid: said.pid,
                    began: said.sent_at,
                    at,
                    outer: frames.collect(),
                    children: Vec::new(),
                });
                Ok(())
            }
            "END" => timing.close(said.pid, said.sent_at),
            other => Err(reading(&format!("unknown kind {other:?}"))),
        }
    }
}

// ── the subject ──────────────────────────────────────────────────────

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

/// A µs budget for scheduling and for the `sleep` each pause forks. Wide,
/// because the bound it guards only has to separate `a`'s own two pauses from
/// the whole tree's time — an order of magnitude apart.
const SLACK: u64 = 60_000;

/// Follow labels down from a root. The tree's shape is what is under test, so
/// a path that does not exist is a failed assertion rather than a `None`.
fn at<'a>(root: &'a Span, path: &[&str]) -> &'a Span {
    path.iter().fold(root, |span, label| {
        span.child(label).unwrap_or_else(|| panic!("no {label:?} under {:?}", span.label))
    })
}

#[test]
fn measurements_nest_the_way_the_calls_do() {
    let scripts = Scripts::of(&[("tree.bash", TREE)]);
    let (timing, status) =
        run(&Profiling, &bash(scripts.at("tree.bash"))).unwrap().whole().unwrap();

    assert_eq!(status, ExitStatus::Code(0));

    // Nothing was left open, so there is a profile and not an error — and
    // from here on no measurement is conditional.
    let profile = timing.finish().expect("every span that opened also closed");
    println!("{profile}");

    // The tree fell out of the nesting; nothing on the wire identified a pair.
    assert_eq!(profile.roots.len(), 1, "one outermost measurement");
    let a = &profile.roots[0];
    assert_eq!(a.label, "a");

    let labels = |span: &Span| span.children.iter().map(|c| c.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(a), ["b", "e"]);
    assert_eq!(labels(at(a, &["b"])), ["c", "d"]);
    assert_eq!(labels(at(a, &["e"])), ["f"]);
    assert!(labels(at(a, &["b", "c"])).is_empty());

    let mut spans = Vec::new();
    a.walk(&mut |span| spans.push(span));
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
    let scripts = Scripts::of(&[("tree.bash", TREE)]);
    let (timing, _) = run(&Profiling, &bash(scripts.at("tree.bash"))).unwrap().whole().unwrap();
    let profile = timing.finish().expect("a complete profile");
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
/// would have without the instrument. The span that was open when it died
/// never becomes a measurement — and the ones that completed still are.
#[test]
fn a_span_the_shell_died_inside_is_an_error_carrying_the_rest() {
    let scripts = Scripts::of(&[(
        "dies.bash",
        r#"
        set -e

        f__ok()   { :; }
        f__dies() { false; echo "RAN PAST ITS OWN FAILURE"; }

        BASHPROF_TIME_CPS ok f__ok
        BASHPROF_TIME_CPS doomed f__dies
        echo "REACHED THE END"
        "#,
    )]);

    let (timing, status) =
        run(&Profiling, &bash(scripts.at("dies.bash"))).unwrap().whole().unwrap();

    assert_eq!(status, ExitStatus::Code(1), "the subject's own status, not the wrapper's");

    let unfinished = timing.finish().expect_err("the shell died inside a span");
    assert_eq!(unfinished.left_open.len(), 1, "one shell left something open");
    assert_eq!(unfinished.left_open[0].1, ["doomed"]);

    // The measurement that completed is no less true for the run ending badly.
    let resolved = &unfinished.resolved;
    assert_eq!(resolved.roots.len(), 1);
    assert_eq!(resolved.roots[0].label, "ok");
    assert!(unfinished.to_string().contains("doomed"), "{unfinished}");
}

/// A span that completed inside one that did not is still a measurement, so it
/// resolves as a root: what encloses it is not a fact.
#[test]
fn a_completed_span_survives_an_enclosing_one_that_did_not() {
    let scripts = Scripts::of(&[(
        "nested.bash",
        r#"
        set -e

        f__inner() { :; }
        f__outer() { BASHPROF_TIME_CPS inner f__inner; false; }

        BASHPROF_TIME_CPS outer f__outer
        "#,
    )]);

    let (timing, status) =
        run(&Profiling, &bash(scripts.at("nested.bash"))).unwrap().whole().unwrap();

    assert_eq!(status, ExitStatus::Code(1));

    let unfinished = timing.finish().expect_err("the outer span never closed");
    assert_eq!(unfinished.left_open[0].1, ["outer"]);
    assert_eq!(
        unfinished.resolved.roots.iter().map(|span| span.label.as_str()).collect::<Vec<_>>(),
        ["inner"],
        "the inner measurement is a fact; the one around it is not"
    );
}
