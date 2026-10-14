//! Fixed lossless JPEG metadata cleanup inside the disposable guest only.
use super::{MAX_INPUT_BYTES, rejected};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Files {
    output_owned: bool,
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/still-input.jpg");
        if self.output_owned {
            let _ = fs::remove_file("/tmp/still-output.jpg");
        }
    }
}

pub(super) fn strip(input: &[u8]) -> io::Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("/tmp/still-input.jpg")?;
    let mut files = Files {
        output_owned: false,
    };
    file.write_all(input)?;
    drop(file);
    // Refuse a pre-existing output even in the disposable, single-job tmpfs.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("/tmp/still-output.jpg")?;
    files.output_owned = true;
    let mut child = Command::new(Path::new("/usr/bin/board-jpegtran"))
        .args([
            "-strict",
            "-copy",
            "none",
            "-maxmemory",
            "1048576",
            "-maxscans",
            "64",
            "-outfile",
            "/tmp/still-output.jpg",
            "/tmp/still-input.jpg",
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(rejected());
            }
        }
    }
    let mut bytes = Vec::new();
    fs::File::open("/tmp/still-output.jpg")?
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_INPUT_BYTES
        || !bytes.starts_with(b"\xff\xd8\xff")
        || !bytes.ends_with(b"\xff\xd9")
    {
        return Err(rejected());
    }
    Ok(bytes)
}
