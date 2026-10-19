autoload -Uz add-zsh-hook add-zle-hook-widget
setopt HIST_IGNORE_SPACE

__borg_osc() {
    builtin printf '\e]%s\a' "$1"
}

__borg_precmd() {
    local last=$?
    __borg_osc "133;D;$last"
    __borg_osc "7;file://${HOST}${PWD//\%/%25}"
    __borg_osc "133;A"
    return $last
}

__borg_preexec() {
    __borg_osc "133;C"
}

__borg_line_init() {
    __borg_osc "133;B"
}

__borg_install() {
    add-zsh-hook -d precmd __borg_install
    precmd_functions=(__borg_precmd $precmd_functions)
    add-zsh-hook preexec __borg_preexec
    add-zle-hook-widget line-init __borg_line_init
    __borg_osc "7;file://${HOST}${PWD//\%/%25}"
    __borg_osc "133;A"
}

add-zsh-hook precmd __borg_install
