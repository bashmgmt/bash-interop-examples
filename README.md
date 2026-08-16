# bash-interop-examples

Worked rigs against the `bash-interop` public API, meant to be read top to
bottom — listening, answering, streaming, reusing a tool's instrument
(`bashcap`), profiling (`bashprof`) — and `tests/joined/`, the same from
bash's side: a fixture script that starts a server for itself and keeps a
merged view in an array it named.

```
cargo test --test examples -- --nocapture <name>
cargo test --test merging
```

Never published; this crate exists so the examples can depend on everything.
