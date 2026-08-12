//! The wire read as flat records.
//!
//! Every BEGIN carries the name its shell gave the call and the name of the
//! call it was made inside of, so the tree is already on the wire. Reading it
//! is one pass over everything the run heard, with a map from name to call —
//! no grouping by shell, no pairing by position, and nothing that depends on
//! the order messages arrived in.
//!
//! What this pass owes the rest of the module is that the names hold up: a
//! name is given once, ended once, and every name a call points at is one
//! that was given. Each is a lookup, and each failure is the instrument's.

use std::collections::HashMap;

use mb_resolver::bash::rig::{field, Failure, Line, Micros};
use mb_resolver::bash::stack::Columns;

use super::record::{Call, Id, Record};

/// The word this instrument's messages begin with.
const TAG: &str = "TIME_CPS";

/// Every call the run made, in the order they began.
pub fn records(heard: &[Line]) -> Result<Vec<Record>, Failure> {
    let mut reading = Reading::default();

    for line in heard {
        reading.hear(line)?;
    }

    reading.settled()
}

/// A call while the run is still being read. It gains an end when its END
/// arrives, and a shell that dies inside it never sends one.
struct Open {
    call: Call,
    ended: Option<Micros>,
}

#[derive(Default)]
struct Reading {
    opened: Vec<Open>,

    /// Where each name's call is, which is what makes both an END and a
    /// parent a lookup rather than a search.
    at: HashMap<Id, usize>,
}

impl Reading {
    /// One message. Anything not this instrument's is someone else's.
    fn hear(&mut self, line: &Line) -> Result<(), Failure> {
        let Some(payload) = line.behind(TAG) else { return Ok(()) };
        let Some((kind, rest)) = payload.split_first() else {
            return Err(reading("an empty TIME_CPS message"));
        };

        match kind.as_str() {
            "BEGIN" => self.begin(line, rest),
            "END" => self.end(named(rest)?, line.sent_at),
            other => Err(reading(format!("unknown kind {other:?}"))),
        }
    }

    /// A name is given once. Two calls under one name would close each other's
    /// spans and claim each other's children, so it is refused here rather
    /// than read as a tree nothing produced.
    fn begin(&mut self, line: &Line, rest: &[String]) -> Result<(), Failure> {
        let call = began(line, rest)?;

        if self.at.insert(call.id.clone(), self.opened.len()).is_some() {
            return Err(reading(format!("a second call named {}", call.id)));
        }

        self.opened.push(Open { call, ended: None });
        Ok(())
    }

    fn end(&mut self, id: Id, ended: Micros) -> Result<(), Failure> {
        let unknown = || reading(format!("an END for {id}, which never began"));
        let open = &mut self.opened[*self.at.get(&id).ok_or_else(unknown)?];

        if open.ended.is_some() {
            return Err(reading(format!("a second END for {id}")));
        }

        open.ended = Some(ended);
        Ok(())
    }

    /// The calls as records, oldest first — once every name they point at is
    /// one that was given.
    fn settled(self) -> Result<Vec<Record>, Failure> {
        let Reading { opened, at } = self;

        let named_but_absent = opened
            .iter()
            .filter_map(|open| open.call.inside.as_ref().map(|inside| (&open.call.id, inside)))
            .find(|(_, inside)| !at.contains_key(inside));

        if let Some((call, missing)) = named_but_absent {
            return Err(reading(format!("{call} was made inside {missing}, which never began")));
        }

        let mut records: Vec<Record> = opened
            .into_iter()
            .map(|Open { call, ended }| match ended {
                Some(ended) => Record::Ended { call, ended },
                None => Record::Unended(call),
            })
            .collect();

        records.sort_by_key(|record| (record.call().began, record.call().pid.0));
        Ok(records)
    }
}

/// The `id` an END names.
fn named(rest: &[String]) -> Result<Id, Failure> {
    field(rest, "id").map(|id| Id(id.to_string())).ok_or_else(|| reading("a message with no id"))
}

fn began(line: &Line, rest: &[String]) -> Result<Call, Failure> {
    let word = |key: &str| {
        field(rest, key).ok_or_else(|| reading(format!("a BEGIN with no {key:?}"))).map(str::to_string)
    };

    let mut frames = Columns::of(rest)?.frames()?.into_iter();
    let at = frames.next().ok_or_else(|| reading("a walk with no frames"))?;

    Ok(Call {
        id: named(rest)?,
        // The outermost call is made inside nothing, and says so with a name
        // it leaves empty.
        inside: Some(word("inside")?).filter(|inside| !inside.is_empty()).map(Id),
        label: word("label")?,
        pid: line.pid,
        began: line.sent_at,
        at,
        outer: frames.collect(),
    })
}

fn reading(what: impl Into<String>) -> Failure {
    Failure::new("reading a span", what.into())
}
