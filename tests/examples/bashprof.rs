//! Timing a tree of calls, in continuation-passing style.
//!
//! `BASHPROF_TIME_CPS <label> <command…>` wraps the call it is given, so a
//! measurement nests wherever the calls do. Nothing is timed in bash: the wire
//! already stamps every message with the sending shell's `$EPOCHREALTIME`, and
//! a span is the interval between two of them.
//!
//! Two facts do the work. Spans nest strictly within one shell, because `"$@"`
//! either returns to its caller or takes the shell down — so pushing on BEGIN
//! and popping on END *is* the tree, with no identifier on the wire. And a
//! frame walk comes with each BEGIN, which says where in the parent the call
//! was made, across however many unmeasured lines lie between.

use std::collections::HashMap;

use mb_resolver::bash::rig::{field, run, ExitStatus, Failure, Line, Micros, Pid, Rig, Startup};
use mb_resolver::bash::stack::{Columns, Frame};
use mb_resolver::bash::STACK;

use crate::support::{bash, Scripts};

/// Two frames are the instrument's: `__bc_stack`'s own and this one.
///
/// The measured call is run unguarded. A `||` list would suppress `errexit`
/// for everything it reaches, so a measured function would run past its own
/// first failure and the run's status would change — a profiler that alters
/// whether the subject aborts is not measuring the subject. Under `set -e` a
/// failure therefore exits at `"$@"` and no END is sent, which leaves the span
/// open: the shell died inside it, and that is the reading.
const BASHPROF_BASH: &str = r#"
BASHPROF_TIME_CPS() {
    local __BP_label="${1-}"
    shift || __BC_THROW

    local -a __BP_begin=(BEGIN label "$__BP_label")
    __bc_stack __BP_begin 2

    BC_INSTR say TIME_CPS "${__BP_begin[@]}" || __BC_BAIL

    "$@"
    local __BP_rc=$?

    BC_INSTR say TIME_CPS END || __BC_BAIL

    return "$__BP_rc"
}
"#;

/// One measured call, and the ones made inside it.
#[derive(Debug)]
struct Span {
    label: String,
    began: Micros,
    ended: Option<Micros>,

    /// Where the call was made, innermost first. `stack[0]` is the frame that
    /// called `BASHPROF_TIME_CPS`.
    stack: Vec<Frame>,

    children: Vec<Span>,
}

impl Span {
    /// Wall clock from BEGIN to END, this span's own work and everything
    /// inside it. `None` where the shell never reached the END.
    fn inclusive(&self) -> Option<u64> {
        Some(self.ended?.0 - self.began.0)
    }

    /// What was spent here rather than in a measured child.
    fn exclusive(&self) -> Option<u64> {
        let inside: u64 = self.children.iter().filter_map(Span::inclusive).sum();

        Some(self.inclusive()? - inside)
    }

    fn child(&self, label: &str) -> &Span {
        self.children
            .iter()
            .find(|span| span.label == label)
            .unwrap_or_else(|| panic!("{:?} has no child {label:?}", self.label))
    }

    /// The labels of this span and everything under it, as an indented tree.
    fn outline(&self, depth: usize, into: &mut String) {
        let inclusive = self.inclusive().unwrap_or_default();
        let at = self.stack.first().map_or("?".into(), Frame::to_string);

        into.push_str(&format!(
            "{:indent$}{} {:>6} µs ({:>6} µs of its own)  at {at}\n",
            "",
            self.label,
            inclusive,
            self.exclusive().unwrap_or_default(),
            indent = depth * 2,
        ));
        for child in &self.children {
            child.outline(depth + 1, into);
        }
    }
}

/// The session: spans still open per shell, and the roots that have closed.
#[derive(Default)]
struct Timing {
    open: HashMap<Pid, Vec<Span>>,
    roots: Vec<Span>,
}

impl Timing {
    /// An unbalanced END is a defect in the instrument, not a shape to carry.
    fn end(&mut self, pid: Pid, at: Micros) -> Result<(), Failure> {
        let stack = self.open.get_mut(&pid).filter(|open| !open.is_empty()).ok_or_else(|| {
            Failure::new("closing a span", format!("an END from pid {pid} with no BEGIN"))
        })?;

        let mut span = stack.pop().expect("a non-empty stack");
        span.ended = Some(at);

        match stack.last_mut() {
            Some(parent) => parent.children.push(span),
            None => self.roots.push(span),
        }
        Ok(())
    }

    /// Spans left open when the run ended: their shell never reached the END.
    fn unclosed(&self) -> usize {
        self.open.values().map(Vec::len).sum()
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
        let Some((kind, rest)) = payload.split_first() else {
            return Err(Failure::new("reading a span", "an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => {
                let label = field(rest, "label")
                    .ok_or_else(|| Failure::new("reading a span", "no label"))?
                    .to_string();

                timing.open.entry(said.pid).or_default().push(Span {
                    label,
                    began: said.sent_at,
                    ended: None,
                    stack: Columns::of(rest)?.frames()?,
                    children: Vec::new(),
                });
                Ok(())
            }
            "END" => timing.end(said.pid, said.sent_at),
            other => Err(Failure::new("reading a span", format!("unknown kind {other:?}"))),
        }
    }
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

/// A µs budget for scheduling and for the `sleep` each pause forks. Wide,
/// because the bound it guards only has to separate `a`'s own two pauses from
/// the whole tree's time — an order of magnitude apart.
const SLACK: u64 = 60_000;

#[test]
fn measurements_nest_the_way_the_calls_do() {
    let scripts = Scripts::of(&[("tree.bash", TREE)]);
    let (timing, status) =
        run(&Profiling, &bash(scripts.at("tree.bash"))).unwrap().whole().unwrap();

    assert_eq!(status, ExitStatus::Code(0));
    assert_eq!(timing.unclosed(), 0, "every span that opened also closed");
    assert_eq!(timing.open.len(), 1, "one shell produced all of it");

    // The tree fell out of the nesting; nothing on the wire identified a pair.
    assert_eq!(timing.roots.len(), 1, "one outermost measurement");
    let a = &timing.roots[0];
    assert_eq!(a.label, "a");

    let outline = {
        let mut out = String::new();
        a.outline(0, &mut out);
        out
    };
    println!("{outline}");

    let labels = |span: &Span| span.children.iter().map(|c| c.label.clone()).collect::<Vec<_>>();
    assert_eq!(labels(a), ["b", "e"]);
    assert_eq!(labels(a.child("b")), ["c", "d"]);
    assert_eq!(labels(a.child("e")), ["f"]);
    assert!(labels(a.child("b").child("c")).is_empty());

    // Each span covers at least what it slept for, its children included.
    for (label, span, slept) in [
        ("c", a.child("b").child("c"), 30_000),
        ("d", a.child("b").child("d"), 40_000),
        ("f", a.child("e").child("f"), 50_000),
        ("b", a.child("b"), 30_000 + 10_000 + 40_000),
        ("e", a.child("e"), 10_000 + 50_000),
        ("a", a, 20_000 + 80_000 + 20_000 + 60_000),
    ] {
        let took = span.inclusive().expect("a closed span");
        assert!(took >= slept, "{label} took {took} µs, less than the {slept} µs it slept\n{outline}");
    }

    // A span's own time is what it did outside its measured children — here,
    // the two unmeasured pauses in `f__A`.
    let own = a.exclusive().unwrap();
    assert!(
        (40_000..40_000 + SLACK).contains(&own),
        "a's own time is its two 20 ms pauses, got {own} µs\n{outline}"
    );

    // A leaf spends everything on itself.
    let leaf = a.child("e").child("f");
    assert_eq!(leaf.exclusive(), leaf.inclusive(), "nothing measured inside f");

    // The stack says where each call was made, which the nesting alone does
    // not: `c` and `d` are both children of `b`, made from different lines of
    // the same function.
    let called_from = |span: &Span| span.stack[0].funcname.clone();
    assert_eq!(called_from(a), "main", "the outermost call is in the script's own body");
    assert_eq!(called_from(a.child("b")), "f__A");
    assert_eq!(called_from(a.child("e")), "f__A");
    assert_eq!(called_from(a.child("b").child("c")), "f__B");
    assert_eq!(called_from(a.child("b").child("d")), "f__B");
    assert_eq!(called_from(a.child("e").child("f")), "f__E");

    assert_ne!(
        a.child("b").child("c").stack[0].lineno,
        a.child("b").child("d").stack[0].lineno,
        "two calls from one function, told apart by their line"
    );

    // The whole tree accounts for itself: every µs belongs to exactly one span.
    let mut total = 0;
    fn walk(span: &Span, into: &mut u64) {
        *into += span.exclusive().unwrap();
        for child in &span.children {
            walk(child, into);
        }
    }
    walk(a, &mut total);
    assert_eq!(total, a.inclusive().unwrap(), "exclusive times partition the root's\n{outline}");
}

/// A measured call is run unguarded, so `set -e` still ends the subject where
/// it would have without the instrument — and the span that was open when it
/// died stays open, which is what says where the shell went.
#[test]
fn a_failure_inside_a_span_is_the_subjects_own() {
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
    assert_eq!(timing.roots.len(), 1, "only the span that completed");
    assert_eq!(timing.roots[0].label, "ok");
    assert_eq!(timing.unclosed(), 1, "the doomed span never closed: the shell died inside it");
}
