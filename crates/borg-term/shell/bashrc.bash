if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

HISTCONTROL="ignorespace${HISTCONTROL:+:$HISTCONTROL}"

__borg_osc() {
    printf '\033]%s\007' "$1"
}

__borg_status() {
    __borg_last=$?
    __borg_osc "133;D;$__borg_last"
}

__borg_mark() {
    __borg_osc "7;file://${HOSTNAME}${PWD//%/%25}"
    __borg_osc "133;A"
    case "$PS1" in
        *'133;B'*) ;;
        *) PS1="${PS1}\[\033]133;B\007\]" ;;
    esac
    return $__borg_last
}

PROMPT_COMMAND=$'__borg_status\n'"${PROMPT_COMMAND}"$'\n__borg_mark'
PS0="${PS0}"$'\033]133;C\007'
. "$BORG_INTEGRATION_DIR/confirm-exit.sh"
