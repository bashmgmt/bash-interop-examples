//! bash drives, and the session hands back a view of itself.
//!
//! Two shells speak. The session merges what they said into one ordered list,
//! and every answer writes that list into an array of the client's — named on
//! this binary's command line, because the variable is the client's and so is
//! its name.
//!
//! The merge is the whole session's, and a reaction is one shell's, so the list
//! is a resource the rig holds and hands a share of to each shell it builds a
//! reaction for. One pipe carries every shell, so pushing as messages arrive
//! *is* the merge — there is nothing to sort afterwards.
//!
//! Nothing is stamped here either. The wire already puts both clocks on every
//! message — the sending shell's `$EPOCHREALTIME` and the run's own — so
//! "merged with timestamps" is reading two fields, not keeping a log.
//!
//! This file is a program rather than a set of `#[test]`s, because a client
//! that drives its own session has to have something to start. With `serve` it
//! is that server; without it, it runs the fixture and checks what came out.
//!
//! `cargo test -p mb_resolver --test merging`

use std::cell::RefCell;
use std::iter::once;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use mb_resolver::bash::rig::{
    Answer, Failure, Layout, Message, Reacting, Rig, Serving, Shell, Workspace,
};
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

/// Everything the shells have said, in the order the session heard it. The
/// client's array is a projection of this and never a second copy of it — which
/// is why nothing here counts what has already been handed over.
type Merged = Rc<RefCell<Vec<(Arc<Shell>, Message)>>>;

/// The rig: it merges what it heard, and writes the merge where it was told.
struct Merging {
    /// The client's array, by name. From this binary's command line.
    into: String,
    heard: Merged,
}

/// One shell's part in that. The shell is a member, so every entry says who
/// said it without anything being read back off the message.
struct Merges {
    shell: Arc<Shell>,
    into: String,
    heard: Merged,
}

impl Rig for Merging {
    type Reaction = Merges;

    fn workspace(&self) -> Workspace {
        Workspace::Temporary
    }

    fn bash(&self) -> String {
        MERGE_INTO.to_string()
    }

    fn joined(&self, _at: &Layout, shell: Arc<Shell>) -> Result<Merges, Failure> {
        Ok(Merges { shell, into: self.into.clone(), heard: Rc::clone(&self.heard) })
    }
}

impl Reacting for Merges {
    /// The merge is the shared list; a shell keeps nothing of its own.
    type Kept = ();

    fn hear(&mut self, said: Message) -> Result<(), Failure> {
        self.heard.borrow_mut().push((Arc::clone(&self.shell), said));

        Ok(())
    }

    /// `MERGE` alone, and nothing else. The question is not kept: it is a turn
    /// in the protocol, not something a shell said.
    fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        let Some([]) = asked.behind("MERGE") else { return Ok(Answer::status(127)) };
        let entries = merged(&self.heard.borrow());

        Ok(Answer::of("__merge_into", once(self.into.clone()).chain(entries)))
    }

    fn finish(self) -> Result<(), Failure> {
        Ok(())
    }
}

impl Serving for Merging {}

/// The list as the client gets it, offsets counted from the first message.
///
/// A shell opens with an account of itself, which is what makes the shell and
/// gives it its number. It is not something the shell *said*, so it cannot be a
/// message here and there is nothing to filter out.
fn merged(heard: &[(Arc<Shell>, Message)]) -> Vec<String> {
    let Some((_, first)) = heard.first() else { return Vec::new() };

    heard.iter().map(|(shell, message)| entry(shell, message, first)).collect()
}

/// One line of the merge: which shell, how far into the session, how long the
/// message took to arrive, and the words themselves as a bash array literal —
/// so the client unpacks one level and gets its word boundaries back.
fn entry(shell: &Shell, message: &Message, first: &Message) -> String {
    let since = message.stamp.heard_at.0 - first.stamp.heard_at.0;
    let travelled = message.stamp.heard_at.0.abs_diff(message.stamp.sent_at.0);

    format!("{} {since} {travelled} {}", shell.nth + 1, emit_array(&message.words))
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
    let merging = Merging { into, heard: Rc::new(RefCell::new(Vec::new())) };
    let served = merging.serve_coprocess().expect("the session");

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
