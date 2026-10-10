//! Freeze the real source wordfilter output for a bounded, synthetic /i/ lease.
//! This helper reads one JSON request and never connects to application services.
#![forbid(unsafe_code)]

use std::io::{Read, Write};

use board_domain::comment_markup::MarkupPolicy;
use board_domain::filtered_formatting;
use board_domain::formatting;
use board_domain::wordfilter::Profile;
use board_domain::wordfiltered_comment;

const MAX_INPUT_BYTES: usize = 4096;
const MAX_COMMENT_BYTES: usize = 256;
const ERROR: &str = "Drawing lease formatter rejected input.";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    comment: String,
    word_filter_enabled: bool,
    word_filter_profile: i16,
    comment_spoiler_cleanup: bool,
    comment_code_spacing: bool,
    comment_sjis_spacing: bool,
    op_markup: bool,
    op: bool,
}

fn run() -> Result<(), ()> {
    // One extra byte distinguishes an over-limit request without consuming
    // unbounded input. Serde rejects missing, duplicate and unknown fields.
    let mut input = Vec::new();
    let mut reader = std::io::stdin().lock().take((MAX_INPUT_BYTES + 1) as u64);
    reader.read_to_end(&mut input).map_err(|_| ())?;
    if input.len() > MAX_INPUT_BYTES {
        return Err(());
    }

    let request: Request = serde_json::from_slice(&input).map_err(|_| ())?;
    if !request.word_filter_enabled || request.word_filter_profile != 0 {
        return Err(());
    }
    let comment = request.comment.as_bytes();
    if comment.is_empty()
        || comment.len() > MAX_COMMENT_BYTES
        || !comment
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b' ' | b'/' | b'-'))
    {
        return Err(());
    }

    // Match the locked board flags and the OP-only markup policy of write.rs.
    let policy = MarkupPolicy {
        spoilers: request.comment_spoiler_cleanup,
        code: request.comment_code_spacing,
        sjis: request.comment_sjis_spacing,
        op: request.op && request.op_markup,
    };
    let mut prepared =
        wordfiltered_comment::prepare(&request.comment, policy, Profile::Global, None)
            .map_err(|_| ())?;
    prepared.freeze_format("i");
    let lines = filtered_formatting::lines(&prepared, "i");
    if filtered_formatting::source_projection(&lines) != request.comment
        || formatting::plain_text(&lines) != request.comment
    {
        return Err(());
    }

    let payload = prepared.encode().map_err(|_| ())?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = Vec::with_capacity(payload.len() * 2 + 1);
    for byte in payload {
        output.push(HEX[usize::from(byte >> 4)]);
        output.push(HEX[usize::from(byte & 15)]);
    }
    output.push(b'\n');
    std::io::stdout().lock().write_all(&output).map_err(|_| ())
}

fn main() {
    if run().is_err() {
        eprintln!("{ERROR}");
        std::process::exit(1);
    }
}
