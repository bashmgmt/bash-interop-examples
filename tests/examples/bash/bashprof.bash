# Time the call this wraps, and everything measured inside it.
#
#     BASHPROF_TIME_CPS <label> <command> [args…]
#
# Nothing is timed here. The wire stamps every message with the sending
# shell's $EPOCHREALTIME, so a span is the interval between two of them and
# this only has to say where each one falls. Nothing is inferred either: each
# call is given a name, hands that name to everything it runs, and reports the
# name it was handed, so the tree travels on the wire.
#
# What a call carries is three declarations, and each is a layer of its own —
# as an alias rather than a function. A function would be a frame: one the
# walk has to skip, one every call measured below it carries in its own
# payload, and one more call per measurement. An alias is the same text in the
# same frame, so the layers separate for a reader and cost nothing.
#
# Every one of them declares in the frame of the word the subject called,
# which is what puts it where the rest of that word and everything it runs
# will read it — and what a fork inherits. See KB/mb_resolver/bash/scoping.md.

# The call site's stack, as `__BP_stack`.
#
# 2 is __bc_stack's own frame and the frame this expands into, so the walk
# points at the subject rather than at us. It holds wherever this is used, as
# long as it is used in the body of the word the subject calls.
#
# `$__BASHPROF_STACK_SHIFT` adds to it, for code that wrapped that word in a
# word of its own and wants the walk to reach past that too. A wrapper
# declares it `local` in the frame it wraps from, so the value dies with that
# frame:
#
#     measure_step() {
#         local __BASHPROF_STACK_SHIFT=1
#         BASHPROF_TIME_CPS "$@"
#     }
#
# Read through `:-0` because an unset name inside `(( ))` is an error under
# `set -u` while an empty one is zero — so a subject that never heard of it
# pays one parameter expansion and adds nothing.
alias __BASHPROF_TAKE_STACK='
    local -a __BP_stack=()
    __bc_stack __BP_stack $(( 2 + ${__BASHPROF_STACK_SHIFT:-0} ))'

# This shell's next call, named, as `__BP_id`.
#
# The name is $BASHPID and a count only this shell advances. $BASHPID is the
# one value that differs in every process; the count is what keeps two calls
# in one shell apart. $RANDOM can do neither job on its own — a subshell
# inherits the generator's state, so two forks made from one point draw the
# same numbers, however many of them are drawn. Bash 5 happens to reseed in a
# subshell, but nothing documents that and bash 4 does not.
#
# `__BP_made` is not local: one count per shell, spanning every call that
# shell makes. A fork inherits the count and advances its own copy under its
# own pid.
alias __BASHPROF_TAKE_NAME='
    __BP_made=$(( __BP_made + 1 ))
    local __BP_id="$BASHPID.$__BP_made"'

# What the calls made inside this one inherit.
#
# `__BP_inside` is read for the payload before this, while the name in scope
# is still the caller's, and declared here — so a call reports the call it was
# made inside of, and hands its own name down in turn. A shift a caller asked
# for was for reaching this call site, not the ones inside it, so it stops
# here as well.
alias __BASHPROF_HAND_ON='
    local __BP_inside="$__BP_id"
    declare -- __BASHPROF_STACK_SHIFT='

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

    __BASHPROF_TAKE_STACK
    __BASHPROF_TAKE_NAME

    BC_INSTR say TIME_CPS BEGIN id "$__BP_id" inside "${__BP_inside-}" \
        label "$__BP_label" "${__BP_stack[@]}" || __BC_BAIL

    __BASHPROF_HAND_ON

    "$@"
    local __BP_rc=$?

    BC_INSTR say TIME_CPS END id "$__BP_id" || __BC_BAIL

    return "$__BP_rc"
}
