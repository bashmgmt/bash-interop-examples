# Time the call this wraps, and everything measured inside it.
#
#     BASHPROF_TIME_CPS <label> <command> [args…]
#
# Nothing is timed here. The wire stamps every message with the sending
# shell's $EPOCHREALTIME, so a span is the interval between two of them and
# this only has to say where each one falls.
#
# Each call names itself and hands that name to everything it runs, so the
# tree travels on the wire rather than being inferred from it. `__BP_inside`
# is declared in this frame, which is what makes the hand-off work: dynamic
# scoping puts it where everything `"$@"` reaches will read it, and a fork
# inherits it along with the rest of the shell.
#
# The name is $BASHPID and a count only that shell advances. $BASHPID is the
# one value that differs in every process; the count is what keeps two calls
# in one shell apart. $RANDOM can do neither job on its own — a subshell
# inherits the generator's state, so two forks made from one point draw the
# same numbers, however many of them are drawn. Bash 5 happens to reseed in a
# subshell, but nothing documents that and bash 4 does not.
#
# The measured call is run unguarded. A `||` list would suppress errexit for
# everything it reaches, so a measured function would run past its own first
# failure and the run's status would change — a profiler that alters whether
# the subject aborts is not measuring the subject. Under `set -e` a failure
# therefore exits at `"$@"`, no END is sent, and the call stays open: the
# shell died inside it, and that is the reading.
#
# `$?` is read as the first command after the call, which is the only place it
# survives, and returned after the END message has clobbered it.
BASHPROF_TIME_CPS() {
    local __BP_label="${1-}"
    shift || __BC_THROW

    # Not local: one count per shell, spanning every call that shell makes. A
    # fork inherits the count and advances its own copy under its own pid.
    __BP_made=$(( __BP_made + 1 ))
    local __BP_id="$BASHPID.$__BP_made"

    # Two frames are the instrument's: __bc_stack's own and this one.
    local -a __BP_begin=(BEGIN id "$__BP_id" inside "${__BP_inside-}" label "$__BP_label")
    __bc_stack __BP_begin 2

    BC_INSTR say TIME_CPS "${__BP_begin[@]}" || __BC_BAIL

    local __BP_inside="$__BP_id"

    "$@"
    local __BP_rc=$?

    BC_INSTR say TIME_CPS END id "$__BP_id" || __BC_BAIL

    return "$__BP_rc"
}
