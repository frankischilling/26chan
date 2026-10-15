//! Fixed metadata cleanup inside the disposable, credential-free guest.
use super::{MAX_INPUT, rejected};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Files {
    output_owned: bool,
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_file("/tmp/gif-input.gif");
        if self.output_owned {
            let _ = fs::remove_file("/tmp/gif-output.gif");
        }
    }
}

pub(super) fn strip(input: &[u8]) -> io::Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("/tmp/gif-input.gif")?;
    let mut files = Files {
        output_owned: false,
    };
    file.write_all(input)?;
    drop(file);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("/tmp/gif-output.gif")?;
    files.output_owned = true;
    let mut child = Command::new("/usr/bin/board-gifsicle")
        .args([
            "--no-comments",
            "--no-extensions",
            "/tmp/gif-input.gif",
            "-o",
            "/tmp/gif-output.gif",
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
    fs::File::open("/tmp/gif-output.gif")?
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return Err(rejected());
    }
    Ok(bytes)
}
