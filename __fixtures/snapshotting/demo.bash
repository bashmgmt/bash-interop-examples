#!/usr/bin/env bash
# Everything bashcap captures, in one script. No test asserts these line
# numbers — edit freely.

source "$(dirname "${BASH_SOURCE[0]}")/bashcap.bash"
declare -F __bc_capture >/dev/null || __bc_capture() { :; }

declare -- greeting="hello world"
declare -a items=(alpha "beta gamma")
declare -A conf=([host]=localhost [port]=8080)
declare -i attempts=3

# Every BASHCAP__CTX__* variable joins every later snapshot.
BASHCAP__CTX__phase=setup

[[ "build-2026-08" =~ ^([a-z]+)-([0-9]{4})-([0-9]{2})$ ]]

outer() { inner "first arg" "second arg"; }
inner() {
    BASHCAP -BCV:greeting -BCV:items -BCV:conf -BCV:attempts -BCV:nonexistent \
            -BCS:"two frames deep, four typed variables, one missing"
}
outer

# The CPS form: snapshot, then run the continuation and return its status.
step() { echo "   [continuation ran with: $*]"; }
BASHCAP__CTX__phase=work
WITH_BASHCAP -BCV:conf -BCS:"about to run step" step one two

# A subshell is its own shell on the wire, with its own provenance.
( BASHCAP -BCS:"from inside a subshell" )

# So is a child process.
bash "$(dirname "${BASH_SOURCE[0]}")/child.bash"
