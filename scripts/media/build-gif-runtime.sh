#!/usr/bin/env bash
# Build the pinned cleaner for the disposable GIF worker.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo 'usage: build-gif-runtime.sh gifsicle-1.96.tar.gz NEW_OUTPUT_DIRECTORY' >&2
  exit 2
fi
archive=$(realpath "$1")
output=$2
[[ ! -e "$output" ]] || { echo 'output already exists' >&2; exit 1; }
expected=fd23d279681a6dfe3c15264e33f344045b3ba473da4d19f49e67a50994b077fb
[[ $(sha256sum "$archive" | cut -d ' ' -f 1) == "$expected" ]] || {
  echo 'GIF source differs from the reviewed official release' >&2; exit 1;
}
build=$(mktemp -d -t 26chan-gif-build-XXXXXXXX)
trap 'rm -rf -- "$build"' EXIT
tar -xzf "$archive" -C "$build"
(
  cd "$build/gifsicle-1.96"
  ./configure --disable-gifview --disable-gifdiff CFLAGS='-Os' > "$build/configure.log"
  make -C src gifsicle -j2 > "$build/build.log" 2>&1
)
mkdir "$output"
install -m 0755 "$build/gifsicle-1.96/src/gifsicle" "$output/gifsicle"
install -m 0644 "$build/gifsicle-1.96/COPYING" "$output/COPYING"
install -m 0644 "$build/configure.log" "$output/configure.log"
printf '%s\n' "$expected" > "$output/source.sha256"
echo "Built reviewed GIF cleaner in $output"
