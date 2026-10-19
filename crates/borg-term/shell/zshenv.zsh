if [[ -n "${BORG_USER_ZDOTDIR+set}" ]]; then
    ZDOTDIR="$BORG_USER_ZDOTDIR"
    unset BORG_USER_ZDOTDIR
else
    unset ZDOTDIR
fi

if [[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]]; then
    source "${ZDOTDIR:-$HOME}/.zshenv"
fi

if [[ -o interactive && -n "$BORG_INTEGRATION_DIR" ]]; then
    source "$BORG_INTEGRATION_DIR/integration.zsh"
fi
