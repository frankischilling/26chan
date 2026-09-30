# Synthetic GIF fixtures

These files were generated on September 28, 2026 by
`apps/media-guest/examples/gif-fixtures.rs` with the locked `gif` 0.14.2 encoder.
They contain constant indexed red/blue pixels. No external image, user upload or
original-site asset is included. The generator creates new files and refuses to
overwrite an existing fixture.

Create an empty directory and reproduce the files with:

```text
cargo run -p board-media-guest --example gif-fixtures --locked -- ABSOLUTE_EMPTY_DIRECTORY
```

The Rust tests compare each committed file with freshly encoded bytes. Native VM
and intake tests use these exact files. Generated test output never replaces the
fixtures.

| File | Purpose | SHA-256 |
|---|---|---|
| static.gif | One opaque red pixel | `5cd456f7e690a9004d8b29fbb70811faff50f21ed76b1ebf1a6596e97a559750` |
| transparent.gif | Red pixel followed by a transparent palette entry | `bf4bdcc2fd99aa905e3e9508c45d7c3abebffb8d7fe69e98403bc7990c9f1e9d` |
| interlaced.gif | Two columns, eight alternating red/blue rows in GIF interlace order | `38c39ad79c9f40798941aab7e08607a7d2540882960c26d8a0b667cef6df464d` |
| animated.gif | Two frames, rejected by the single-frame policy | `3c2c4148bc44e595f458af13f43f9e4d3e9dccf33f7120b93917df3b274e5270` |
| partial.gif | One pixel on a two-pixel canvas, rejected without a compositor | `23547e584ac8defcc1525cc197783793a532b72d3de2401f1a9bfb77d94a3a31` |
| too-wide.gif | 1,025-pixel width, rejected by the dimension limit | `a53e902ea0375d573535384ded4d9df35c8ed8fb8f362a970bc5967da2801c5e` |
