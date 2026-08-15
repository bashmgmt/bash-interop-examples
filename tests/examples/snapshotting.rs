//! Reusing another tool's instrument: bashcap's bash and its decoder, with no
//! command line and no JSON in between.
//!
//! `bashcap run --into out.jsonl --trace-calls bash script.bash` is
//! this rig writing to a file. Here each shell's reaction keeps typed captures,
//! so what is reused is exactly the pair that matters — the bash that harvests
//! a shell, and the code that reads one back. The rendering comes with them:
//! `Capture` is `Display`, and `bashcap show` prints the same text.
//!
//! `cargo test --test examples -- --nocapture snapshotting`

use std::sync::Arc;

use mb_resolver::bash::rig::{
    Answer, Doing, Driving, ExitStatus, Failure, Layout, Message, Reached, Reaching, Reacting, Rig, Setup,
    Shell,
};
use mb_resolver::bashcap::{instrument, Capture, Tracing, LABEL};

use crate::fixture;
use crate::support::bash;

struct Snapshots;

/// One shell's snapshots. The shell is a member rather than something looked up
/// per message: a walk is read against the shell it was taken in, and
/// `BASH_SOURCE` alone cannot say what its own words mean.
struct Seen {
    shell: Arc<Shell>,
    captures: Vec<Capture>,
}

impl Rig for Snapshots {
    type Reaction = Seen;

    /// bashcap's instrument, in every shell the subject starts, asking for
    /// the full stack. `Tracing::Calls` is not free: `extdebug` also makes
    /// `ERR`, `DEBUG` and `RETURN` traps inherited by functions and
    /// subshells, so a subject with traps of its own behaves differently
    /// under it. That is why it is asked for rather than assumed.
    fn setup(&self) -> Setup {
        Setup { label: LABEL.to_string(), bash: instrument(Tracing::Calls) }
    }

    async fn joined(&self, _at: &Layout, shell: Arc<Shell>) -> Result<Seen, Failure> {
        Ok(Seen { shell, captures: Vec::new() })
    }
}

impl Reacting for Seen {
    type Kept = Vec<Capture>;

    /// Recognise, then decode. `None` is some other tool's message, and a
    /// snapshot that will not decode ends the run.
    async fn hear(&mut self, said: Message) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said, &self.shell) else {
            return Ok(());
        };

        self.captures.push(decoded.doing(|| format!("a snapshot from pid {}", self.shell.pid))?);

        Ok(())
    }

    /// It only listens, so a question is heard and the word reported unknown.
    async fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        self.hear(asked).await?;

        Ok(Answer::unknown())
    }

    async fn finish(self) -> Result<Vec<Capture>, Failure> {
        Ok(self.captures)
    }
}

#[tokio::test]
async fn a_tools_instrument_is_reusable_without_its_command_line() {
    let ran =
        Reached { rig: Snapshots, reaching: Reaching::BashEnv }
            .run(&bash(fixture("bashcap_demo/demo.bash")))
            .await
            .unwrap()
            .whole()
            .unwrap();
    assert_eq!(ran.subject, ExitStatus::Code(0));

    let seen: Vec<&Capture> = ran.shells.iter().flat_map(|at| &at.kept).collect();
    for (at, capture) in seen.iter().enumerate() {
        println!("[{at}] {capture}");
    }

    // The fixture is meant to be edited: what follows holds for any script
    // that calls `BASHCAP`, and reads none of its lines, names or counts.
    assert!(!seen.is_empty(), "an instrumented script took at least one snapshot");
    assert!(ran.shells.len() > 1, "the fixture's subshell and child are shells of their own");

    for capture in &seen {
        assert!(
            capture.snapshot.stack.frames().next().is_some(),
            "pid {} says where it is",
            capture.shell.pid
        );
        assert!(capture.snapshot.state.contains_key("seconds"), "and how long it had been up");
    }

    // `BASH_ENV` reaches every shell; a command line would have reached only
    // the first.
    let frames: Vec<_> = seen.iter().flat_map(|capture| capture.snapshot.stack.frames()).collect();
    assert!(
        frames.iter().all(|frame| frame.args.is_some()),
        "asking for the full stack gets it in every shell, not just where the run started"
    );
    assert!(
        frames.iter().any(|frame| frame.args.as_deref().is_some_and(|args| !args.is_empty())),
        "and the arguments are the real ones, not empty lists"
    );
}
