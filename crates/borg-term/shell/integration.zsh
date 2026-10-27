autoload -Uz add-zsh-hook add-zle-hook-widget
setopt HIST_IGNORE_SPACE

# Borg changes the shell's folder by sending this key after writing the folder to
# $BORG_CD_FILE. It must match SILENT_CD_KEY in integration.rs.
__borg_cd_key=$'\e[2468~'

__borg_osc() {
    builtin printf '\e]%s\a' "$1"
}

__borg_announce() {
    __borg_osc "7;file://${HOST}${PWD//\%/%25}"
    __borg_osc "133;A"
    __borg_osc "6633;silent-cd"
}

__borg_precmd() {
    local last=$?
    __borg_osc "133;D;$last"
    __borg_announce
    return $last
}

__borg_preexec() {
    __borg_osc "133;C"
}

__borg_line_init() {
    __borg_osc "133;B"
}

# Changes folder without a command line: nothing is echoed or added to history, and
# the prompt is redrawn in place. Prompt hooks run again so themes show the new folder.
__borg_cd() {
    local target hook
    [[ -r $BORG_CD_FILE ]] || return 0
    target=$(<$BORG_CD_FILE)
    [[ -n $target ]] || return 0
    builtin cd -- "$target" 2>/dev/null || return 0
    __borg_osc "7;file://${HOST}${PWD//\%/%25}"
    for hook in $precmd_functions; do
        [[ $hook == __borg_precmd ]] || "$hook"
    done
    zle reset-prompt
}

__borg_install() {
    local keymap
    add-zsh-hook -d precmd __borg_install
    precmd_functions=(__borg_precmd $precmd_functions)
    add-zsh-hook preexec __borg_preexec
    add-zle-hook-widget line-init __borg_line_init
    zle -N __borg_cd
    for keymap in emacs viins vicmd; do
        bindkey -M $keymap "$__borg_cd_key" __borg_cd
    done
    __borg_announce
}

add-zsh-hook precmd __borg_install
source "$BORG_INTEGRATION_DIR/confirm-exit.sh"
