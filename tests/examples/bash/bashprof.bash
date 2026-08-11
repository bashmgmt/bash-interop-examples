# Time the call this wraps, and everything measured inside it.
#
#     BASHPROF_TIME_CPS <label> <command> [args…]
#
# Nothing is timed here. The wire stamps every message with the sending
# shell's $EPOCHREALTIME, so a span is the interval between two of them and
# this only has to say where each one falls.
#
# The measured call is run unguarded. A `||` list would suppress errexit for
# everything it reaches, so a measured function would run past its own first
# failure and the run's status would change — a profiler that alters whether
# the subject aborts is not measuring the subject. Under `set -e` a failure
# therefore exits at `"$@"`, no END is sent, and the span stays open: the
# shell died inside it, and that is the reading.
#
# `$?` is read as the first command after the call, which is the only place it
# survives, and returned after the END message has clobbered it.
BASHPROF_TIME_CPS() {
    local __BP_label="${1-}"
    shift || __BC_THROW

    # Two frames are the instrument's: __bc_stack's own and this one.
    local -a __BP_begin=(BEGIN label "$__BP_label")
    __bc_stack __BP_begin 2

    BC_INSTR say TIME_CPS "${__BP_begin[@]}" || __BC_BAIL

    "$@"
    local __BP_rc=$?

    BC_INSTR say TIME_CPS END || __BC_BAIL

    return "$__BP_rc"
}
