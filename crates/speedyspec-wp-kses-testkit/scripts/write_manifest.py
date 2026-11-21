"""Writes corpus/MANIFEST.json: every attack and bomb file with its source, commit, license and SHA-256.

Usage: python3 -I scripts/write_manifest.py <corpus>
"""

import hashlib
import json
import os
import re
import sys

CORPUS = sys.argv[1]
entries = []
for name in sorted(os.listdir(os.path.join(CORPUS, "attacks"))):
    if not name.endswith(".json"):
        continue
    id = name[:-5]
    source = open(os.path.join(CORPUS, "attacks", id + ".SOURCE.md")).read()
    m = re.search(r"From (https://github.com/\S+) at commit (\w+) \(([^;]+);", source)
    data = open(os.path.join(CORPUS, "attacks", name), "rb").read()
    entries.append({"file": "attacks/" + name, "source": m.group(1), "commit": m.group(2), "license": m.group(3),
                    "sha256": hashlib.sha256(data).hexdigest()})
for name in sorted(os.listdir(os.path.join(CORPUS, "bombs"))):
    if name.endswith(".md"):
        continue
    data = open(os.path.join(CORPUS, "bombs", name), "rb").read()
    entries.append({"file": "bombs/" + name, "source": "written for the kses port", "commit": "", "license": "MIT",
                    "sha256": hashlib.sha256(data).hexdigest()})
with open(os.path.join(CORPUS, "MANIFEST.json"), "w") as f:
    json.dump(entries, f, indent=2)
    f.write("\n")
print(len(entries), "entries")
