#!/usr/bin/env bash
set -euo pipefail

output="${1:?usage: report-test-failures.sh <cargo test output>}"
limit="${2:-8000}"

awk -v limit="$limit" '
function flush() {
    if (name == "") return
    body = substr(body, 1, limit)
    gsub(/%/, "%25", body)
    gsub(/\r/, "%0D", body)
    gsub(/\n/, "%0A", body)
    printf "::error title=%s::%s\n", name, body
    name = ""
    body = ""
}
/^---- .* stdout ----$/ {
    flush()
    name = $2
    next
}
/^(failures:|---- )/ { flush() }
name != "" { body = body $0 "\n" }
END { flush() }
' "$output"
