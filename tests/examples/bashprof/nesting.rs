//! Placed records read as a tree.
//!
//! The relation is already in them — each says which call it was made inside
//! of, on the authority of the shell that produced it — so what is left is the
//! unfold. A [`Treeish`](hylic::graph::Treeish) over "whose children are
//! these" and a fold that materialises them, exactly as
//! `resolve::pipeline::resolution` materialises a `Resolution`.
//!
//! Nothing here asks whether a call ended. Where a call sits does not depend
//! on how it went.

use std::sync::Arc;

use hylic::prelude::{treeish, vec_fold, VecHeap, FUSED};

use super::record::{Call, Placed, Record};
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

/// Which of these were made inside `of`, in the order they began. `None` is
/// the forest's own roots, and the two are one question.
fn made_inside(placed: &[Placed], of: Option<usize>) -> Vec<usize> {
    let mut found: Vec<usize> = placed
        .iter()
        .enumerate()
        .filter(|(_, one)| one.inside == of)
        .map(|(index, _)| index)
        .collect();

    found.sort_by_key(|&index| placed[index].record.call().began);
    found
}

/// One record, and the neighbourhood it takes to ask for its children.
#[derive(Clone)]
struct At {
    index: usize,
    placed: Arc<Vec<Placed>>,
}

impl At {
    fn record(&self) -> &Record {
        &self.placed[self.index].record
    }

    fn children(&self) -> Vec<At> {
        made_inside(&self.placed, Some(self.index))
            .into_iter()
            .map(|index| At { index, placed: self.placed.clone() })
            .collect()
    }
}

/// Read placed records as the forest they describe.
pub fn nest(placed: Vec<Placed>) -> Vec<Recorded> {
    let roots = made_inside(&placed, None);
    let placed = Arc::new(placed);
    let shape = treeish(At::children);
    let build = vec_fold(|heap: &VecHeap<At, Recorded>| Recorded {
        record: heap.node.record().clone(),
        children: Arc::from(heap.childresults.as_slice()),
    });

    roots
        .into_iter()
        .map(|index| At { index, placed: placed.clone() })
        .map(|root| FUSED.run(&build, &shape, &root))
        .collect()
}
