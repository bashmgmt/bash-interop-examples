//! Timing a tree of calls, and reading the two shapes a run can have.
//!
//! bashprof is a rig like any other: `run` gives back every message, and the
//! reading is a separate step the caller takes when it wants to. That split is
//! the point — a run that died mid-call still recorded everything up to it.
//!
//! `cargo test --test examples -- --nocapture profiling`

use mb_resolver::bash::rig::{heard, Driving, ExitStatus};
use mb_resolver::bashprof::{recorded, BashProf, Profile};

use crate::support::{bash, Scripts};

/// A build that calls two steps, one of which calls a third. Every `sleep` is
/// work that belongs to the call it sits in.
const BUILD: &str = r#"
    compile() { sleep 0.02; link; }
    link()    { sleep 0.01; }
    test()    { sleep 0.03; }

    build() {
        BASHPROF_TIMETHIS compile compile
        BASHPROF_TIMETHIS test test
    }

    BASHPROF_TIMETHIS build build
    "#;

#[test]
fn a_run_is_read_as_a_tree_and_then_as_measurements() {
    let scripts = Scripts::of(&[("build.bash", BUILD)]);

    // Step one: run it. Every shell that joined kept what it said.
    let ran = BashProf.run(&bash(scripts.at("build.bash"))).unwrap().whole().unwrap();
    assert_eq!(ran.subject, ExitStatus::Code(0), "the subject's own status, as always");

    // Step two: read those messages as the tree the calls made. `heard` puts
    // the per-shell foldings back in arrival order, each message with the shell
    // that sent it — a walk means nothing without it. Every call that began is
    // in the tree, whether or not it ended.
    let forest = recorded(&heard(&ran.shells)).expect("the instrument's own messages");
    println!("as recorded:\n{}\n", mb_resolver::bashprof::Recorded::render(&forest));

    // Step three: read that tree as measurements. This is the step that can
    // refuse — a call the shell died inside has no duration to report.
    let profile = Profile::of(&forest).expect("every call that began also ended");
    println!("as timings:\n{profile}");

    let build = &profile.roots[0];
    assert_eq!(build.complete.call.label, "build");
    assert_eq!(build.complete.status, 0, "and what the measured command returned");

    // The tree is the nesting of the calls, not of the shells or the files.
    let inside: Vec<&str> =
        build.children.iter().map(|span| span.complete.call.label.as_str()).collect();
    assert_eq!(inside, ["compile", "test"]);

    // A span covers its own work and everything it called; what no child was
    // running for is its own.
    assert!(build.complete.took() >= 60_000, "the whole build slept 60 ms");
    assert!(build.exclusive() < build.complete.took(), "most of which was inside its children");

    // Where the call was made, which the nesting alone does not say.
    let at = build.complete.call.stack.top();
    assert_eq!(at.site.to_string(), "main");
    assert!(at.source.found().is_some(), "and the file it was made in is right there");
}

/// The other shape: the shell dies inside a call, so there is no whole
/// profile — and the measurements that did complete are no less true for it.
#[test]
fn a_run_that_died_mid_call_still_measured_what_completed() {
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"set -e
        ok()     { :; }
        broken() { false; }

        BASHPROF_TIMETHIS ok ok
        BASHPROF_TIMETHIS doomed broken
        "#,
    )]);

    let ran = BashProf.run(&bash(scripts.at("build.bash"))).unwrap().whole().unwrap();
    assert_eq!(ran.subject, ExitStatus::Code(1), "the subject failed, so the run reports that");

    let forest = recorded(&heard(&ran.shells)).unwrap();
    let unfinished = Profile::of(&forest).expect_err("the shell died inside `doomed`");

    // The error carries the result: a caller that can proceed does.
    let labels: Vec<&str> =
        unfinished.resolved.roots.iter().map(|span| span.complete.call.label.as_str()).collect();
    assert_eq!(labels, ["ok"]);
    assert_eq!(unfinished.unended().iter().map(|call| call.label.as_str()).collect::<Vec<_>>(),
        ["doomed"]);
}
