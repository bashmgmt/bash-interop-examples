#!/usr/bin/env bash
# The subject the snapshotting example drives. The test reads none of its
# line numbers, names or counts — edit freely.
#
# Driven, the injected instrument defines BASHCAP before this file runs;
# standalone, the guard makes the word a no-op.
declare -F BASHCAP >/dev/null || BASHCAP() { :; }

declare -a stages=(configure pack publish)

configure() { BASHCAP -BCV:stages -BCS:"configuring $1"; }
configure "the demo target"

# A subshell is a shell of its own on the wire.
( BASHCAP -BCS:"packing, one level down" )

# So is a child process.
bash "$(dirname "${BASH_SOURCE[0]}")/child.bash"
