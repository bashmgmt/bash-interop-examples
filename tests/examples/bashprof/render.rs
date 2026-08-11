//! One tree formatter, for whichever tree is being shown.
//!
//! Hylic's shipped [`TreeFormatCfg`] — the same one `resolve::pretty` renders
//! resolution trees with — driven fused over a forest. The head closure is the
//! only thing a caller supplies.

use hylic::prelude::{treeish, TreeFormatCfg, FUSED};

/// Each root on its own, children indented under their parent.
pub fn forest<N: Clone + Send + Sync + 'static>(
    roots: &[N],
    children: impl Fn(&N) -> Vec<N> + Send + Sync + 'static,
    head: impl Fn(&N) -> String + Send + Sync + 'static,
) -> String {
    let fold = TreeFormatCfg::new(head, "\n", "\n", "", "  ").make_fold();
    let shape = treeish(children);

    roots.iter().map(|root| FUSED.run(&fold, &shape, root)).collect::<Vec<_>>().join("\n")
}
