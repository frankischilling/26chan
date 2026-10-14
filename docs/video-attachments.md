# Video attachments

WebM and MP4 attachments use isolated processing, source board admission,
approval-bound publication and native playback. This document covers the
implementation and qualification for issue #204.

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
synthetic probe records, with process-launch functions disabled. The local
reference capture used PHP 8.4.26. Actual container handling is checked separately
in the guest.

## Processing boundary

The `video-v5` job carries one finite container tag and the bounded
input. It requires its own `video_initramfs`; an older manifest cannot send it
to an image or PDF guest. The guest remuxes, probes and renders one preview
under a shared deadline. The response is a fixed 9 MiB disk with bounded
metadata, at most 8 MiB of remuxed video, neutral RGBA preview pixels and zero
padding. The host checks this envelope and computes hashes itself. It does
not parse containers or link video codecs. Remuxed bytes remain untrusted media
and use the separate restricted media origin and live approval checks.

The minimal runtime uses the signed upstream
[FFmpeg 7.1.5 release](https://ffmpeg.org/download.html).
The archive SHA-256 is
`de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`.
Its signature was verified against upstream's published key fingerprint
`FCF986EA15E6E293A5644F10B4322F04D67658D8` before recording this pin.
The build disables network protocols, device inputs, external codec libraries
and unused formats. It enables the source video decoders, stream-copy muxers
and bounded PPM preview output inside the guest. That raw pixel transport is
converted to RGBA before leaving the guest. Autorotation precedes source
sample-aspect correction and the 250-pixel preview cap. The recipe retains the LGPL license; distribute the
matching source and license with deployed binaries.

The locally built FFmpeg and FFprobe executables are about 4 MiB each, with
only platform C/math dependencies. The ordinary distro build pulled in 213
runtime files totaling over 228 MB, exceeding the existing initramfs budget.
It is therefore used only to generate owned test clips, not packaged in a guest.
The video image retains the 64 MiB artifact cap and baseline 128 MiB guest RAM.
Its private 24 MiB tmpfs holds the bounded input and remux output. Host cgroup,
worker address-space, process, CPU and execution deadlines remain unchanged.
Owned fixtures cover VP8/WebM and H.264/MP4, both silent and with allowed
Opus/AAC audio. The rotated MP4 retains its 64×48 public dimensions and produces
a 48×64 preview. A 3:2 sample-aspect clip produces a 64×32 preview. Rejected
fixtures cover excessive duration and dimensions, MPEG-4 Part 2 and H.264
4:4:4 chroma; the next valid job must still succeed. SIGTERM and SIGINT cases
check that cancellation removes the job's processes and output authority.

## Publication and playback

Migration 0147 adds the source board policy and immutable duration/audio
metadata. The posting transaction checks the current locked board again,
including filename/container agreement and separate input/output caps. Video
metadata requires a current processing lease and cannot change after insertion.
Reply video metadata is capped at 125 pixels; OP metadata keeps the 250-pixel
cap. Both use the bounded first-frame preview. Source JPEG thumbnail bytes are
replaced with host-encoded PNGs at the same `s.jpg` routes. The guest returns
neutral pixels so credentialed services need no JPEG decoder; exact source JPEG
compression is not preserved. The HTTP integration covers both
containers as OPs and replies, current-policy rejection, retries, byte ranges,
conditional reads, tampered files and deletion. The full migration chain and
populated historical upgrade/restore tests include the new immutable fields.

The media reader verifies approval and the whole file's hash before serving a
single byte range. Unsatisfiable ranges return 416; unsupported range syntax
falls back to the complete object. HEAD and conditional responses keep their
usual semantics. Video URLs cannot become image sources in updater recipes.

Successful video responses retain the isolated media origin in their CSP
sandbox. Chromium's standalone video document requests the file through an
anonymous-CORS media element; an opaque sandbox origin would need a CORS grant
for `null`. Keeping the media origin lets that read remain same-origin while
scripts, frames and forms remain blocked. Other media types and errors keep the
opaque sandbox. The decoder's CORS grant still names only the configured board
origin.

Native players have controls, loop, mute preference, saved volume and a close
button. Hover previews show duration and browser-detected audio. Closing,
retiring, hiding or disabling a player releases its source. Playback failure
keeps the filename link available for opening or downloading the attachment.
The WebM expansion fallback uses the pinned OGV 1.9.0 decoder when native WebM
support is missing or on the original iOS path. Hover stays native, as in the
source. It provides play/pause, mute, seek, volume,
fullscreen and elapsed time. The source SVG controls keep their 32-pixel overlay,
0–100 volume scale and tap-to-show behavior; keyboard-accessible buttons replace
the original clickable divs. `fixtures/source-video-controls.json` records the
source hash and exact SVG paths. Only one fallback decoder runs at once. Closing
it calls OGV's stop method to abort reads and terminate codec workers.

The fallback's scripts and WebAssembly files have fixed routes and recorded
hashes under `docs/third-party/ogv/`. CSP permits those assets and WebAssembly
compilation without allowing JavaScript eval, arbitrary workers or blob workers.
The configured board origin can read approved video bytes for this decoder;
other origins receive no CORS grant. Thumbnail and PDF behavior is unchanged.
Browser fixtures exercise VP8, VP9, Opus and Vorbis decoding, repeated loops,
controls, saved volume, cancellation and decoder teardown. Standalone video
checks use the media server policy and verify that page scripts stay blocked.

The post menu retains mobile file deletion and opening for video attachments.
Image-search links use the approved video thumbnail, as in the source, rather
than submitting the video container to an image-search provider.

## Verification

- `scripts/extract-video-reference.py --php-oracle` checks the source admission
  decisions against independent expected cases.
- `tests/media/test_video_vm.py` exercises actual isolated decoding, rejected
  containers and recovery. `tests/media/test_intake_service.py` sends owned
  files through authenticated intake, persisted publication and browser posting.
- `apps/media-http/tests/video.rs` checks approved bytes, range and conditional
  behavior, CORS, current board policy and deletion against PostgreSQL.
- `tests/browser/native-images-dom.test.mjs` checks native and fallback playback,
  source controls, mobile behavior and teardown. `quick-reply-upload.mjs` covers
  real standalone and expanded playback under the application security policy.
- The ordinary upload browser checks thumbnails, catalogs, JSON/checksums and
  no-JavaScript viewing. Preview and updater tests keep video URLs out of image
  source recipes.

The full media qualification needs the reviewed guest image and a Linux host
that can run the repository's isolated dispatch harness. A browser fixture or
local unit test does not replace that run. Use the existing `media-and-operations`
CI lane and the complete application checks before merging; deployment retains
its separate operator qualification requirements.

Hosted browser checks verify standalone WebM and MP4 playback and blocked
page scripts under the video policy. The complete Quick Reply video flow is
awaiting qualification of its mobile file menu. Issue #204 remains open until
that run and all exact-final-head checks pass.
