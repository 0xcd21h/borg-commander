if [ "${BORG_CONFIRM_EXIT-}" = 1 ]; then
    unset BORG_CONFIRM_EXIT
    __borg_exit_pid="${BASHPID:-$$}"
    __borg_exit_subshell="${BASH_SUBSHELL:-${ZSH_SUBSHELL:-0}}"

    exit() {
        if [ "${BASHPID:-$$}" != "$__borg_exit_pid" ] || [ "${BASH_SUBSHELL:-${ZSH_SUBSHELL:-0}}" != "$__borg_exit_subshell" ]; then
            builtin exit "$@"
        fi
        command sh "$BORG_INTEGRATION_DIR/confirm-exit-read.sh"
        [ "$?" = 2 ] && builtin exit "$@"
        return 0
    }
fi
