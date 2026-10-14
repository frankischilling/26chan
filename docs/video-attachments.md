# Video attachments

Issue #204 covers WebM and MP4 uploads and native playback. The current branch
contains the processing foundation. Board admission, durable publication,
serving, client integration and complete qualification are still in progress.
Video upload capacity must remain disabled until those paths are connected.

## Source decisions

The reference is commit `545b7812d1849f7958d914950c91fdbbe38f6b22`.
Its `imgboard.php` SHA-256 is
`caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`.
The case-insensitive filename branch selects `.webm` or `.mp4` before ordinary
image parsing and the image-only GIF restriction. Both the original byte count
and the remuxed file are checked. The source applies the video byte cap to both
input containers; its later output check specifically names WebM, followed by
the general board byte cap for every format.

The imported 82 board configurations enable video. Audio is enabled on `/gif/`,
`/r/`, `/test/`, `/wsg/` and `/wsr/`. The duration limit is 120 seconds, except
300 on `/gif/` and 400 on `/wsg/`. Video input capacity is 4 MiB, except 2 MiB
on `/b/` and `/bant/` and 6 MiB on `/wsg/`; the general board cap also applies.
The source video dimension cap is 2048 pixels on either axis.

The source remuxes before probing. WebM admits VP8/VP9 and Vorbis/Opus;
MP4 admits H.264 with `yuv420p` and no more than eight raw bits, plus AAC where
audio is permitted. Its remux command uses automatic stream selection. A
second video stream or other stream type in the resulting probe record is
rejected. Container duration must be positive and no greater than the board
limit. Sample aspect corrections apply only strictly between 0.5 and 2, with
equal numerator/denominator ignored.

`fixtures/source-video-reference.json` contains the captured decision tail and
65 independent expected cases. The extractor checks the whole source file and
the excerpt hashes. Its optional PHP oracle executes only that tail against
synthetic probe records, with process-launch functions disabled. The oracle
passed locally with PHP 8.4.26. It does not establish actual container handling.

## Processing boundary

The proposed `video-v5` job carries one finite container tag and the bounded
input. It requires its own `video_initramfs`; an older manifest cannot send it
to an image or PDF guest. The guest remuxes, probes and renders one preview
under a shared deadline. The response is a fixed 9 MiB disk with bounded
metadata, at most 8 MiB of remuxed video, neutral RGBA preview pixels and zero
padding. The host checks this envelope and computes hashes itself. It does
not parse containers or link video codecs. Remuxed bytes remain untrusted media
and will require the separate restricted media origin and live approval checks.

The minimal runtime uses the signed upstream
[FFmpeg 7.1.5 release](https://ffmpeg.org/download.html).
The archive SHA-256 is
`de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`.
Its signature was verified against upstream's published key fingerprint
`FCF986EA15E6E293A5644F10B4322F04D67658D8` before recording this pin.
The build disables network protocols, device inputs, external codec libraries
and unused formats. It enables the source video decoders, stream-copy muxers
and raw preview output. The recipe retains the LGPL license; distribute the
matching source and license with deployed binaries.

The locally built FFmpeg and FFprobe executables are about 4 MiB each, with
only platform C/math dependencies. The ordinary distro build pulled in 213
runtime files totaling over 228 MB, exceeding the existing initramfs budget.
It is therefore used only to generate owned test clips, not packaged in a guest.
The video image retains the 64 MiB artifact cap and baseline 128 MiB guest RAM.
Its private 24 MiB tmpfs holds the bounded input and remux output. Host cgroup,
worker address-space, process, CPU and execution deadlines remain unchanged.
These proposed limits still require actual VM qualification.

Local prototype checks passed for remuxing and first-frame preview of owned
VP8/WebM and H.264/MP4 clips, both silent and with allowed Opus/AAC audio.
Host envelope validation, malformed framing and focused Rust lint also passed.
The full guest workspace is blocked locally by an unavailable cached GIF
dependency; hosted CI remains required. No VM or complete feature result is
claimed by these prototype checks.

## Remaining qualification

- Complete current-policy admission, byte checks, lease fencing and deletion.
- Check rotation and sample-aspect thumbnail behavior against actual owned clips.
- Serve bounded byte ranges after live approval and integrity checks.
- Restore expansion, hover duration/audio labels, mute and saved volume, mobile
  sizing, fallback playback, pause-on-scroll and complete player teardown.
- Cover ordinary forms, Quick Reply, catalogs, previews, snapshots and JSON.
- Pass actual guest containment, cancellation/recovery, persisted HTTP tests,
  browser regressions and all exact-final-head CI before closing #204.
