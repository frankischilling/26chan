#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
temporary=$(mktemp -d)
trap 'rm -rf -- "$temporary"' EXIT HUP INT TERM
if [ "$#" -eq 1 ]; then
    python3 scripts/media/prepare-swf-player.py "$temporary/player" --archive "$1"
else
    python3 scripts/media/prepare-swf-player.py "$temporary/player"
fi
cmp fixtures/ruffle-player-manifest.json "$temporary/player/manifest.json"
node --input-type=module - "$temporary/player" <<'JS'
import fs from 'node:fs';
import path from 'node:path';
const root=process.argv[2];
for(const name of fs.readdirSync(root).filter(name=>name.endsWith('.wasm'))){
  if(!WebAssembly.validate(fs.readFileSync(path.join(root,name))))throw new Error('Patched module is invalid');
}
JS
cargo run --locked -p board-media-http --example player-assets -- "$temporary/player"
printf 'not the pinned player' > "$temporary/player/ruffle.js"
if cargo run --locked -p board-media-http --example player-assets -- "$temporary/player"; then
    echo 'Substituted runtime was accepted' >&2
    exit 1
fi
