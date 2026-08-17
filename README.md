# bash-interop-examples

Worked programs against the public API of
[bash-interop](https://github.com/bashmgmt/bash-interop).

Each file under `tests/examples/` is one complete program, commented as it
goes: listening to a script, answering the questions it asks, streaming
messages as they arrive, reusing a shipped tool's instrument, and profiling.

```
cargo test --test examples -- --nocapture <name>
```

`tests/joined/` comes at the same API from bash's side. A fixture script
starts a server for itself as a coprocess, joins the session it made, and
keeps a merged view of two tools in an array it names. This is the
arrangement a script uses when it instruments itself instead of being run
under a tool.

```
cargo test --test merging
```

The crate is not published to crates.io. It exists so the examples can depend
on bash-interop, bashcap and bashprof at once.

Licensed under the MIT licence.
