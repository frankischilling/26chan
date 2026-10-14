#!/usr/bin/env bash
# Build the pinned lossless metadata cleaner for the disposable image guest.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo 'usage: build-still-runtime.sh libjpeg-turbo-3.2.0.tar.gz NEW_OUTPUT_DIRECTORY' >&2
  exit 2
fi
archive=$(realpath "$1")
output=$2
[[ ! -e "$output" ]] || { echo 'output already exists' >&2; exit 1; }
expected=6f30092cef9fb839779646608f4ee14ae3cbac989c47fa05e841b0841f09878e
[[ $(sha256sum "$archive" | cut -d ' ' -f 1) == "$expected" ]] || {
  echo 'JPEG source differs from the reviewed official release' >&2; exit 1;
}
build=$(mktemp -d -t 26chan-still-build-XXXXXXXX)
trap 'rm -rf -- "$build"' EXIT
tar -xzf "$archive" -C "$build"
cmake -S "$build/libjpeg-turbo-3.2.0" -B "$build/work" \
  -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=OFF \
  -DWITH_TOOLS=ON -DWITH_TESTS=OFF -DWITH_SIMD=OFF \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS=-Os > "$build/configure.log"
cmake --build "$build/work" --target jpegtran-static -j2 > "$build/build.log" 2>&1
mkdir "$output"
install -m 0755 "$build/work/jpegtran-static" "$output/jpegtran"
install -m 0644 "$build/configure.log" "$output/configure.log"
install -m 0644 "$build/libjpeg-turbo-3.2.0/LICENSE.md" "$output/LICENSE.md"
printf '%s\n' "$expected" > "$output/source.sha256"
echo "Built reviewed JPEG cleaner in $output"
