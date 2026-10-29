"""Explicit install operation. Downloads only reviewed files from an immutable revision."""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile
import urllib.request

from model import FILES, KNOWN_SHA256, REPOSITORY, REVISION


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    base = f"https://huggingface.co/{REPOSITORY}"
    url = f"https://huggingface.co/api/models/{REPOSITORY}/revision/{REVISION}?blobs=true"
    with urllib.request.urlopen(url, timeout=30) as response:
        metadata = json.loads(response.read(2 * 1024 * 1024 + 1))
    if metadata["sha"] != REVISION:
        raise ValueError("model revision mismatch")
    entries = {x["rfilename"]: x for x in metadata["siblings"]}
    args.directory.mkdir(parents=True, exist_ok=True)
    hashes = {}
    for name in FILES:
        entry = entries[name]
        size = entry["size"]
        if not 0 < size <= 140 * 1024 * 1024:
            raise ValueError("invalid model file size")
        target = args.directory / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                sha = hashlib.sha256()
                git = hashlib.sha1(f"blob {size}\0".encode())
                total = 0
                with urllib.request.urlopen(f"{base}/resolve/{REVISION}/{name}", timeout=60) as response:
                    while block := response.read(1024 * 1024):
                        total += len(block)
                        if total > size:
                            raise ValueError("oversized model file")
                        output.write(block)
                        sha.update(block)
                        git.update(block)
                if total != size:
                    raise ValueError("incomplete model download")
                actual = sha.hexdigest()
                # LFS/Xet payloads use SHA256; ordinary Git blobs use their Git object ID.
                if "lfs" in entry:
                    if actual != entry["lfs"]["sha256"]:
                        raise ValueError("model SHA256 mismatch")
                elif git.hexdigest() != entry["blobId"]:
                    raise ValueError("model Git object mismatch")
                if name in KNOWN_SHA256 and actual != KNOWN_SHA256[name]:
                    raise ValueError("reviewed artifact mismatch")
                hashes[name] = actual
                output.flush()
                temporary.replace(target)
            finally:
                temporary.unlink(missing_ok=True)
    manifest = args.directory / "manifest.json"
    temporary = manifest.with_suffix(".tmp")
    temporary.write_text(json.dumps({"repository": REPOSITORY, "revision": REVISION, "sha256": hashes}, indent=2) + "\n")
    temporary.replace(manifest)
    print(json.dumps({"revision": REVISION, "files": len(hashes)}))


if __name__ == "__main__":
    main()
