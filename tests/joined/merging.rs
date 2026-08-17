//! bash drives, and the session hands back a view of itself.
//!
//! Two shells speak. The session merges what they said into one ordered list,
//! and every answer writes that list into an array the client owns and names —
//! on this binary's command line, since the name is the client's to choose.
//!
//! The merge is the whole session's, and a reaction is one shell's, so the list
//! is a resource the rig holds and hands a share of to each shell it builds a
//! reaction for. Every shell has a pipe of its own, so the order they were
//! *said* in is the sending shell's clock, which every message carries; the
//! merge sorts on it when it is asked for.
//!
//! Nothing is stamped here either. The wire already puts both clocks on every
//! message — the sending shell's `$EPOCHREALTIME` and the run's own — so
//! "merged with timestamps" is reading two fields, not keeping a log.
//!
//! This file is a program rather than a set of `#[test]`s, because a client
//! that drives its own session has to have something to start. With `serve` it
//! is that server; without it, it runs the fixture and checks what came out.
//!
//! `cargo test --test merging`

use std::cell::RefCell;
use std::iter::once;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use bash_interop::rig::{Answer, Failure, Layout, Message, Reacting, Rig, Serving, Shell};
use bash_strings::emit_array;

/// `RUST_LOG` filters, `info` by default.
fn logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
}

/// What the subject's shells get: the word the fixture speaks, and the word
/// this rig's own answers call. A nameref is how bash lets a callee write into
/// a variable the caller named, which makes the target a runtime choice rather
/// than a compiled-in one.
const MERGE_INTO: &str = r#"
alias STEP='BC_SAY__ARG_LABEL=MERGE BC_SAY STEP'

__merge_into() {
    declare -n __merge_target="${1:?the array to write}"
    shift
    __merge_target=("$@")
}
"#;

/// Everything the shells have said. The client's array is a projection of this
/// and never a second copy of it — which is why nothing here counts what has
/// already been handed over.
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

    fn bash(&self, _at: &Layout) -> String {
        MERGE_INTO.to_string()
    }

    async fn joined(&self, _at: &Layout, shell: Arc<Shell>) -> Result<Merges, Failure> {
        Ok(Merges {
            shell,
            into: self.into.clone(),
            heard: Rc::clone(&self.heard),
        })
    }
}

impl Reacting for Merges {
    /// The merge is the shared list; a shell keeps nothing of its own.
    type Kept = ();

    async fn hear(&mut self, said: Message) -> Result<(), Failure> {
        self.heard
            .borrow_mut()
            .push((Arc::clone(&self.shell), said));

        Ok(())
    }

    /// `MERGE` alone, and nothing else. The question is not kept: it is a turn
    /// in the protocol, not something a shell said.
    async fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        let Some([]) = asked.behind("MERGE") else {
            return Ok(Answer::status(127));
        };
        let entries = merged(&self.heard.borrow());

        Ok(Answer::of(
            "__merge_into",
            once(self.into.clone()).chain(entries),
        ))
    }

    async fn finish(self) -> Result<(), Failure> {
        Ok(())
    }
}

impl Serving for Merging {}

/// The list as the client gets it, in the order the shells said it, offsets
/// counted from the first message.
///
/// A shell opens with an account of itself, which is what makes the shell and
/// gives it its number. It is not something the shell *said*, so it cannot be a
/// message here and there is nothing to filter out.
fn merged(heard: &[(Arc<Shell>, Message)]) -> Vec<String> {
    let mut said: Vec<&(Arc<Shell>, Message)> = heard.iter().collect();
    said.sort_by_key(|(_, message)| message.stamp.sent_at);

    let Some((_, first)) = said.first() else {
        return Vec::new();
    };

    said.iter()
        .map(|(shell, message)| entry(shell, message, first))
        .collect()
}

/// One line of the merge: which shell, how far into the session, how long the
/// message took to arrive, and the words themselves as a bash array literal —
/// so the client unpacks one level and gets its word boundaries back.
fn entry(shell: &Shell, message: &Message, first: &Message) -> String {
    let since = message.stamp.sent_at.0 - first.stamp.sent_at.0;
    let travelled = message.stamp.heard_at.0.abs_diff(message.stamp.sent_at.0);

    format!(
        "{} {since} {travelled} {}",
        shell.nth + 1,
        emit_array(&message.words)
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    logging();

    let mut argv = std::env::args().skip(1);

    match argv.next().as_deref() {
        Some("serve") => {
            let (at, into) = arguments(&mut argv);
            serve(at, into).await;
        }
        _ => demonstrate(),
    }
}

/// `serve --at <dir> --into <array>`.
fn arguments(argv: &mut impl Iterator<Item = String>) -> (PathBuf, String) {
    match (
        argv.next().as_deref(),
        argv.next(),
        argv.next().as_deref(),
        argv.next(),
    ) {
        (Some("--at"), Some(at), Some("--into"), Some(name)) => (at.into(), name),
        _ => panic!("usage: merging serve --at <dir> --into <array>"),
    }
}

/// The server the fixture starts. It holds our standard input; nothing is
/// written back — the fixture probes the workspace it named and attaches.
async fn serve(at: PathBuf, into: String) {
    let merging = Merging {
        into,
        heard: Rc::new(RefCell::new(Vec::new())),
    };
    let served = merging.serve_coprocess(&at).await.expect("the session");

    assert!(
        served.failed.is_none(),
        "the session closed up cleanly"
    );
}

/// The example: run the fixture, handing it this binary's own path as the
/// server it should start. The fixture decides which array that server writes
/// into, on the command line it builds.
fn demonstrate() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/__fixtures/merging.bash"
    );
    let me = std::env::current_exe().expect("this binary");

    let ran = Command::new("bash")
        .arg(fixture)
        .arg(&me)
        .output()
        .expect("bash");

    // The entries as they were written, timestamps and all. They are logged
    // because the offsets are real measurements and nothing can assert them.
    log::info!(
        "{}",
        String::from_utf8(ran.stderr).expect("the script's own stderr")
    );

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

    log::info!("merging: two shells, one merged view, written where the client asked");
}
