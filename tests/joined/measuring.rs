//! A build script that measures itself.
//!
//! bashprof is what `bashprof <cmd>` runs from the outside, and it is also this:
//! a script vendors the word, starts a server when it wants one, and the
//! measurements land where it asked. Nothing of the tool changes between the
//! two — `BashProf` implements both orchestrations, and a span is the interval
//! between two messages however the shells that sent them were started.
//!
//! The fixture runs either way, which is the whole of the vendoring contract:
//! the words come from `assets/bashprof.bash`, the empty hooks stand in when
//! nothing is listening, and joining replaces them with the ones that measure.
//!
//! `cargo test -p mb_resolver --test measuring`

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mb_resolver::bash::rig::{Doing, Failure, Line, Slave};
use mb_resolver::bashprof::{recorded, BashProf, Profile};

fn main() {
    let mut argv = std::env::args().skip(1);

    match argv.next().as_deref() {
        Some("serve") => serve(&into(&mut argv)),
        _ => demonstrate(),
    }
}

/// `serve --into <file>` — the same flag the tool's own command line takes,
/// and for the same reason: the subject owns both streams, so a reading goes
/// to a file the caller named.
fn into(argv: &mut impl Iterator<Item = String>) -> PathBuf {
    match (argv.next().as_deref(), argv.next()) {
        (Some("--into"), Some(path)) => PathBuf::from(path),
        _ => panic!("usage: measuring serve --into <file>"),
    }
}

/// The server the fixture starts. It serves until the script lets go, and only
/// then reads what it heard — which is why the script's `wait` is what tells it
/// the file is written.
fn serve(into: &Path) {
    let served = BashProf.serve_coprocess().expect("the session");
    assert!(served.failed.is_none(), "the session closed up cleanly");

    write(into, &served.session).expect("a reading of the run");
}

fn write(into: &Path, heard: &[Line]) -> Result<(), Failure> {
    let forest = recorded(heard)?;
    let profile = Profile::of(&forest)
        .map_err(|unfinished| Failure::new("reading the run", unfinished.to_string()))?;

    fs::write(into, profile.to_string()).doing(|| format!("writing {}", into.display()))
}

fn demonstrate() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/__fixtures/joined/build.bash");
    let me = std::env::current_exe().expect("this binary");
    let into = std::env::temp_dir().join(format!("bashprof-joined.{}", std::process::id()));

    // Nobody listening: the vendored word is defined, the hooks are the empty
    // ones the script installed, and the build is a build.
    let alone = Command::new("bash").arg(fixture).output().expect("bash");
    assert_eq!(String::from_utf8(alone.stdout).unwrap(), "built\n", "the script runs on its own");
    assert_eq!(alone.status.code(), Some(0));

    // The same script, with a server of its own. The measurements are on disk
    // by the time it exits, because it waited for the process it started.
    let joined = Command::new("bash")
        .arg(fixture)
        .arg(&me)
        .arg("serve")
        .arg("--into")
        .arg(&into)
        .output()
        .expect("bash");

    eprint!("{}", String::from_utf8(joined.stderr).expect("the script's own stderr"));
    assert_eq!(String::from_utf8(joined.stdout).unwrap(), "built\n", "and the same output");
    assert_eq!(joined.status.code(), Some(0));

    let reading = fs::read_to_string(&into).expect("the reading the server wrote");
    fs::remove_file(&into).expect("clearing up after ourselves");

    println!("{reading}");
    assert_eq!(
        shape(&reading),
        [(0, "build"), (1, "compile"), (2, "link"), (1, "package")],
        "the tree the calls made, indented by how deep they nested"
    );

    println!("measuring: a script measured itself, and the reading outlived it");
}

/// Each measured call as `(depth, label)`. The rendering indents two spaces per
/// level and leads with the label, which is all this needs of it.
fn shape(reading: &str) -> Vec<(usize, &str)> {
    reading
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let depth = (line.len() - line.trim_start().len()) / 2;

            (depth, line.trim_start().split(' ').next().expect("a label"))
        })
        .collect()
}
