#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(id -u) = 0 ]] || { echo 'Run as root against the owned disposable Linux cluster.' >&2; exit 1; }
source .local/database.env
source .local/media.env
source .local/media-reader.env
source .local/intake.env
python3 scripts/test-attachment-restore.py
