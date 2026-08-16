#!/usr/bin/env bash
# A child process, and the one place this demo traces call arguments.
# `extdebug` is bash's own switch for recording them; bashcap never sets it
# — from BASH_ENV that would mean "start the debugger" — so a script that
# wants them turns it on itself, as its own first statement.
shopt -s extdebug

source "$(dirname "${BASH_SOURCE[0]}")/bashcap.bash"
declare -F __bc_capture >/dev/null || __bc_capture() { :; }

child_work() {
    declare -a payload=(x y z)
    BASHCAP -BCV:payload -BCS:"child process, own pid and SHLVL"
}
child_work "a first argument" "a second"
