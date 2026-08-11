//! Flat records read as a tree.
//!
//! One fact is computed — each record's parent — and the shape follows from
//! it. [`Nesting`] answers "whose children are these" and "which have no
//! parent"; a [`Treeish`](hylic::graph::Treeish) over those answers is the
//! tree, and a fold materialises it, exactly as
//! `resolve::pipeline::resolution` materialises a `Resolution`.
//!
//! Nothing here asks whether a call ended. A record's place is in its stack.

use std::sync::Arc;

use hylic::prelude::{treeish, vec_fold, VecHeap, FUSED};

use super::record::{Call, Record};
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
            match &node.record {
                Record::Ended { call, ended } => {
                    format!("{} {} µs at {}", call.label, ended.0 - call.began.0, call.at)
                }
                Record::Unended(call) => format!("{} NEVER ENDED at {}", call.label, call.at),
            }
        })
    }
}

/// Records paired with the one call each was made from inside of.
struct Nesting {
    records: Vec<Record>,
    parents: Vec<Option<usize>>,
}

impl Nesting {
    fn of(records: Vec<Record>) -> Self {
        let parents = (0..records.len()).map(|child| parent(&records, child)).collect();

        Self { records, parents }
    }

    fn children(&self, of: usize) -> Vec<usize> {
        self.whose_parent(|parent| *parent == Some(of))
    }

    fn roots(&self) -> Vec<usize> {
        self.whose_parent(Option::is_none)
    }

    fn whose_parent(&self, is: impl Fn(&Option<usize>) -> bool) -> Vec<usize> {
        self.parents
            .iter()
            .enumerate()
            .filter(|(_, parent)| is(parent))
            .map(|(index, _)| index)
            .collect()
    }
}

/// The innermost call `child` was made from inside of: the deepest site among
/// the records enclosing it that were still running when it began.
fn parent(records: &[Record], child: usize) -> Option<usize> {
    let inner = records[child].call();

    records
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != child)
        .filter(|(_, record)| record.call().encloses(inner))
        .filter(|(_, record)| record.running_at(inner.began))
        .max_by_key(|(_, record)| record.call().depth())
        .map(|(index, _)| index)
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
            .children(self.index)
            .into_iter()
            .map(|index| At { index, nesting: self.nesting.clone() })
            .collect()
    }
}

/// Read flat records as the forest their stacks describe.
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

#[cfg(test)]
mod tests {
    use mb_resolver::bash::rig::{Micros, Pid};
    use mb_resolver::bash::stack::Frame;

    use super::*;

    /// A call at `began`, made on the stack `site` written innermost first.
    fn call(label: &str, pid: u32, began: u64, site: &[(&str, u32)]) -> Call {
        let frame = |&(funcname, lineno): &(&str, u32)| Frame {
            funcname: funcname.into(),
            source: "/x.bash".into(),
            lineno,
            args: None,
        };

        Call {
            label: label.into(),
            pid: Pid(pid),
            began: Micros(began),
            at: frame(&site[0]),
            outer: site[1..].iter().map(frame).collect(),
        }
    }

    fn ended(call: Call, ended: u64) -> Record {
        Record::Ended { call, ended: Micros(ended) }
    }

    /// Every label in the forest, parents before their children.
    fn shape(forest: &[Recorded]) -> Vec<String> {
        forest
            .iter()
            .flat_map(|node| {
                let own = format!("{}({})", node.call().label, node.children.len());
                std::iter::once(own).chain(shape(&node.children))
            })
            .collect()
    }

    #[test]
    fn a_call_made_inside_another_nests_under_it() {
        let outer = ended(call("a", 1, 0, &[("main", 8)]), 100);
        let inner = ended(call("b", 1, 10, &[("f__A", 5), ("main", 8)]), 20);

        assert_eq!(shape(&nest(vec![outer, inner])), ["a(1)", "b(0)"]);
    }

    /// A stack outlives a fork, so a call measured in a subshell belongs to
    /// the call it was made from — which the shell it ran in cannot say.
    #[test]
    fn a_call_from_a_forked_shell_nests_by_its_stack() {
        let parent = ended(call("a", 1, 0, &[("main", 8)]), 100);
        let forked = ended(call("sub", 2, 10, &[("f__A", 6), ("main", 8)]), 20);

        assert_eq!(shape(&nest(vec![parent, forked])), ["a(1)", "sub(0)"]);
    }

    /// Two turns of a loop share a site exactly, so neither encloses the
    /// other and the stack cannot say which one a child belongs to. When each
    /// ran does.
    #[test]
    fn calls_from_one_line_are_siblings_and_their_children_go_by_time() {
        let site = [("f__A", 5), ("main", 8)];
        let inside = [("f__B", 2), ("f__A", 5), ("main", 8)];

        let forest = nest(vec![
            ended(call("a", 1, 0, &[("main", 8)]), 100),
            ended(call("turn", 1, 10, &site), 30),
            ended(call("first", 1, 15, &inside), 20),
            ended(call("turn", 1, 40, &site), 60),
            ended(call("second", 1, 45, &inside), 50),
        ]);

        assert_eq!(shape(&forest), ["a(2)", "turn(1)", "first(0)", "turn(1)", "second(0)"]);
    }

    #[test]
    fn a_call_no_other_encloses_is_a_root() {
        let forest = nest(vec![
            ended(call("a", 1, 0, &[("main", 8)]), 10),
            ended(call("b", 1, 20, &[("main", 9)]), 30),
        ]);

        assert_eq!(shape(&forest), ["a(0)", "b(0)"]);
    }

    /// Where a call sits does not depend on how it went, so a call the shell
    /// died inside keeps what completed within it — and is itself placed.
    #[test]
    fn a_call_that_never_ended_is_placed_and_keeps_its_children() {
        let forest = nest(vec![
            Record::Unended(call("outer", 1, 0, &[("main", 8)])),
            ended(call("done", 1, 10, &[("f__O", 3), ("main", 8)]), 20),
        ]);

        assert_eq!(shape(&forest), ["outer(1)", "done(0)"]);
        assert_eq!(
            Recorded::unended(&forest).iter().map(|call| &call.label).collect::<Vec<_>>(),
            ["outer"]
        );
    }
}
