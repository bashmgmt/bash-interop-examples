//! bash drives, and the session hands back a view of itself.
//!
//! Two shells speak. The session merges what they said into one ordered list,
//! and every answer writes that list into an array of the client's — named on
//! this binary's command line, because the variable is the client's and so is
//! its name.
//!
//! Nothing is stamped here. The wire already puts both clocks on every message
//! — the sending shell's `$EPOCHREALTIME` and the run's own — so "merged with
//! timestamps" is reading two fields, not keeping a log.
//!
//! This file is a program rather than a set of `#[test]`s, because a client
//! that drives its own session has to have something to start. With `serve` it
//! is that server; without it, it runs the fixture and checks what came out.
//!
//! `cargo test -p mb_resolver --test merging`

use std::iter::once;
use std::process::Command;

use mb_resolver::bash::rig::{shells, Answer, Failure, Line, Rig, Slave};
use mb_resolver::bash::value::emit_array;

/// The word the rig's own answers call. A nameref is how bash lets a callee
/// write into a variable the caller named, which is what makes the target a
/// runtime choice rather than a compiled-in one.
const MERGE_INTO: &str = r#"
__merge_into() {
    local -n __merge_target="$1"
    shift
    __merge_target=("$@")
}
"#;

/// The rig: it merges what it heard, and writes the merge where it was told.
struct Merging {
    /// The client's array, by name. From this binary's command line.
    into: String,
}

impl Rig for Merging {
    /// Every message, in arrival order. The client's array is a projection of
    /// this and never a second copy of it — which is why nothing here counts
    /// what has already been handed over.
    type Session = Vec<Line>;

    fn bash(&self) -> String {
        MERGE_INTO.to_string()
    }

    fn open(&self) -> Result<Vec<Line>, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, heard: &mut Vec<Line>, said: Line) -> Result<(), Failure> {
        heard.push(said);

        Ok(())
    }

    /// `MERGE` alone, and nothing else. The question is not kept: it is a turn
    /// in the protocol, not something a shell said.
    fn answer(&self, heard: &mut Vec<Line>, asked: Line) -> Result<Answer, Failure> {
        let Some([]) = asked.behind("MERGE") else { return Ok(Answer::status(127)) };

        Ok(Answer::of("__merge_into", once(self.into.clone()).chain(merged(heard))))
    }
}

impl Slave for Merging {}

/// What the shells said, as one list ordered by the clock the run kept.
///
/// [`shells`] is the arrangement — one entry per shell, in the order they
/// joined — and merging them back by `heard_at` is what makes the numbering
/// mean something. One pipe carries every shell, so the order was already
/// there; the clock is what says so.
fn merged(heard: &[Line]) -> Vec<String> {
    let shells = shells(heard);
    let mut lines: Vec<(&Line, usize)> = shells
        .iter()
        .enumerate()
        .flat_map(|(at, shell)| shell.lines.iter().map(move |line| (*line, at + 1)))
        .collect();

    lines.sort_by_key(|(line, _)| line.sent.heard_at);

    let Some(&(first, _)) = lines.first() else { return Vec::new() };

    lines.iter().map(|&(line, shell)| entry(shell, line, first)).collect()
}

/// One line of the merge: which shell, how far into the session, how long the
/// message took to arrive, and the words themselves as a bash array literal —
/// so the client unpacks one level and gets its word boundaries back.
fn entry(shell: usize, line: &Line, first: &Line) -> String {
    let since = line.sent.heard_at.0 - first.sent.heard_at.0;
    let travelled = line.sent.heard_at.0.abs_diff(line.sent.sent_at.0);

    format!("{shell} {since} {travelled} {}", emit_array(&line.words))
}

fn main() {
    let mut argv = std::env::args().skip(1);

    match argv.next().as_deref() {
        Some("serve") => serve(into(&mut argv)),
        _ => demonstrate(),
    }
}

/// `serve --into <array>`.
fn into(argv: &mut impl Iterator<Item = String>) -> String {
    match (argv.next().as_deref(), argv.next()) {
        (Some("--into"), Some(name)) => name,
        _ => panic!("usage: merging serve --into <array>"),
    }
}

/// The server the fixture starts. It holds our standard input and reads the
/// address from our standard output; `BC_JOIN` is the word that does both.
fn serve(into: String) {
    let served = Merging { into }.serve_coprocess().expect("the session");

    assert!(served.failed.is_none(), "the session closed up cleanly");
}

/// The example: run the fixture, handing it this binary's own path as the
/// server it should start. What array that server writes into is the fixture's
/// decision, made on the command line it builds.
fn demonstrate() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/__fixtures/joined/merging.bash");
    let me = std::env::current_exe().expect("this binary");

    let ran = Command::new("bash").arg(fixture).arg(&me).output().expect("bash");

    // The entries as they were written, timestamps and all. They are on stderr
    // because the offsets are real measurements and nothing can assert them.
    eprint!("{}", String::from_utf8(ran.stderr).expect("the script's own stderr"));

    let said = String::from_utf8(ran.stdout).expect("the script's own stdout");
    assert_eq!(
        said,
        "\
3 entries
  shell 1 said 2 words: STEP alpha
  shell 2 said 2 words: STEP beta from a subshell
  shell 1 said 2 words: STEP gamma
4 entries
  shell 1 said 2 words: STEP alpha
  shell 2 said 2 words: STEP beta from a subshell
  shell 1 said 2 words: STEP gamma
  shell 1 said 2 words: STEP delta
unknown question: 127
server exited 0
",
        "the array grew with the merge, and the subshell is in it"
    );
    assert_eq!(ran.status.code(), Some(0));

    println!("merging: two shells, one merged view, written where the client asked");
}
