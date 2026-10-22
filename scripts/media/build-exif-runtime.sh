#!/usr/bin/env bash
# Build the pinned extractor for the disposable image guest, never the web tier.
set -euo pipefail
if [[ $# != 3 ]]; then
  echo 'usage: build-exif-runtime.sh exiftags_1.01.orig.tar.gz exiftags_1.01-8.debian.tar.xz NEW_OUTPUT_DIRECTORY' >&2
  exit 2
fi
archive=$(realpath "$1")
patches=$(realpath "$2")
output=$3
[[ ! -e "$output" ]] || { echo 'output already exists' >&2; exit 1; }
source_sha=d95744de5f609f1562045f1c2aae610e8f694a4c9042897a51a22f0f0d7591a4
patch_sha=ab45f09f55c43caae9ede45d137f1a9236c72690cefc0fe37ae863c8a71745f3
[[ $(sha256sum "$archive" | cut -d ' ' -f 1) == "$source_sha" && $(sha256sum "$patches" | cut -d ' ' -f 1) == "$patch_sha" ]] || {
  echo 'EXIF source differs from the reviewed Debian release' >&2; exit 1;
}
build=$(mktemp -d -t 26chan-exif-build-XXXXXXXX)
trap 'rm -rf -- "$build"' EXIT
tar -xzf "$archive" -C "$build"
src="$build/exiftags-1.01"
tar -xJf "$patches" -C "$src"
while IFS= read -r patch_name; do
  patch --batch --forward -d "$src" -p1 < "$src/debian/patches/$patch_name" >> "$build/patch.log"
done < "$src/debian/patches/series"
make -C "$src" exiftags -j2 CFLAGS='-Os -fstack-protector-strong -fPIE' CPPFLAGS='-D_FORTIFY_SOURCE=2' LDFLAGS='-Wl,-z,relro,-z,now -pie' > "$build/build.log" 2>&1
mkdir "$output"
install -m 0755 "$src/exiftags" "$output/exiftags"
install -m 0644 "$src/debian/copyright" "$output/copyright"
install -m 0644 "$build/build.log" "$output/build.log"
printf '%s\n%s\n' "$source_sha" "$patch_sha" > "$output/source.sha256"
echo "Built pinned EXIF extractor in $output"
