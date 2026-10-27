function __borg_osc
    printf '\e]%s\a' $argv[1]
end

function __borg_preexec --on-event fish_preexec
    __borg_osc "133;C"
end

function __borg_postexec --on-event fish_postexec
    __borg_osc "133;D;$status"
end

function __borg_prompt --on-event fish_prompt
    __borg_osc "7;file://$hostname"(string replace -a '%' '%25' -- $PWD)
    __borg_osc "133;A"
end

if functions -q fish_prompt
    functions -c fish_prompt __borg_user_prompt
else
    function __borg_user_prompt
        printf '%s> ' (prompt_pwd)
    end
end

function fish_prompt
    __borg_user_prompt
    printf '\e]133;B\a'
end

if test "$BORG_CONFIRM_EXIT" = 1
    set -e BORG_CONFIRM_EXIT
    set --global __borg_exit_pid $fish_pid
    function exit
        if test "$fish_pid" != "$__borg_exit_pid"
            builtin exit $argv
        end
        command sh "$BORG_INTEGRATION_DIR/confirm-exit-read.sh"
    end
end
