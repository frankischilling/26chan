# Task 1 report: bounded block protocol

## Scope

Implemented the bounded input and output block protocol in the files assigned by
the task brief. No Cargo manifest or lockfile changes belong to this task.

The public API now exports:

- `write_input_disk`, which writes a big-endian u64 length, the exact input,
  and zero padding through the next 512-byte sector boundary;
- `OUTPUT_DISK_BYTES`, fixed at 4,194,816 bytes;
- `ValidatedOutput::read_disk`, which reads the existing `IBRGBA01` header and
  pixels, verifies the rest of the fixed disk is zero, and rejects trailing
  bytes.

Input lengths must be from 1 byte through 8 MiB. The writer rejects short and
long sources with `MediaError::InputLengthMismatch`. A long source is read only
through the declared length plus one byte. Invalid declared lengths are rejected
before reading the source. Writer failures may leave partial output for the
caller to clean up, as required by the task contract.

`ValidatedOutput::read` and `ValidatedOutput::read_disk` share the same private
header, dimension, and pixel parser. Dimensions are checked before pixel
allocation. Disk padding is read in fixed-size chunks, and the parser reads only
one byte beyond the fixed disk size when checking for trailing data.

## TDD evidence

RED was recorded before implementation with:

```text
cargo test -p board-media --test block --locked
```

The test target failed to compile because `OUTPUT_DISK_BYTES`,
`write_input_disk`, `MediaError::InputLengthMismatch`, and
`ValidatedOutput::read_disk` did not exist. This was the expected failure for
the new public behavior.

After implementation, the same command passed all 9 block protocol tests. The
tests cover:

- exact one-byte input framing and the exact 1x1 red RGBA output disk;
- maximum input and 1,024 by 1,024 output framing;
- short and long input length mismatches, including the one-byte lookahead;
- zero and oversized declared input lengths without source reads;
- output truncation in the header, pixels, and padding;
- one trailing byte with no reads beyond that byte;
- nonzero padding at the start and end of the padding region;
- zero, excessive, and `u32::MAX` dimensions with exactly 16 bytes consumed.

## Verification

The final pre-commit verification commands were:

```text
cargo test -p board-media --locked
cargo clippy -p board-media --all-targets --locked -- -D warnings
cargo fmt -p board-media -- --check
```

The package test run passed 28 tests: 1 unit test, 9 block protocol tests, 10
output integration tests, and 8 storage integration tests. Doc tests contained
no tests. Clippy and formatting both exited successfully without diagnostics.

## Review

The implementation uses safe Rust and performs no filesystem mounting or guest
filesystem parsing. Input reads and output allocation are bounded by validated
limits. The exact output size, zero padding, truncation, and trailing-byte checks
match `docs/firecracker-design.md`. Existing stream parsing and PNG promotion
behavior remain covered by the prior media tests.
