#!/usr/bin/env python3
import argparse
import concurrent.futures
import dataclasses
import datetime
import functools
import json
import pathlib
import subprocess
import sys
import threading
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".config" / "dependency-age.toml"
CARGO_LOCK = "Cargo.lock"
NPM_LOCK = "website/package-lock.json"
CRATES_REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
CRATES_INDEX = "https://index.crates.io"
CRATES_API = "https://crates.io/api/v1/crates"
CRATES_API_INTERVAL_SECS = 1
NPM_REGISTRY = "https://registry.npmjs.org"
NODE_MODULES = "node_modules/"
USER_AGENT = "borg-commander dependency age check (https://github.com/0xcd21h/borg-commander)"
NOT_FOUND = 404
ATTEMPTS = 3
RETRY_DELAY_SECS = 2
TIMEOUT_SECS = 60
WORKERS = 8
ECOSYSTEM_LOCKS = {"crate": CARGO_LOCK, "npm": NPM_LOCK}
CRATES_API_LOCK = threading.Lock()


@dataclasses.dataclass(frozen=True, order=True)
class Package:
    lock: str
    name: str
    version: str

    def __str__(self):
        return f"{self.lock}: {self.name} {self.version}"


@dataclasses.dataclass(frozen=True)
class Release:
    published: datetime.datetime | None
    yanked: bool = False


def fetch(url, decode, missing):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "application/json"})
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(request, timeout=TIMEOUT_SECS) as response:
                return decode(response.read())
        except urllib.error.HTTPError as error:
            if error.code == NOT_FOUND:
                return missing
            failure = f"HTTP {error.code}"
        except urllib.error.URLError as error:
            failure = error.reason
        if attempt < ATTEMPTS:
            time.sleep(RETRY_DELAY_SECS * attempt)
    sys.exit(f"{url}: {failure}")


def index_lines(body):
    return [json.loads(line) for line in body.decode().splitlines() if line.strip()]


def parse_time(text):
    return datetime.datetime.fromisoformat(text) if text else None


def index_path(name):
    lowered = name.lower()
    match len(lowered):
        case 1 | 2:
            return f"{len(lowered)}/{lowered}"
        case 3:
            return f"3/{lowered[0]}/{lowered}"
        case _:
            return f"{lowered[:2]}/{lowered[2:4]}/{lowered}"


@functools.cache
def index_entries(name):
    return fetch(f"{CRATES_INDEX}/{index_path(name)}", index_lines, [])


def crate_created_at(name, version):
    with CRATES_API_LOCK:
        time.sleep(CRATES_API_INTERVAL_SECS)
        found = fetch(f"{CRATES_API}/{name}/{version}", json.loads, None)
    return found["version"]["created_at"] if found else None


def crate_release(name, version):
    entry = next((entry for entry in index_entries(name) if entry["vers"] == version), None)
    if entry is None:
        return None
    published = entry.get("pubtime") or crate_created_at(name, version)
    return Release(parse_time(published), entry["yanked"])


@functools.cache
def npm_times(name):
    return fetch(f"{NPM_REGISTRY}/{urllib.parse.quote(name, safe='@')}", json.loads, {}).get("time", {})


def npm_release(name, version):
    times = npm_times(name)
    return Release(parse_time(times[version])) if version in times else None


RELEASES = {CARGO_LOCK: crate_release, NPM_LOCK: npm_release}


def release(package):
    return RELEASES[package.lock](package.name, package.version)


def cargo_packages(text):
    return {
        Package(CARGO_LOCK, entry["name"], entry["version"])
        for entry in tomllib.loads(text).get("package", [])
        if entry.get("source") == CRATES_REGISTRY
    }


def npm_packages(text):
    return {
        Package(NPM_LOCK, entry.get("name") or path.rsplit(NODE_MODULES, 1)[-1], entry["version"])
        for path, entry in json.loads(text).get("packages", {}).items()
        if path and not entry.get("link")
    }


PARSERS = {CARGO_LOCK: cargo_packages, NPM_LOCK: npm_packages}


def read(path, revision):
    if revision is None:
        file = ROOT / path
        return file.read_text() if file.exists() else None
    shown = subprocess.run(["git", "show", f"{revision}:{path}"], cwd=ROOT, capture_output=True, text=True)
    return shown.stdout if shown.returncode == 0 else None


def locked(revision):
    return {
        package
        for path, parse in PARSERS.items()
        if (text := read(path, revision)) is not None
        for package in parse(text)
    }


def verify_revision(revision):
    found = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"{revision}^{{commit}}"], cwd=ROOT, capture_output=True)
    if found.returncode != 0:
        sys.exit(f"unknown Git revision: {revision}")


def exceptions(config):
    allowed = set()
    for entry in config.get("exceptions", []):
        for key, lock in ECOSYSTEM_LOCKS.items():
            if key in entry:
                name, version = entry[key].rsplit("@", 1)
                allowed.add(Package(lock, name, version))
    return allowed


def problem(package, found, now, minimum, allowed):
    if found is None:
        return f"{package}: not found in the registry"
    if found.yanked:
        return f"{package}: yanked"
    if found.published is None:
        return f"{package}: the registry has no publish date"
    ready = found.published + minimum
    if ready > now and package not in allowed:
        return f"{package}: published {found.published:%Y-%m-%d %H:%M} UTC, allowed from {ready:%Y-%m-%d %H:%M} UTC"
    return None


def main():
    parser = argparse.ArgumentParser(description="Check that every locked crate and npm package was published long enough ago and is not yanked.")
    parser.add_argument("--base", metavar="REVISION", help="check only the packages that are new since this Git revision")
    args = parser.parse_args()
    config = tomllib.loads(CONFIG.read_text())
    minimum = datetime.timedelta(days=config["minimum-days"])
    if args.base:
        verify_revision(args.base)
    packages = sorted(locked(None) - locked(args.base) if args.base else locked(None))
    with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
        releases = list(pool.map(release, packages))
    now = datetime.datetime.now(datetime.UTC)
    allowed = exceptions(config)
    issues = [issue for package, found in zip(packages, releases) if (issue := problem(package, found, now, minimum, allowed))]
    for issue in issues:
        print(issue)
    if issues:
        sys.exit(f"{len(issues)} of {len(packages)} packages break the dependency rules in DEVELOPMENT.md.")
    print(f"Checked {len(packages)} packages: none yanked, all published at least {minimum.days} days ago.")


if __name__ == "__main__":
    main()
