#!/bin/sh
# Installs borg-relay, the relay borgs meet through for borg cloud, as a systemd service.
#
#   curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
#
# The domain must point at this machine, and port 443 must be free: the relay gets its TLS
# certificate from Let's Encrypt by itself. The email, optional, is where Let's Encrypt
# writes about the certificate. Run it again to update the relay.
set -eu

repo="0xcd21h/borg-commander"
domain="${1:-}"
email="${2:-}"
user="borg-relay"
data="/var/lib/borg-relay"
bin="/usr/local/bin/borg-relay"
unit="/etc/systemd/system/borg-relay.service"

fail() {
    printf 'borg-relay installer: %s\n' "$1" >&2
    exit 1
}

[ -n "${domain}" ] || fail "give the relay's domain: sh -s -- relay.example.com [email]"
[ "$(id -u)" = 0 ] || fail "run it as root, for example with sudo"
command -v systemctl >/dev/null 2>&1 || fail "it needs systemd"
command -v curl >/dev/null 2>&1 || fail "it needs curl"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) target="x86_64-unknown-linux-musl" ;;
    Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
    *) fail "no prebuilt borg-relay for $(uname -s) $(uname -m)" ;;
esac
if command -v sha256sum >/dev/null 2>&1; then verify="sha256sum -c"; else verify="shasum -a 256 -c"; fi

tag="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
[ -n "${tag}" ] || fail "could not find the latest release"
name="borg-relay-${tag#v}-${target}"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

echo "Downloading ${name}…"
curl -fsSL -o "${work}/${name}.tar.gz" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz"
curl -fsSL -o "${work}/${name}.tar.gz.sha256" "https://github.com/${repo}/releases/download/${tag}/${name}.tar.gz.sha256"
(cd "${work}" && ${verify} "${name}.tar.gz.sha256" >/dev/null) || fail "the download does not match its checksum"
tar -xzf "${work}/${name}.tar.gz" -C "${work}"
install -m 0755 "${work}/${name}/borg-relay" "${bin}"

if ! id "${user}" >/dev/null 2>&1; then
    useradd --system --home-dir "${data}" --shell /usr/sbin/nologin "${user}"
fi
mkdir -p "${data}"
chown "${user}" "${data}"
chmod 0700 "${data}"

contact=""
if [ -n "${email}" ]; then contact=" --email ${email}"; fi
cat >"${unit}" <<UNIT
[Unit]
Description=borg-relay, where borgs meet for borg cloud
After=network-online.target
Wants=network-online.target

[Service]
User=${user}
ExecStart=${bin} --domain ${domain}${contact} --cache ${data}
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ReadWritePaths=${data}
Restart=always
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable borg-relay >/dev/null 2>&1
systemctl restart borg-relay
sleep 1
systemctl is-active --quiet borg-relay || fail "the service did not start; see journalctl -u borg-relay"

echo
echo "borg-relay ${tag#v} is running for wss://${domain}."
echo "It gets its certificate on the first connection; journalctl -u borg-relay -f shows it."
echo "Borgs use it with this line in their config:  relay = \"wss://${domain}\""
