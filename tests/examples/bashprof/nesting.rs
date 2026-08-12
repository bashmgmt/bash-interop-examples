//! Flat records read as a tree.
//!
//! Each record names the call it was made inside of, so the edges are given
//! and the shape follows from one index over them: which records name each
//! call. A [`Treeish`](hylic::graph::Treeish) over that index is the tree, and
//! a fold materialises it, exactly as `resolve::pipeline::resolution`
//! materialises a `Resolution`.
//!
//! Nothing here asks whether a call ended. Where a call sits does not depend
//! on how it went.

use std::collections::HashMap;
use std::sync::Arc;

use hylic::prelude::{treeish, vec_fold, VecHeap, FUSED};

use super::record::{Call, Id, Record};
use super::render;

/// A call, and everything called inside it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub record: Record,
    pub children: Arc<[Recorded]>,
}

impl Recorded {
    pub fn call(&self) -> &Call {
        self.record.call()
    }

    /// The calls in this forest the shell died inside, outermost first.
    pub fn unended(forest: &[Recorded]) -> Vec<&Call> {
        forest.iter().flat_map(Recorded::unended_here).collect()
    }

    fn unended_here(&self) -> Vec<&Call> {
        let own = match &self.record {
            Record::Unended(call) => Some(call),
            Record::Ended { .. } => None,
        };

        own.into_iter().chain(self.children.iter().flat_map(Recorded::unended_here)).collect()
    }

    /// The forest as it stands, ended and unended alike.
    pub fn render(forest: &[Recorded]) -> String {
        render::forest(forest, |node: &Recorded| node.children.to_vec(), |node: &Recorded| {
            let call = node.call();
            let took = match &node.record {
                Record::Ended { ended, .. } => format!("{} µs", ended.0 - call.began.0),
                Record::Unended(_) => "NEVER ENDED".to_string(),
            };

            format!("{} {took} at {} in pid {}", call.label, call.at, call.pid)
        })
    }
}

/// The records, and the one index the shape asks of them: which of them each
/// call was told it holds. Records come in the order they began, so every
/// list built from them is in that order too.
struct Nesting {
    records: Vec<Record>,
    inside: HashMap<Id, Vec<usize>>,
}

impl Nesting {
    fn of(records: Vec<Record>) -> Self {
        let mut inside: HashMap<Id, Vec<usize>> = HashMap::new();

        for (index, record) in records.iter().enumerate() {
            if let Some(outer) = &record.call().inside {
                inside.entry(outer.clone()).or_default().push(index);
            }
        }

        Self { records, inside }
    }

    fn children(&self, of: &Id) -> &[usize] {
        self.inside.get(of).map_or(&[], Vec::as_slice)
    }

    /// The calls nothing measured encloses.
    fn roots(&self) -> Vec<usize> {
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.call().inside.is_none())
            .map(|(index, _)| index)
            .collect()
    }
}

/// One record, and the neighbourhood it takes to ask for its children.
#[derive(Clone)]
struct At {
    index: usize,
    nesting: Arc<Nesting>,
}

impl At {
    fn record(&self) -> &Record {
        &self.nesting.records[self.index]
    }

    fn children(&self) -> Vec<At> {
        self.nesting
            .children(&self.record().call().id)
            .iter()
            .map(|&index| At { index, nesting: self.nesting.clone() })
            .collect()
    }
}

/// Read flat records as the forest their names describe.
pub fn nest(records: Vec<Record>) -> Vec<Recorded> {
    let nesting = Arc::new(Nesting::of(records));
    let shape = treeish(At::children);
    let build = vec_fold(|heap: &VecHeap<At, Recorded>| Recorded {
        record: heap.node.record().clone(),
        children: Arc::from(heap.childresults.as_slice()),
    });

    nesting
        .roots()
        .into_iter()
        .map(|index| At { index, nesting: nesting.clone() })
        .map(|root| FUSED.run(&build, &shape, &root))
        .collect()
}
