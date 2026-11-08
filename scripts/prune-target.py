#!/usr/bin/env python3
"""Lets go of old builds of this project's own crates in `target`.

Cargo keeps every build it ever made of a crate: each change of dependencies or features gives the app a new hash, and
the old one stays, a gigabyte and a half of incremental state and object files each. `target/debug` once held 22
builds of the app and grew to 47 GB. This keeps the newest few of each of our crates and tests, which covers what
`check.sh` builds (the program, its tests, and clippy's check of both), and leaves the dependencies alone. Anything
it removes is only built again when it is needed.
"""

import os
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "target"
KEPT = 4
HASHED = re.compile(r"^(?P<name>[a-z0-9_]+)-(?P<hash>[0-9a-z]{13,16})(?:[.-]|$)")


def own_names() -> set[str]:
    names = {"proton_mail_light", "proton_core", "build_script_build"}
    for folder in ("app/tests", "core/tests", "app/examples", "core/examples", "app/benches", "core/benches"):
        place = ROOT / folder
        if place.is_dir():
            names.update(entry.stem.replace("-", "_") for entry in place.iterdir() if entry.suffix == ".rs")
    return names


def newest(paths: list[Path]) -> float:
    times = []
    for path in paths:
        try:
            times.append(path.lstat().st_mtime)
        except OSError:
            pass
    return max(times, default=0.0)


# Cargo hard-links a binary into several places: a file is counted once, by its inode.
SEEN: set[tuple[int, int]] = set()


def once(path: Path) -> int:
    try:
        status = path.lstat()
    except OSError:
        return 0
    key = (status.st_dev, status.st_ino)
    if key in SEEN or status.st_nlink > 1:
        # Another name of this file stays, or was counted already: nothing is freed by this one.
        SEEN.add(key)
        return 0
    SEEN.add(key)
    return status.st_size


def size(path: Path) -> int:
    if path.is_symlink() or path.is_file():
        return once(path)
    return sum(once(Path(folder) / file) for folder, _, files in os.walk(path) for file in files)


def remove(path: Path) -> int:
    freed = size(path)
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path, ignore_errors=True)
    else:
        try:
            path.unlink()
        except OSError:
            return 0
    return freed


def prune(folder: Path, names: set[str]) -> int:
    """In `deps` or `incremental`: groups entries by crate and build, and removes all but the newest builds."""
    if not folder.is_dir():
        return 0
    builds: dict[str, dict[str, list[Path]]] = {}
    for entry in folder.iterdir():
        found = HASHED.match(entry.name)
        if found and found["name"] in names:
            builds.setdefault(found["name"], {}).setdefault(found["hash"], []).append(entry)
    freed = 0
    for by_hash in builds.values():
        ordered = sorted(by_hash.values(), key=newest, reverse=True)
        for old in ordered[KEPT:]:
            freed += sum(remove(entry) for entry in old)
    return freed


def main() -> int:
    if not TARGET.is_dir():
        return 0
    names = own_names()
    freed = 0
    for profile in TARGET.iterdir():
        if profile.is_dir():
            freed += prune(profile / "deps", names) + prune(profile / "incremental", names)
            for triple in profile.iterdir():
                # Builds for another system keep their own profiles one level down.
                if triple.is_dir() and (triple / "deps").is_dir() and triple.name not in ("deps", "incremental"):
                    freed += prune(triple / "deps", names) + prune(triple / "incremental", names)
    print(f"== old builds let go: {freed / 1024**3:.1f} GB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
