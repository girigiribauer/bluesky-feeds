#!/bin/sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu --locked |
  python3 -c '
import glob, hashlib, json, os, sys

MARKER = "==== Rust crates linked into the consumer program ====\n"
MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

meta = json.load(sys.stdin)
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
root = next(p["id"] for p in meta["packages"] if p["name"] == "bluesky-feeds-consumer")
seen, stack = set(), [root]
while stack:
    pid = stack.pop()
    if pid in seen:
        continue
    seen.add(pid)
    for dep in nodes[pid]["deps"]:
        if any(k["kind"] in (None, "normal") for k in dep["dep_kinds"]):
            stack.append(dep["pkg"])
crates = sorted((packages[p] for p in seen if packages[p]["source"]), key=lambda p: (p["name"], p["version"]))

groups = {}
for p in crates:
    label = p["name"] + " " + p["version"]
    folder = os.path.dirname(p["manifest_path"])
    files = sorted(f for pattern in ("LICEN[CS]E*", "COPYING*", "NOTICE*", "UNLICENSE*") for f in glob.glob(os.path.join(folder, pattern)) if os.path.isfile(f))
    texts = [open(f, encoding="utf-8", errors="replace").read().strip() + "\n" for f in files]
    if not texts:
        holders = ", ".join(p.get("authors") or []) or "the " + p["name"] + " authors"
        texts = ["MIT License\n\nCopyright (c) " + holders + "\n\n" + MIT]
    for text in texts:
        key = hashlib.sha256(text.encode()).hexdigest()
        groups.setdefault(key, (text, []))[1].append(label)

out = [MARKER, "\nThe consumer program statically links the following Rust crates:\n\n"]
for p in crates:
    out.append("- " + p["name"] + " " + p["version"] + " (" + (p.get("license") or "see below") + ")\n")
for text, labels in sorted(groups.values(), key=lambda g: g[1][0]):
    out.append("\n" + "-" * 78 + "\nApplies to: " + ", ".join(labels) + "\n" + "-" * 78 + "\n\n" + text)

path = "THIRD_PARTY_NOTICES"
current = open(path, encoding="utf-8").read()
head = current.split(MARKER)[0].rstrip("\n") + "\n\n"
open(path, "w", encoding="utf-8").write(head + "".join(out))
print(len(crates), "crates,", len(groups), "distinct license texts")
'
