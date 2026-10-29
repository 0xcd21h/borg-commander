mode=$(stty -g < /dev/tty) || exit 1
trap 'stty "$mode" < /dev/tty' 0
stty -echo < /dev/tty || exit 1
token="$$"
printf '\033]6633;confirm-exit;%s\007' "$token" > /dev/tty || exit 1
while IFS= read -r reply < /dev/tty; do
    [ "$reply" = "$token:no" ] && exit 0
    [ "$reply" = "$token:yes" ] && exit 2
done
exit 1
