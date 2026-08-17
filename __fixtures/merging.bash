#!/usr/bin/env bash
# A script that drives its own session and keeps the merge in an array.
#
#     merging.bash <the server's command line…>
#
# Everything here is what a shipped client writes, and nothing is vendored:
# the coproc starts the server and reads nothing back; the script probes the
# workspace it named until the session is up, sources the laid definitions
# and initiates its own channel — from then on the session's words speak, in
# this shell and in every subshell it makes.
set -euo pipefail

# The array is this script's, and so is its name. The server is told which one
# to write into, on the command line the client builds.
declare -a heard=()
declare -- workspace="$(mktemp -d)"
coproc SERVER { "$@" serve --at "$workspace" --into heard; }
until [[ -p "$workspace/join" ]]; do sleep 0.01; done
source "$workspace/prelude.bash"
source "$workspace/rig.bash"
BC_JOIN MERGE "$workspace"

# Each entry is `<shell> <µs into the session> <µs in flight> <words>`, and the
# words are a bash array literal — one level to unpack, and the boundaries the
# sender wrote come back intact.
report() {
    declare entry shell since travelled rest

    printf '%s entries\n' "${#heard[@]}"
    for entry in "${heard[@]}"; do
        read -r shell since travelled rest <<<"$entry"
        declare -a words="$rest"

        printf '  shell %s said %s words: %s\n' "$shell" "${#words[@]}" "${words[*]}"
        printf '%s\n' "$entry" >&2
    done
}

STEP alpha
( STEP "beta from a subshell" )
STEP gamma

# The answer replaces the array with the whole merge, so it grows as the
# session does rather than being appended to from two places.
declare -- BC_ASK__ARG_LABEL=MERGE
declare -a BC_ASK__ARGS=(MERGE)
BC_ASK
report

STEP delta
BC_ASK
report

# Saying no is a command that returns non-zero, like any other answer.
declare -a BC_ASK__ARGS=(MERGE nonsense)
BC_ASK || echo "unknown question: $?"

# The session is this script's to end: let go of the handle coproc left it,
# and wait for what it started. The workspace was this script's to name, so
# it is this script's to remove.
declare -- handle="${SERVER[1]}"
exec {handle}>&-
wait "$SERVER_PID"
echo "server exited $?"
rm -rf "$workspace"
