//! PDF parsing belongs inside the disposable guest, never a credentialed service.
//! Tests may invoke the fixed renderer only with their own synthetic documents.
use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_INPUT: usize = 8 * 1024 * 1024;
const MAX_PREVIEW: usize = 5 * 1024 * 1024;
const RENDERER: &str = "/usr/bin/gs";

fn rejected() -> io::Error {
    io::Error::other("PDF processing rejected")
}

/// Validate the complete document, then render its first page. Both operations
/// share one deadline; the surrounding VM still owns the hard execution limit.
pub fn decode(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.len() > MAX_INPUT || !input.starts_with(b"%PDF-") {
        return Err(rejected());
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    run(input, &["-sDEVICE=nullpage"], 0, deadline)?;
    let image = run(
        input,
        &[
            "-sDEVICE=jpeg",
            "-dFirstPage=1",
            "-dLastPage=1",
            "-sOutputFile=%stdout",
        ],
        MAX_PREVIEW,
        deadline,
    )?;
    // The existing guest JPEG decoder enforces the decoded dimensions and pixel
    // budget. Only its neutral RGBA protocol crosses the VM boundary.
    crate::decode_image(&image)
}

fn run(input: &[u8], device: &[&str], maximum: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    if Instant::now() >= deadline {
        return Err(rejected());
    }
    let mut child = Command::new(RENDERER)
        .args([
            "-q",
            "-dSAFER",
            "-dNOPAUSE",
            "-dNOPROMPT",
            "-dBATCH",
            "-dPDFSTOPONERROR",
            "-dPDFSTOPONWARNING",
            "-sICCProfilesDir=/usr/share/color/icc/ghostscript/",
        ])
        .args(device)
        .arg("-")
        .env_clear()
        .env("GS_FONTPATH", "/usr/share/fonts/type1/urw-base35")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(rejected)?;
    let stdout = child.stdout.take().ok_or_else(rejected)?;
    thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(input));
        let reader = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > maximum {
                return Err(rejected());
            }
            Ok::<_, io::Error>(bytes)
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(rejected());
                }
            }
        };
        let written = writer.join().map_err(|_| rejected())?;
        let output = reader.join().map_err(|_| rejected())?;
        if !status?.success() {
            return Err(rejected());
        }
        written?;
        output
    })
}

/// Exact private block framing, including zero sector padding and EOF.
pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    let size = u64::from_be_bytes(header);
    if !(1..=MAX_INPUT as u64).contains(&size) {
        return Err(rejected());
    }
    let mut bytes = vec![0; size as usize];
    input.read_exact(&mut bytes)?;
    let padding_size = (512 - (8 + size as usize) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_size])?;
    if padding[..padding_size].iter().any(|&value| value != 0) || input.read(&mut [0; 1])? != 0 {
        return Err(rejected());
    }
    decode(&bytes)
}
