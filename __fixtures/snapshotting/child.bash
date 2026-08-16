#!/usr/bin/env bash
# The child the snapshotting fixture starts: its own pid, its own shell.
declare -F BASHCAP >/dev/null || BASHCAP() { :; }

finish() { BASHCAP -BCS:"publishing from the child"; }
finish "an argument the trace records"
