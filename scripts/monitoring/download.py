"""Download only pinned official x86_64 monitoring tools; standard library only."""

import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import tarfile
import tempfile
import urllib.request

# GitHub official release asset SHA256 digests, checked 2026-09-10.
# https://github.com/prometheus/{prometheus,alertmanager}/releases
RELEASES = {
    "prometheus": ("3.14.0", {
        "linux": "f665c6da19eb7ba399c915d30c7d9793c9b417bf8a749b504bc470678631478d",
        "windows": "272bcdd15d9327c7b1e08fe916ea48633819f82f2ea0bf354e6b8c0350c156ba",
    }, ("prometheus", "promtool")),
    "alertmanager": ("0.34.0", {
        "linux": "19c75a11d8c03dc4ade7abdbddfb3a8f28c9e7b000d0849cda0cd71dffd74a03",
        "windows": "5e52bce013a5d8cf81748b007e03c8fcff3f51842548c92f9b690bc75ad851a6",
    }, ("alertmanager", "amtool")),
}
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024


def verify(path, expected):
    with path.open("rb") as stream:
        actual = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != expected:
        raise ValueError(f"SHA256 mismatch for {path.name}")


def download(destination):
    system = platform.system().lower()
    if system not in ("linux", "windows") or platform.machine().lower() not in ("amd64", "x86_64"):
        raise ValueError("Only Linux and Windows x86_64 releases are pinned")
    destination.mkdir(parents=True, exist_ok=True)
    for project, (version, checksums, binaries) in RELEASES.items():
        root = f"{project}-{version}.{system}-amd64"
        url = f"https://github.com/prometheus/{project}/releases/download/v{version}/{root}.tar.gz"
        with tempfile.TemporaryDirectory(prefix="board-monitor-download-") as temporary:
            archive = Path(temporary) / f"{root}.tar.gz"
            with urllib.request.urlopen(url, timeout=60) as source, archive.open("wb") as output:
                count = 0
                while chunk := source.read(1024 * 1024):
                    count += len(chunk)
                    if count > MAX_ARCHIVE_BYTES:
                        raise ValueError("Official archive exceeds bounded download size")
                    output.write(chunk)
            verify(archive, checksums[system])
            with tarfile.open(archive, "r:gz") as release:
                for binary in binaries:
                    name = binary + (".exe" if system == "windows" else "")
                    member = release.getmember(f"{root}/{name}")
                    if not member.isfile() or member.size > MAX_ARCHIVE_BYTES:
                        raise ValueError("Release binary is not a bounded regular file")
                    # Copy only named files; never extract archive paths or links.
                    with release.extractfile(member) as source, (destination / name).open("wb") as output:
                        shutil.copyfileobj(source, output)
                    os.chmod(destination / name, 0o755)
            print(f"Verified {project} {version} ({system}-amd64)", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, default=Path(".local/monitoring/bin"))
    download(parser.parse_args().destination.resolve())
