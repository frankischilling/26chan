# Synthetic JPEG fixtures

Generated locally on September 12, 2026 from constant pixels by the workspace
example `apps/media-guest/examples/jpeg-fixtures.rs`, using test-only
`jpeg-encoder =0.7.1`, quality 100 and its default subsampling. No external image,
camera metadata, original-site asset or user upload is included.

To reproduce, create an empty directory and run:

```text
cargo run -p board-media-guest --example jpeg-fixtures --locked -- ABSOLUTE_EMPTY_DIRECTORY
```

The generator refuses to overwrite files. The guest Rust test compares all five
committed byte arrays with freshly encoded output using the same settings.
Native tests read these exact files; generated test output never replaces them.

| File | Input pixels | SHA-256 |
|---|---|---|
| baseline.jpg | 1x1 RGB (255,0,0), baseline | `1642053c90501bf99557adc3924ca0ce1b78e8a7127b04d8dc046074f44e0572` |
| progressive.jpg | 1x1 RGB (255,0,0), progressive | `38303fb97bcf9834fbee0e4cc8597e01ebb10ff7dc0b925fdf0c3ee1a3835e62` |
| grayscale.jpg | 1x1 gray 80, baseline | `4289bb3680b6c8c943396ee8f767b0d09110d8f77ad99649e52eb52ce1bf3be7` |
| cmyk.jpg | 1x1 CMYK (0,255,255,0), baseline | `f9915aa8c0e94075da5c925daadbe9b46e1f0af867ad5730e1dcd6c3faca3e7f` |
| too-wide.jpg | 1025x1 gray 80, baseline, deliberate dimension rejection | `30178c4f96113aac24fb76a7c10c3826900534357485e85cca81fb5529102778` |
