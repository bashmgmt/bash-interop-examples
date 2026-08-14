//! A session that keeps nothing and holds a resource instead.
//!
//! One reaction per shell, and one file for all of them. What is shared is the
//! caller's own — the rig opens it and hands each reaction a share — which is
//! why the core names no sharing discipline. `hear` writes as each message
//! arrives, so resident memory does not track the run; `finish` flushes, so a
//! failed flush ends the run rather than being lost in a `Drop`.
//!
//! This is the shape `bashcap` takes.

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use mb_resolver::bash::rig::{
    Doing, ExitStatus, Failure, Laid, Line, Master, Reacting, Rig, Shell,
};

use crate::support::{bash, Scripts};

type Sink = Rc<RefCell<BufWriter<File>>>;

struct Logging {
    into: PathBuf,
    sink: Sink,
}

impl Logging {
    fn writing(into: PathBuf) -> Result<Self, Failure> {
        let file = File::create(&into).doing(|| format!("writing {}", into.display()))?;

        Ok(Self { into, sink: Rc::new(RefCell::new(BufWriter::new(file))) })
    }
}

/// One shell's share of the log. The shell is a member, so the pid on every
/// line is what that shell said it was rather than something read off each
/// message.
struct Writing {
    shell: Arc<Shell>,
    into: PathBuf,
    sink: Sink,
    written: usize,
}

impl Rig for Logging {
    type Attending = Writing;

    fn joined(&self, _at: &Laid, shell: Arc<Shell>) -> Result<Writing, Failure> {
        Ok(Writing { shell, into: self.into.clone(), sink: Rc::clone(&self.sink), written: 0 })
    }
}

impl Reacting for Writing {
    /// How many lines this shell wrote. What they said is in the file.
    type Kept = usize;

    fn hear(&mut self, said: Line) -> Result<(), Failure> {
        let at = || format!("writing {}", self.into.display());

        writeln!(self.sink.borrow_mut(), "{} {}", self.shell.pid, said.words.join(" ")).doing(at)?;
        self.written += 1;

        Ok(())
    }

    fn finish(self) -> Result<usize, Failure> {
        self.sink.borrow_mut().flush().doing(|| format!("flushing {}", self.into.display()))?;

        Ok(self.written)
    }
}

impl Master for Logging {}

#[test]
fn a_session_may_hold_a_resource_and_keep_no_messages() {
    let scripts = Scripts::of(&[(
        "main.bash",
        r#"
        BC_INSTR say REC one
        ( BC_INSTR say REC from-a-subshell )
        BC_INSTR say REC two
        "#,
    )]);
    let into = scripts.at("said.log");

    let ran = Logging::writing(into.clone())
        .unwrap()
        .run(&bash(scripts.at("main.bash")))
        .unwrap()
        .whole()
        .unwrap();

    assert_eq!(ran.subject, ExitStatus::Code(0));

    // Two shells and three `say`s. A shell announcing itself is not a message:
    // it is what makes the shell, and what a reaction is built from.
    assert_eq!(ran.shells.len(), 2, "the subshell is a shell of its own");
    assert_eq!(ran.shells.iter().map(|at| at.kept).sum::<usize>(), 3, "what the script said");

    let log = std::fs::read_to_string(&into).unwrap();
    assert_eq!(log.lines().filter(|line| line.contains("REC")).count(), 3);
    assert!(log.contains("from-a-subshell"));
}
