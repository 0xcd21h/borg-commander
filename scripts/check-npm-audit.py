#!/usr/bin/env python3
import argparse
import json
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".config" / "npm-audit.toml"
WEBSITE = ROOT / "website"
ADVISORY_URL = "https://github.com/advisories/"


def ignored(config):
    entries = config.get("ignore", [])
    for entry in entries:
        if not entry.get("id") or not entry.get("reason"):
            sys.exit(f"{CONFIG.relative_to(ROOT)}: every ignore entry needs an id and a reason: {entry}")
    return {entry["id"] for entry in entries}


def audit():
    result = subprocess.run(["npm", "audit", "--json"], cwd=WEBSITE, capture_output=True, text=True)
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        sys.exit(f"npm audit printed no report:\n{result.stderr}")
    if "vulnerabilities" not in report:
        sys.exit(f"npm audit failed: {report.get('error', report)}")
    return report["vulnerabilities"]


def advisories(vulnerabilities):
    """The advisories npm audit found, by id, with their title, severity and affected ranges."""
    found = {}
    for package in vulnerabilities.values():
        for via in package["via"]:
            if isinstance(via, dict):
                advisory = via["url"].removeprefix(ADVISORY_URL)
                title, severity, ranges = found.setdefault(advisory, (via["title"], via["severity"], set()))
                ranges.add(f"{via['name']} {via['range']}")
    return found


def main():
    parser = argparse.ArgumentParser(description=f"Run npm audit for the website and fail on any advisory that {CONFIG.relative_to(ROOT)} does not ignore.")
    parser.parse_args()
    allowed = ignored(tomllib.loads(CONFIG.read_text()))
    found = advisories(audit())
    for advisory, (title, severity, ranges) in sorted(found.items()):
        state = "ignored" if advisory in allowed else severity
        print(f"{advisory} ({state}): {title}; {', '.join(sorted(ranges))}")
    for advisory in sorted(allowed - found.keys()):
        print(f"{advisory} is no longer reported: remove it from {CONFIG.relative_to(ROOT)}")
    failing = [advisory for advisory in found if advisory not in allowed]
    if failing:
        sys.exit(f"{advisories_count(len(failing))} affect the website. Update the packages, or ignore an advisory in {CONFIG.relative_to(ROOT)} with the reason.")
    print(f"{advisories_count(len(found))} found, all ignored with a reason." if found else "npm audit found no advisories.")


def advisories_count(count):
    return f"{count} npm {'advisory' if count == 1 else 'advisories'}"


if __name__ == "__main__":
    main()
