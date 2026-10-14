#!/usr/bin/env bash
# Build only the reviewed remux/probe/first-frame surface. Output is not installed.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo 'usage: build-video-runtime.sh ffmpeg-7.1.5.tar.xz NEW_OUTPUT_DIRECTORY' >&2
  exit 2
fi
archive=$(realpath "$1")
output=$2
[[ ! -e "$output" ]] || { echo 'output already exists' >&2; exit 1; }
expected=de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f
[[ $(sha256sum "$archive" | cut -d ' ' -f 1) == "$expected" ]] || {
  echo 'FFmpeg archive differs from the reviewed signed release' >&2; exit 1;
}
build=$(mktemp -d -t 26chan-video-build-XXXXXXXX)
trap 'rm -rf -- "$build"' EXIT
tar -xJf "$archive" -C "$build"
mkdir "$build/work"
(
  cd "$build/work"
  ../ffmpeg-7.1.5/configure \
    --disable-everything --disable-autodetect --disable-network --disable-doc \
    --disable-debug --disable-x86asm --disable-avdevice --disable-postproc \
    --disable-shared --enable-static --enable-ffmpeg --enable-ffprobe \
    --enable-protocol=file,pipe --enable-demuxer=matroska,mov \
    --enable-muxer=webm,mp4,image2pipe --enable-decoder=vp8,vp9,h264 \
    --enable-parser=vp8,vp9,h264 --enable-encoder=ppm \
    --enable-filter=scale,format,null,transpose,hflip,vflip,rotate --enable-bsf=aac_adtstoasc,h264_mp4toannexb \
    --extra-cflags=-Os > configure.log
  make -j2 > build.log 2>&1
)
mkdir "$output"
install -m 0755 "$build/work/ffmpeg" "$output/ffmpeg"
install -m 0755 "$build/work/ffprobe" "$output/ffprobe"
install -m 0644 "$build/work/configure.log" "$output/configure.log"
install -m 0644 "$build/ffmpeg-7.1.5/COPYING.LGPLv2.1" "$output/COPYING.LGPLv2.1"
printf '%s\n' "$expected" > "$output/source.sha256"
echo "Built reviewed video tools in $output"
