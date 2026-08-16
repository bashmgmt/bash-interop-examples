#!/usr/bin/env bash
# A script that drives its own session and keeps the merge in an array.
#
#     merging.bash <the server's command line…>
#
# Everything here is what a shipped client writes. `BC_START` starts the server,
# takes the one command it prints, and runs it — from then on `BC_INSTR` is
# defined, in this shell and in every subshell it makes.
set -euo pipefail

source "${BASH_SOURCE[0]%/*}/vendor/joining.bash"

# The array is this script's, and so is its name. The server is told which one
# to write into, on the command line the client builds.
declare -a heard=()
__workspace="$(mktemp -d)"
BC_START "$@" serve --at "$__workspace" --into heard

# Each entry is `<shell> <µs into the session> <µs in flight> <words>`, and the
# words are a bash array literal — one level to unpack, and the boundaries the
# sender wrote come back intact.
report() {
    local entry shell since travelled rest

    printf '%s entries\n' "${#heard[@]}"
    for entry in "${heard[@]}"; do
        read -r shell since travelled rest <<<"$entry"
        local -a words="$rest"

        printf '  shell %s said %s words: %s\n' "$shell" "${#words[@]}" "${words[*]}"
        printf '%s\n' "$entry" >&2
    done
}

BC_INSTR MERGE say STEP alpha
( BC_INSTR MERGE say STEP "beta from a subshell" )
BC_INSTR MERGE say STEP gamma

# The answer replaces the array with the whole merge, so it grows as the
# session does rather than being appended to from two places.
BC_INSTR MERGE ask MERGE
report

BC_INSTR MERGE say STEP delta
BC_INSTR MERGE ask MERGE
report

# Saying no is a command that returns non-zero, like any other answer.
BC_INSTR MERGE ask MERGE nonsense || echo "unknown question: $?"

# The session is this script's to end: let go, and wait for what it started.
# The workspace was this script's to name, so it is this script's to remove.
BC_LEAVE
echo "server exited $?"
rm -rf "$__workspace"
