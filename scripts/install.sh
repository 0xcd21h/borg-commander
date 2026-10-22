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

# The logo: a block "BORG". On colour terminals it powers up like a broken screen: rows
# tear sideways and smear, glyphs corrupt, colours flare, and a scan beam passes, then it
# settles. The template uses only ASCII so any awk can slice it; glyphs are swapped in.
logo() {
    awk -v colors="${colors}" -v unicode="${unicode}" -v animate="${animate}" '
        function rgb(r, g, b) { return sprintf("\033[38;2;%d;%d;%dm", r, g, b) }
        function vivid(i) {
            if (colors >= 16777216) return rgb(v_r[i], v_g[i], v_b[i])
            if (colors >= 256) return sprintf("\033[38;5;%dm", v_256[i])
            return sprintf("\033[%dm", v_16[i])
        }
        function paint(column, row, tint,   x, r, g, b, u) {
            if (colors == 0) return ""
            if (tint == "beam") return colors >= 256 ? "\033[1;38;5;231m" : "\033[1;97m"
            if (tint != "") return vivid(tint)
            x = width > 1 ? (column - 1) / (width - 1) : 0
            if (colors >= 16777216) {
                if (x < 0.5) { u = x * 2; r = 40; g = int(255 - 35 * u); b = int(140 + 115 * u) }
                else { u = (x - 0.5) * 2; r = int(40 + 130 * u); g = int(220 - 130 * u); b = 255 }
                return rgb(r, int(g - 6 * row), b)
            }
            if (colors >= 256) { split("46 48 50 51 45 39 63 99", p, " "); return sprintf("\033[38;5;%dm", p[int(x * 7.999) + 1]) }
            return x < 0.33 ? "\033[92m" : x < 0.66 ? "\033[96m" : "\033[95m"
        }
        function pick() { return noise[int(rand() * count) + 1] }
        { row[++n] = $0; if (length($0) > width) width = length($0) }
        END {
            srand()
            split("255 190 0 255 150 255 64", v_r, " "); split("0 255 140 120 60 64 220", v_g, " ")
            split("170 0 255 0 255 110 255", v_b, " "); split("199 154 33 208 99 204 45", v_256, " ")
            split("95 92 94 93 95 91 96", v_16, " ")
            block = unicode == "true" ? "█" : "#"
            count = split(unicode == "true" ? "▓ ▒ ░ ▚ ▞ ▙ ▟ ▛ ▜ ▀ ▄ ▌ ▐ ■ ▪ ◢ ◣ ◤ ◥ ╳ ╱ ╲ ┼ ╪ ╫ ¦ ≡ ∆ ¤ Ξ Ψ Ж λ Ø § ∑ ∂ √ # %" : "% @ & $ / | = + : # ~ ^ * ? ! < >", noise, " ")
            split(unicode == "true" ? "░ ░ ▒ ▓" : ". : = #", smear, " ")
            frames = animate == "true" ? 28 : 1
            if (animate == "true") printf "\033[?25l"
            for (f = 0; f < frames; f++) {
                t = frames > 1 ? f / (frames - 1) : 1
                heat = (1 - t) * (1 - t)
                beam = int(t * (n + 3)) - 1
                if (f > 0) printf "\033[%dA", n
                for (r = 1; r <= n; r++) {
                    shift = rand() < 0.65 * heat ? int((rand() * 2 - 1) * 14 * heat) : 0
                    tint = shift == 0 ? "" : int(rand() * 7) + 1
                    if (frames > 1 && r == beam && t < 1) tint = "beam"
                    inverted = colors > 0 && rand() < 0.12 * heat
                    line = "  "
                    for (k = 0; k < shift; k++) line = line paint(1, r, tint) smear[int(k * 4 / shift) + 1]
                    for (c = 1; c <= length(row[r]); c++) {
                        if (shift < 0 && c <= -shift) continue
                        ch = substr(row[r], c, 1)
                        static = f < 3 ? 0.5 * heat : 0.05 * heat
                        if (ch == "#") glyph = rand() < 0.4 * heat ? pick() : block
                        else glyph = rand() < static ? pick() : " "
                        cell = rand() < 0.2 * heat ? int(rand() * 7) + 1 : tint
                        line = line paint(c, r, cell) glyph
                    }
                    for (k = shift; k < 0; k++) line = line paint(width, r, tint) smear[int((k - shift) * 4 / -shift) + 1]
                    if (inverted) line = "\033[7m" line
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
