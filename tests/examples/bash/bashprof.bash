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
# The public word is a spine of `with_` steps and a last one that does the
# work. Each step declares what it contributes in its own frame and calls the
# next, so everything downstream reads it through dynamic scoping and every
# fork inherits it. The order is not free: the stack has to be taken at the
# head, where only this spine stands between the walk and the call site.
BASHPROF_TIME_CPS() {
    __BASHPROF_WITH_STACK 2 __BASHPROF_WITH_UNIQUE_ID __BASHPROF_TIME_NAMED "$@"
}

# Take the call site's stack, and run the continuation with it in scope as
# `__BP_stack`.
#
#     __BASHPROF_WITH_STACK <spine> <continuation> [args…]
#
# `<spine>` is how many frames stand between here and the call site — 2 where
# this is the first step a public word reaches, being that word's frame and
# this one. `__bc_stack`'s own is added here, because whose frame that is, is
# this function's business and not its caller's.
#
# `$__BASHPROF_STACK_SHIFT` adds to it, for code that wrapped the public word
# in a word of its own and wants the walk to point past that too. A wrapper
# declares it `local` in the frame it is wrapping from, so the value dies with
# that frame:
#
#     measure_step() {
#         local __BASHPROF_STACK_SHIFT=1
#         BASHPROF_TIME_CPS "$@"
#     }
#
# It is read through `:-0` because an unset name inside `(( ))` is an error
# under `set -u` while an empty one is zero — so a subject that never heard of
# it pays one parameter expansion and adds nothing.
__BASHPROF_WITH_STACK() {
    local __BP_spine="${1-}"
    shift || __BC_THROW

    local -a __BP_stack=()
    __bc_stack __BP_stack $(( 1 + __BP_spine + ${__BASHPROF_STACK_SHIFT:-0} ))

    "$@"
}

# Name this shell's next call, and run the continuation with that name in
# scope as `__BP_id`.
#
# The name is $BASHPID and a count only this shell advances. $BASHPID is the
# one value that differs in every process; the count is what keeps two calls
# in one shell apart. $RANDOM can do neither job on its own — a subshell
# inherits the generator's state, so two forks made from one point draw the
# same numbers, however many of them are drawn. Bash 5 happens to reseed in a
# subshell, but nothing documents that and bash 4 does not.
__BASHPROF_WITH_UNIQUE_ID() {
    # Not local: one count per shell, spanning every call that shell makes. A
    # fork inherits the count and advances its own copy under its own pid.
    __BP_made=$(( __BP_made + 1 ))

    local __BP_id="$BASHPID.$__BP_made"

    "$@"
}

# Measure the call, under the name and the walk already in scope.
#
# `__BP_inside` is read for the payload while the name in scope is still the
# caller's, and declared after — so a call reports the call it was made inside
# of, and hands its own name down in turn.
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
__BASHPROF_TIME_NAMED() {
    local __BP_label="${1-}"
    shift || __BC_THROW

    local -a __BP_begin=(
        BEGIN id "$__BP_id" inside "${__BP_inside-}" label "$__BP_label" "${__BP_stack[@]}"
    )

    BC_INSTR say TIME_CPS "${__BP_begin[@]}" || __BC_BAIL

    local __BP_inside="$__BP_id"

    # A shift a caller asked for was for reaching this call site, not for the
    # ones inside it.
    declare -- __BASHPROF_STACK_SHIFT=

    "$@"
    local __BP_rc=$?

    BC_INSTR say TIME_CPS END id "$__BP_id" || __BC_BAIL

    return "$__BP_rc"
}
