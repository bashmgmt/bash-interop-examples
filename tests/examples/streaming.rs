//! A session that keeps nothing and holds a resource instead.
//!
//! `hear` writes each message as it arrives, so resident memory does not
//! track the run; `end` flushes, so a failed flush ends the run rather than
//! being lost in a `Drop`. This is the shape `bashcap` takes.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use mb_resolver::bash::rig::{run, Doing, ExitStatus, Failure, Line, Rig};

use crate::support::Scripts;

struct Logging {
    into: PathBuf,
}

/// The session: a sink and a tally. Neither is the rig's — a rig is `&self`
/// and never changes.
struct Writing {
    written: usize,
    sink: BufWriter<File>,
}

impl Rig for Logging {
    type Session = Writing;

    fn open(&self) -> Result<Writing, Failure> {
        let sink = File::create(&self.into).doing(|| format!("writing {}", self.into.display()))?;

        Ok(Writing { written: 0, sink: BufWriter::new(sink) })
    }

    fn hear(&self, session: &mut Writing, said: Line) -> Result<(), Failure> {
        let at = || format!("writing {}", self.into.display());

        writeln!(session.sink, "{} {}", said.pid, said.words.join(" ")).doing(at)?;
        session.written += 1;

        Ok(())
    }

    fn end(&self, session: &mut Writing, _status: ExitStatus) -> Result<(), Failure> {
        session.sink.flush().doing(|| format!("flushing {}", self.into.display()))
    }
}

#[test]
fn a_session_may_hold_a_resource_and_keep_no_messages() {
    let scripts = Scripts::of(&[(
        "main.bash",
        "BC_INSTR say REC one\n( BC_INSTR say REC from-a-subshell )\nBC_INSTR say REC two\n",
    )]);
    let into = scripts.at("said.log");

    let (session, status) =
        run(&Logging { into: into.clone() }, &[scripts.at("main.bash")]).unwrap();

    assert_eq!(status, ExitStatus::Code(0));

    // Exactly what the script said. A shell joining costs no message of its
    // own: provenance rides on the ones it goes on to write.
    assert_eq!(session.written, 3);

    let log = std::fs::read_to_string(&into).unwrap();
    assert_eq!(log.lines().filter(|line| line.contains("REC")).count(), 3);
    assert!(log.contains("from-a-subshell"));
}
