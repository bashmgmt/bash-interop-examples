//! Nesting: three layers of scripts, the middle reaching the deepest through
//! a subshell. Four emitting shells, one forest, global ordering, a breakpoint
//! at the bottom, and a status that walks back out.

use mb_resolver::bash::rig::{converse, ExitStatus, Reply, Setup, ShellNode};

use crate::{args, fixture, report};

#[test]
fn three_layers_report_and_a_failure_walks_back_out() {
    // The answer is a `say` of its own, so it shows up in the same stream.
    let (seen, status) = converse(
        Setup::new(),
        &[fixture("rig_nested/layer1.bash")],
        |_seen, asked| {
            assert_eq!(asked.args(), ["probe", "deepest"]);
            Ok(Reply::of(["BC_INSTR", "say", "REC", "probe", "answered"]))
        },
    )
    .unwrap();

    // Global order across four shells and three processes. Each shell joins
    // the same pipe by name, so nothing had to be inherited to get here.
    assert_eq!(
        args(&seen, "REC"),
        [
            "layer1 enter",
            "layer2 enter",
            "layer2 subshell",
            "layer3 enter",
            "probe answered",
            "layer3 leave",
            "layer2 subshell-saw 5",
            "layer2 saw 5",
            "layer1 saw 5",
        ],
        "{}",
        report(&seen)
    );

    // layer1 process, layer2 process, layer2's subshell, layer3 process.
    let shells = seen.shells();
    assert_eq!(shells.len(), 4, "{}", report(&seen));
    assert!(shells.iter().all(|shell| shell.origin.is_some()));

    // The forest follows the emitting parent at every step, including through
    // the subshell, where $PPID would have named the grandparent.
    let forest = seen.forest();
    assert_eq!(forest.len(), 1);
    let layer2 = &forest[0].children[0];
    let subshell = &layer2.children[0];
    let layer3 = &subshell.children[0];
    assert_eq!(forest[0].children.len(), 1);
    assert_eq!(layer2.children.len(), 1);
    assert_eq!(subshell.children.len(), 1);
    assert!(layer3.children.is_empty());

    // SHLVL deepens monotonically down the chain of real processes.
    let level = |node: &ShellNode| node.shell.origin.as_ref().unwrap().shlvl;
    assert!(level(&forest[0]) < level(layer2));
    assert!(level(layer2) < level(layer3));

    // layer3's status is carried out through the subshell and both parents.
    assert_eq!(status, ExitStatus::Code(5));
}
