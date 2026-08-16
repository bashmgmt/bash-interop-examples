# bash-interop-examples — working in this crate

Worked rigs against the public APIs of bash-interop, bashcap and bashprof —
instructive, read top to bottom, never published. Examples may carry more
comments than the other crates; they teach. A corner case belongs in the crate
that owns the mechanism, not here.

```bash
cargo test --test examples -- --test-threads=1 --nocapture
cargo test --test merging
```
