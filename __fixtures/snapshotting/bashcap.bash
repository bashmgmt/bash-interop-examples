# The words bashcap gives a script. This file is what the tool injects and what
# a client vendors, so each word has one definition and only its effect exists
# twice:
#
#     source lib/bashcap.bash
#     declare -F __bc_capture >/dev/null || __bc_capture() { :; }
#
# Nothing here names the protocol. What a call does when the tool is present
# lives behind __bc_capture, whose argument is how many leading frames of the
# walk belong to the instrument — a number each word knows about itself.

# Leading -BCV:/-BCS: flags off the front, into two arrays of the caller's.
# Every consumed word lands in one of them, so their combined length is how far
# the caller has to shift.
__bc_take_flags() {
    local -n __bc_v="$1" __bc_n="$2"
    shift 2

    while (( $# > 0 )); do
        case "$1" in
            -BCV:*) __bc_v+=("${1#-BCV:}"); shift ;;
            -BCS:*) __bc_n+=("${1#-BCS:}"); shift ;;
            *) break ;;
        esac
    done
}

BASHCAP() {
    local -a __bc_vars=() __bc_notes=()
    __bc_take_flags __bc_vars __bc_notes "$@"

    __bc_capture 3
}

WITH_BASHCAP() {
    local -a __bc_vars=() __bc_notes=()
    __bc_take_flags __bc_vars __bc_notes "$@"
    shift $(( ${#__bc_vars[@]} + ${#__bc_notes[@]} ))

    # A snapshot that could not be taken is a broken run, so it is forwarded
    # rather than stepped over.
    __bc_capture 2 || return $?

    "$@"
}
