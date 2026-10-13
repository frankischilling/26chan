# PDF attachments

PDF processing is an opt-in branch of the existing development media pipeline.
Production activation still requires the operator qualification described in
[media dispatch](media-dispatch.md). The PDF implementation is under qualification;
a passing local renderer test does not establish a passing disposable-VM run.

## Source behavior

The reference is commit `545b7812d1849f7958d914950c91fdbbe38f6b22`. Its
`imgboard.php` SHA-256 is
`caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445`.
The imported `ENABLE_PDF` setting enables `/po/`, `/sci/`, `/qst/`, `/tg/` and
`/gd/`. `/sci/` permits 4 MiB; the other four permit 8 MiB, also the intake cap.
The board's current settings are checked again when an attachment is consumed.
A processed upload is not permission to post after a board policy change.

The source selects a PDF by its case-insensitive `.pdf` filename suffix before
its ordinary image branch. PDF metadata has width and height `1`, while the
header displays `PDF`. The image-only GIF restriction does not apply to that
branch. General byte and dimension policy still applies. A PNG named `.pdf`
is rejected on a PDF-enabled board; a PDF cannot acquire image authority by
being named `.png`.

The source validates the document with Ghostscript's `nullpage` device and uses
its JPEG output for a preview. Its multipage output is read as an image, which
selects the first page. The rewrite explicitly renders page one after validating
the whole document. Later malformed page content must not be hidden by a valid
first page. The reference does not provide its Ghostscript build, fonts or
rasterizer version. Exact historical preview bytes are therefore not claimed.

`js/extension.js` has SHA-256
`05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31`.
Its PDF links open the document normally; image expansion and full-image hover
return without handling them. The rewrite follows that interaction. Spoiler
reveal uses the static preview, and deletion retires the document URL alongside
its thumbnail. Quick Reply inherits the board form's accepted types.

## Processing and publication

The coordinator takes one bounded snapshot of the uploaded file. `%PDF-` selects
an explicit `IBJOB004` request and `IBOUT004` response. The broker requires a
separately hashed `pdf_initramfs` artifact. An older manifest cannot silently
send a PDF to the image worker. PNG, paired drawing and GIF modes retain their
own transport versions and original initramfs.

The document parser runs only inside the disposable guest. The PDF image carries
the installed root-owned Ghostscript executable, its shared libraries, bundled
resources and URW base fonts. `pdf-runtime.json` records their SHA-256 hashes.
The builder rejects writable or untrusted runtime files and enforces the file
count and artifact byte budgets. The
[resource provenance manifest](pdf-runtime-resources.json) records the official
Ubuntu font and Ghostscript resource packages, archive checksums and file hashes.
CI's writable `/usr/share` directory cannot authenticate those resources, so the
builder checks the exact bounded bytes against the reviewed digests instead.
Missing resources or different contents fail the build. The packager includes only
that reviewed inventory, not the host's extra font hooks or configuration files.
These pins currently match Ubuntu 24.04's listed packages; a different resource
package revision needs a reviewed manifest update. Executables and shared
libraries still require trusted parent directories. Identical read-only resource
files share one stored copy through guest-local symlinks; the complete initramfs
still has the same 64 MiB cap. Adobe Font Metrics sidecars are omitted; PDF
rendering uses the retained Type 1 font programs. CI packages the release-profile
guest binaries, keeping debug metadata out of the boot image. PDF initramfs files
use gzip with a fixed timestamp; both stored and unpacked bytes must fit the
64 MiB cap. The pinned kernel enables `CONFIG_RD_GZIP`. Compression keeps the
boot artifacts inside the existing private tmpfs without enlarging its quota. The
image contains no host credentials, user font configuration or font caches.

To update these resource pins, retrieve the packages from the official Ubuntu
archive over HTTPS, verify their SHA-256 values against the package index, and
hash the regular files under the fixed resource roots directly from the archives.
Record package symlink aliases with their target bytes, binding each expected hash
to the requested archive path.
Do not derive new approved hashes from a writable installed directory. Package
extraction for this review does not require installing or executing the packages.
The manifest is reviewed repository data; runtime packaging performs no downloads.


Ghostscript receives the snapshot on stdin, a cleared environment and fixed
arguments. No upload supplies a command, output pathname or renderer option.
The whole-document check and first-page renderer share a six-second deadline.
The guest's process limits, no-network VM and broker deadline remain in force.
PDF spooling uses a private 16 MiB tmpfs. Password-protected files and documents
that Ghostscript reports as malformed are rejected without a password prompt.
Strict PDF error and warning handling is a deliberate backend validation rule.

The preview crosses the VM boundary as the existing fixed RGBA output disk.
Its dimensions must fit within 1024 by 1024 pixels. A page that exceeds that
preview budget is rejected rather than rescaled by an unbounded host decoder.
The host encodes a PNG thumbnail and validates its dimensions independently of
the source's `1x1` document metadata.

The published PDF is the exact retained snapshot, not bytes returned by the
worker. Its size must match the queued input size. SHA-256 binds stored bytes to
approval; the original file's MD5 supplies the existing public interoperability
field. PDF approval does not claim the PNG source-cleanup provenance profile.
Publication keeps the existing storage lock, lease fencing, immutable format,
reservation replay and uncertain-commit rules.

## Serving

PDFs use fixed `.pdf` routes and `application/pdf`; their previews use the usual
PNG thumbnail representation. File headers, catalog thumbnails, quote previews
and the public API use the approved format. Document URLs cannot be used as
image sources by the client snapshot grammar.

The media origin serves PDFs with `nosniff`, its restrictive CSP and sandbox,
no cookies, and no drawing-import CORS permission. A successful parser run does
not make active PDF content trusted. PDFs must remain on that separate origin.
GET, HEAD and conditional requests check current approval and actual stored bytes
before returning a response. File deletion removes visibility for the document
and preview; publication cleanup includes the `.pdf` object.

## Verification

Owned fixtures cover first-page selection, base-font text, encrypted and malformed
documents, preview and input bounds, explicit protocol versions, retained bytes,
format fencing, board admission, conditional HTTP reads and deletion. HTTP storage
fixtures intentionally do not stand in for document-parser tests.

`tests/media/test_pdf_vm.py` runs complete synthetic documents through the actual
PDF guest. `tests/media/test_dispatch.py` also checks queue intake, authenticated
transport, guest processing, typed approval and exact document readback. These
native checks require the reviewed disposable Linux qualification environment.
Full CI and browser qualification must pass before merging the feature.
