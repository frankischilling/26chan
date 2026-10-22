//! EXIF extraction runs only inside the disposable still-image guest.
use super::{MAX_EXIF_BYTES, rejected};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct InputFile;
impl Drop for InputFile {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/still-exif.jpg");
    }
}

pub(super) fn extract(input: &[u8]) -> io::Result<String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("/tmp/still-exif.jpg")?;
    let _owned = InputFile;
    file.write_all(input)?;
    drop(file);
    run(
        Path::new("/usr/bin/board-exiftags"),
        Path::new("/tmp/still-exif.jpg"),
    )
}

fn run(binary: &Path, input: &Path) -> io::Result<String> {
    let mut child = Command::new(binary)
        .arg(input)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(rejected)?;
    // Drain concurrently so a full pipe cannot deadlock the deadline loop.
    let overflow = Arc::new(AtomicBool::new(false));
    let reader_overflow = Arc::clone(&overflow);
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_EXIF_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        reader_overflow.store(bytes.len() > MAX_EXIF_BYTES, Ordering::Release);
        Ok::<_, io::Error>(bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline && !overflow.load(Ordering::Acquire) => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) if Instant::now() < deadline && overflow.load(Ordering::Acquire) => {
                // The reader stopped at its byte ceiling. Stop the process
                // rather than allowing more output or computation.
                let _ = child.kill();
                break child.wait().ok();
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().map_err(|_| rejected())??;
    // Exit 1 with empty output is exiftags' ordinary no-EXIF result. Signals,
    // unavailable tools, truncated overflow and partial failed output reject.
    if !status.is_some_and(|s| s.success() || (s.code() == Some(1) && bytes.is_empty()))
        || bytes.len() > MAX_EXIF_BYTES
    {
        return Err(rejected());
    }
    let text = String::from_utf8(bytes).map_err(|_| rejected())?;
    if text.contains('\0') || text.split('\n').count() > 256 {
        return Err(rejected());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // CI and the local qualification invoke this explicitly with the pinned
    // runtime; the normal domain suite must never silently fake extraction.
    #[test]
    #[ignore = "requires the explicitly built pinned EXIF executable"]
    fn owned_runtime_extracts_only_bounded_synthetic_metadata() {
        let runtime = std::env::var_os("EXIF_TEST_RUNTIME").expect("EXIF_TEST_RUNTIME required");
        let binary = Path::new(&runtime).join("exiftags");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/media/fixtures/exif/camera.jpg");
        let actual = run(&binary, &fixture).unwrap();
        assert_eq!(
            actual,
            include_str!("../../../../tests/media/fixtures/exif/camera.txt")
        );
        let without = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/media/fixtures/jpeg/baseline.jpg");
        assert_eq!(run(&binary, &without).unwrap(), "");
        assert!(run(&binary.with_file_name("missing-extractor"), &fixture).is_err());
    }
}
