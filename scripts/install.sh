#!/bin/sh
# Installs the latest borg release from https://github.com/0xcd21h/borg-commander
#
#   curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
#
# BORG_INSTALL_DIR and BORG_DATA_DIR choose where the binary and fonts go. NO_COLOR turns
# colours off, FORCE_COLOR turns them on, and BORG_NO_ANIMATION skips the animated logo.
set -eu

repo="0xcd21h/borg-commander"
dest="${BORG_INSTALL_DIR:-${HOME}/.local/bin}"
work=""

# Terminal: colour depth, Unicode, and whether the logo may animate.
tty=false
if [ -t 1 ]; then tty=true; fi
colors=0
if [ -z "${NO_COLOR:-}" ] && { [ -n "${FORCE_COLOR:-}" ] || { $tty && [ "${TERM:-dumb}" != dumb ]; }; }; then
    case "${COLORTERM:-}" in
        truecolor | 24bit) colors=16777216 ;;
        *)
            case "${TERM:-}" in
                *256color* | *kitty* | *ghostty*) colors=256 ;;
                *) colors="$(tput colors 2>/dev/null || echo 8)" ;;
            esac
            ;;
    esac
    case "${colors}" in '' | *[!0-9]*) colors=8 ;; esac
    if [ "${colors}" -lt 8 ]; then colors=8; fi
fi
unicode=false
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in *[Uu][Tt][Ff]-8* | *[Uu][Tt][Ff]8*) unicode=true ;; esac
animate=false
if $tty && [ "${colors}" -gt 0 ] && [ -z "${CI:-}${BORG_NO_ANIMATION:-}" ] && sleep 0.01 2>/dev/null; then
    animate=true
fi

esc="$(printf '\033')"
if [ "${colors}" -gt 0 ]; then
    reset="${esc}[0m" bold="${esc}[1m" dim="${esc}[2m"
    if [ "${colors}" -ge 256 ]; then
        accent="${esc}[38;5;49m" good="${esc}[38;5;84m" warn="${esc}[38;5;221m" bad="${esc}[38;5;203m"
    else
        accent="${esc}[96m" good="${esc}[92m" warn="${esc}[93m" bad="${esc}[91m"
    fi
else
    reset="" bold="" dim="" accent="" good="" warn="" bad=""
fi
if $unicode; then
    done_mark="✓" fail_mark="✗" wait_mark="◌" arrow="›" rule="·"
else
    done_mark="ok" fail_mark="x" wait_mark=".." arrow=">" rule="-"
fi

cleanup() {
    if [ -n "${work}" ]; then rm -rf "${work}"; fi
    if $animate; then printf '%s[?25h' "${esc}"; fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

# The logo: a block "BORG". On colour terminals it powers up like an old screen, with
# tearing rows, split colours, and a scan beam, then settles. The template uses only ASCII
# so any awk can slice it; glyphs are swapped in on output.
logo() {
    awk -v colors="${colors}" -v unicode="${unicode}" -v animate="${animate}" '
        function paint(column, row, tint,   x, r, g, b) {
            if (colors == 0) return ""
            if (tint == "beam") return colors >= 256 ? "\033[1;38;5;231m" : "\033[1;97m"
            if (tint == "red") return colors >= 16777216 ? "\033[38;2;255;64;110m" : colors >= 256 ? "\033[38;5;204m" : "\033[91m"
            if (tint == "cyan") return colors >= 16777216 ? "\033[38;2;64;220;255m" : colors >= 256 ? "\033[38;5;45m" : "\033[96m"
            x = width > 1 ? (column - 1) / (width - 1) : 0
            if (colors >= 16777216) {
                r = int(40 + 20 * x); g = int(255 - 35 * x - 6 * row); b = int(130 + 125 * x)
                return sprintf("\033[38;2;%d;%d;%dm", r, g, b)
            }
            if (colors >= 256) { split("48 48 49 50 50 51", p, " "); return sprintf("\033[38;5;%dm", p[int(x * 5.999) + 1]) }
            return x < 0.5 ? "\033[92m" : "\033[96m"
        }
        { row[++n] = $0; if (length($0) > width) width = length($0) }
        END {
            srand()
            block = unicode == "true" ? "█" : "#"
            count = split(unicode == "true" ? "▓ ▒ ░ ▚ ▞ ▀ ▄ ▌ ▐" : "% @ & $ / | = + :", noise, " ")
            frames = animate == "true" ? 24 : 1
            if (animate == "true") printf "\033[?25l"
            for (f = 0; f < frames; f++) {
                t = frames > 1 ? f / (frames - 1) : 1
                heat = (1 - t) * (1 - t)
                beam = int(t * (n + 3)) - 1
                if (f > 0) printf "\033[%dA", n
                for (r = 1; r <= n; r++) {
                    shift = rand() < 0.6 * heat ? int((rand() * 2 - 1) * 8 * heat) : 0
                    tint = shift == 0 ? "" : rand() < 0.5 ? "red" : "cyan"
                    if (frames > 1 && r == beam && t < 1) tint = "beam"
                    line = "  "
                    if (shift > 0) line = line sprintf("%" shift "s", "")
                    for (c = 1; c <= length(row[r]); c++) {
                        if (shift < 0 && c <= -shift) continue
                        ch = substr(row[r], c, 1)
                        static = f < 3 ? 0.45 * heat : 0.03 * heat
                        if (ch == "#") glyph = rand() < 0.4 * heat ? noise[int(rand() * count) + 1] : block
                        else glyph = rand() < static ? noise[int(rand() * count) + 1] : " "
                        line = line paint(c, r, tint) glyph
                    }
                    printf "%s%s%s\n", line, (colors > 0 ? "\033[0m" : ""), (animate == "true" ? "\033[K" : "")
                }
                if (animate == "true" && f < frames - 1) { fflush(); system("sleep 0.04") }
            }
            if (animate == "true") printf "\033[?25h"
        }' <<'LOGO'
######   ######  ######   ######
##   ## ##    ## ##   ## ##
######  ##    ## ######  ##   ###
##   ## ##    ## ##   ## ##    ##
######   ######  ##   ##  ######
LOGO
    printf '  %sc o m m a n d e r%s  %s%s  installer%s\n\n' "${accent}" "${reset}" "${dim}" "${rule}" "${reset}"
}

field() { printf '  %s%-9s%s %s\n' "${dim}" "$1" "${reset}" "$2"; }
waiting() {
    if $tty; then printf '  %s%s%s %s' "${dim}" "${wait_mark}" "${reset}" "$1"; fi
}
finished() {
    if $tty; then printf '\r%s[K' "${esc}"; fi
    printf '  %s%s%s %-10s %s\n' "${good}" "${done_mark}" "${reset}" "$1" "$2"
}
fail() {
    if $tty; then printf '\r%s[K' "${esc}"; fi
    printf '  %s%s %s%s\n' "${bad}" "${fail_mark}" "$1" "${reset}" >&2
    if [ $# -gt 1 ]; then printf '    %s\n' "$2" >&2; fi
    exit 1
}
need() {
    command -v "$1" >/dev/null 2>&1 || fail "borg's installer needs $1" "Install $1 with your package manager, then run the installer again."
}

logo
need curl
need tar

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target="aarch64-apple-darwin" ;;
    Darwin-x86_64) target="x86_64-apple-darwin" ;;
    Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
    Linux-x86_64) target="x86_64-unknown-linux-musl" ;;
    *) fail "No prebuilt borg for $(uname -s) $(uname -m)" "Build it instead: cargo install --locked --git https://github.com/${repo} borg-app" ;;
esac

# Linux ships sha256sum and macOS ships shasum; both read the same checksum file.
if command -v sha256sum >/dev/null 2>&1; then
    verify="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
    verify="shasum -a 256 -c"
else
    fail "Cannot verify the download" "Install sha256sum (coreutils) or shasum (perl), then run the installer again."
fi

waiting "Finding the latest release…"
tag="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)" || true
if [ -z "${tag}" ]; then
    fail "Could not find the latest release" "Check your connection, or download borg from https://github.com/${repo}/releases"
fi
if $tty; then printf '\r%s[K' "${esc}"; fi
version="${tag#v}"
name="borg-${version}-${target}"
field "Release" "${bold}borg ${version}${reset}"
field "Platform" "${target}"
printf '\n'

work="$(mktemp -d)"
waiting "Downloading ${name}.tar.gz…"
curl -fsSL -o "${work}/${name}.tar.gz" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz" ||
    fail "Download failed" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz"
curl -fsSL -o "${work}/${name}.tar.gz.sha256" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz.sha256" ||
    fail "Checksum download failed"
size="$(wc -c <"${work}/${name}.tar.gz" | awk '{ printf "%.1f MB", $1 / 1048576 }')"
finished "Downloaded" "${name}.tar.gz ${dim}${size}${reset}"

waiting "Verifying…"
(cd "${work}" && ${verify} "${name}.tar.gz.sha256" >/dev/null 2>&1) ||
    fail "The download does not match its checksum" "Nothing was installed. Try again, or report it at https://github.com/${repo}/issues"
finished "Verified" "SHA-256"

waiting "Installing…"
tar -xzf "${work}/${name}.tar.gz" -C "${work}"
mkdir -p "${dest}"
install -m 0755 "${work}/${name}/borg" "${dest}/borg"
finished "Installed" "${dest}/borg"
if [ -d "${work}/${name}/fonts" ]; then
    data="${BORG_DATA_DIR:-${XDG_DATA_HOME:-${HOME}/.local/share}/borg-commander}"
    mkdir -p "${data}"
    cp -R "${work}/${name}/fonts" "${data}/"
    finished "Fonts" "${data}/fonts ${dim}${rule} optional, select Borg Mono in your terminal${reset}"
fi

printf '\n  %s%sborg %s is ready.%s\n' "${good}" "${bold}" "${version}" "${reset}"
case ":${PATH}:" in
    *":${dest}:"*)
        printf '  Run %sborg%s in any folder. Update later with %sborg update%s.\n\n' "${accent}" "${reset}" "${accent}" "${reset}"
        ;;
    *)
        shown="${dest}"
        case "${dest}" in "${HOME}"/*) shown="\$HOME${dest#"${HOME}"}" ;; esac
        # The shell that ran the installer, when SHELL is not exported (docker exec, su).
        shell="${SHELL:-}"
        if [ -z "${shell}" ] && [ -r "/proc/${PPID}/comm" ]; then shell="$(cat "/proc/${PPID}/comm")"; fi
        if [ -z "${shell}" ]; then shell="$(ps -o comm= -p "${PPID}" 2>/dev/null || true)"; fi
        case "$(basename "${shell:-sh}" | sed 's/^-//')" in
            fish) add="fish_add_path ${shown}" ;;
            zsh) add="echo 'export PATH=\"${shown}:\$PATH\"' >> ~/.zshrc" ;;
            bash)
                rc="~/.bashrc"
                if [ "$(uname -s)" = Darwin ]; then rc="~/.bash_profile"; fi
                add="echo 'export PATH=\"${shown}:\$PATH\"' >> ${rc}"
                ;;
            *) add="echo 'export PATH=\"${shown}:\$PATH\"' >> ~/.profile" ;;
        esac
        printf '  %s%s%s %s is not on your PATH yet. Add it:\n\n' "${warn}" "${arrow}" "${reset}" "${dest}"
        printf '      %s%s%s\n\n' "${accent}" "${add}" "${reset}"
        printf '  Then open a new terminal and run %sborg%s.\n\n' "${accent}" "${reset}"
        ;;
esac
