#!/usr/bin/env python3
"""Writes what the program is built from, for the About page and for the bundle.

app/components/list.tsv      every crate that can end up in the program: name, version, license, where its
                             source is, and the systems it is built into (m macOS, w Windows, l Linux)
app/components/unshipped.txt the crates of Cargo.lock that are only for building or testing, or for other systems
app/components/notices.txt   the license and notice files those crates carry, each text once, with the crates
                             that carry it

Run it after any change to the dependencies. `app/tests/components.rs` fails until it has been run.
"""

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "app" / "components"
APP = "proton-mail-light"
SYSTEMS = [("m", "aarch64-apple-darwin"), ("m", "x86_64-apple-darwin"), ("w", "x86_64-pc-windows-msvc"),
           ("l", "x86_64-unknown-linux-gnu")]
NOTICE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|authors)([-._].*)?$", re.IGNORECASE)


def metadata(target=None):
    platform = ["--filter-platform", target] if target else []
    found = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", str(ROOT / "Cargo.toml")]
        + platform,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(found.stdout)


def shipped(meta):
    """The crates reached from the app by normal dependencies only, for every target."""
    nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
    start = next(package["id"] for package in meta["packages"] if package["name"] == APP)
    seen, waiting = set(), [start]
    while waiting:
        current = waiting.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                waiting.append(dependency["pkg"])
    return seen


def own(package):
    return package["source"] is None and not package["manifest_path"].startswith(str(ROOT / "vendor"))


def notice_files(package):
    folder = pathlib.Path(package["manifest_path"]).parent
    named = [package.get("license_file")] if package.get("license_file") else []
    files = {folder / name for name in named}
    files |= {path for path in folder.iterdir() if path.is_file() and NOTICE_FILE.match(path.name)}
    return sorted(files)


def main():
    meta = metadata()
    ids = shipped(meta)
    systems = {}
    for letter, target in SYSTEMS:
        for found in shipped(metadata(target)):
            systems.setdefault(found, set()).add(letter)
    packages = sorted(meta["packages"], key=lambda package: (package["name"], package["version"]))
    rows, unshipped, texts, shown = [], [], {}, {}
    for package in packages:
        label = f"{package['name']} {package['version']}"
        # Reached only on a system the app is not built for, such as Android: never in the program.
        if package["id"] not in ids or not systems.get(package["id"]):
            unshipped.append(label)
            continue
        if own(package):
            continue
        license_name = package.get("license") or "see its license file"
        source = package.get("repository") or package.get("homepage") or ""
        built_into = "".join(letter for letter in "mwl" if letter in systems.get(package["id"], set()))
        rows.append("\t".join([package["name"], package["version"], license_name, source, built_into]))
        files = notice_files(package)
        if not files:
            texts.setdefault(f"No license file in the package. Its license: {license_name}.", []).append(label)
        for path in files:
            text = path.read_text(encoding="utf-8", errors="replace").strip()
            # The same license laid out differently is the same text.
            same = " ".join(text.split())
            texts.setdefault(same, []).append(label)
            shown.setdefault(same, text)
    OUT.mkdir(exist_ok=True)
    (OUT / "list.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")
    (OUT / "unshipped.txt").write_text("\n".join(unshipped) + "\n", encoding="utf-8")
    parts = []
    for same, carriers in sorted(texts.items(), key=lambda item: sorted(item[1])[0]):
        parts.append("== " + ", ".join(sorted(set(carriers))) + "\n\n" + shown.get(same, same) + "\n")
    (OUT / "notices.txt").write_text("\n".join(parts), encoding="utf-8")
    size = (OUT / "notices.txt").stat().st_size
    print(f"{len(rows)} components, {len(unshipped)} only for building, {len(texts)} texts in {size} bytes")


if __name__ == "__main__":
    sys.exit(main())
