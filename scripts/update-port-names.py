#!/usr/bin/env python3
import argparse
import csv
import io
import pathlib
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
TABLE = ROOT / "crates" / "borg-core" / "src" / "port_scan" / "services.tsv"
REGISTRY = "https://www.iana.org/assignments/service-names-port-numbers/service-names-port-numbers.csv"
USER_AGENT = "borg-commander port names (https://github.com/0xcd21h/borg-commander)"
TCP = "tcp"
WELL_KNOWN = range(1, 1024)
REGISTERED_COMMON = [
    1080, 1194, 1433, 1723, 1883, 2049, 2375, 2376, 3000, 3128, 3260, 3306, 3389, 5000, 5060, 5061,
    5222, 5269, 5357, 5432, 5672, 5900, 5985, 5986, 6379, 6443, 7000, 8000, 8008, 8080, 8443, 8883,
    8888, 9000, 9090, 9200, 11211, 27017,
]
COMMON = "1"
OTHER = "0"


def names(text: str) -> dict[int, str]:
    table = {}
    for row in csv.DictReader(io.StringIO(text)):
        number = row["Port Number"].strip()
        name = row["Service Name"].strip()
        if row["Transport Protocol"].strip() != TCP or not name or not number.isdigit():
            continue
        table.setdefault(int(number), name)
    return table


def main() -> int:
    parser = argparse.ArgumentParser(description="Writes Borg's TCP port names from the IANA service registry.")
    parser.add_argument(
        "registry", nargs="?", type=pathlib.Path, help="a downloaded service-names-port-numbers.csv; read from IANA without it"
    )
    arguments = parser.parse_args()
    if arguments.registry:
        text = arguments.registry.read_text(encoding="utf-8")
    else:
        request = urllib.request.Request(REGISTRY, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request) as response:
            text = response.read().decode("utf-8")
    table = names(text)
    missing = [port for port in REGISTERED_COMMON if port not in table]
    if missing:
        print(f"not in the registry: {missing}", file=sys.stderr)
        return 1
    common = {port for port in table if port in WELL_KNOWN} | set(REGISTERED_COMMON)
    lines = [f"{port}\t{name}\t{COMMON if port in common else OTHER}\n" for port, name in sorted(table.items())]
    TABLE.parent.mkdir(parents=True, exist_ok=True)
    TABLE.write_text("".join(lines), encoding="utf-8")
    print(f"wrote {len(lines)} port names, {len(common)} of them common, to {TABLE.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
