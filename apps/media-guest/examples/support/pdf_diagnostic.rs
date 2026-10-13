//! Diagnostics use only the compiled-in owned fixture, never an uploaded PDF.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    process::{Command, Stdio},
};

const DOCUMENT: &[u8] = include_bytes!("../../../../tests/media/fixtures/pdf/single-page.pdf");

pub fn run() -> io::Result<()> {
    let mut report = format!("uid={}\n", rustix::process::getuid().as_raw());
    report.push_str(&format!(
        "tmp_write={}\n",
        fs::write("/tmp/owned-probe", b"owned").is_ok()
    ));
    for (name, device) in [
        ("version", vec!["--version"]),
        ("nullpage", vec!["-sDEVICE=nullpage", "-"]),
        (
            "jpeg",
            vec![
                "-sDEVICE=jpeg",
                "-dFirstPage=1",
                "-dLastPage=1",
                "-sOutputFile=%stdout",
                "-",
            ],
        ),
    ] {
        let child = Command::new("/usr/bin/gs")
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
            .env_clear()
            .env("GS_FONTPATH", "/usr/share/fonts/type1/urw-base35")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match child {
            Ok(mut child) => {
                if name != "version" {
                    let written = child.stdin.take().unwrap().write_all(DOCUMENT);
                    report.push_str(&format!("{name}_stdin={written:?}\n"));
                } else {
                    drop(child.stdin.take());
                }
                let output = child.wait_with_output()?;
                report.push_str(&format!(
                    "{name}: success={} status={} stdout_bytes={} stderr={}\n",
                    output.status.success(),
                    output.status,
                    output.stdout.len(),
                    String::from_utf8_lossy(&output.stderr[..output.stderr.len().min(1024)])
                ));
            }
            Err(error) => report.push_str(&format!("{name}: spawn={error}\n")),
        }
    }
    let decoded = board_media_guest::pdf::decode(DOCUMENT);
    report.push_str(&format!(
        "decode: {}\n",
        match decoded {
            Ok(bytes) => format!("ok {} bytes", bytes.len()),
            Err(error) => format!("{error:?}"),
        }
    ));
    let bytes = report.as_bytes();
    let bytes = &bytes[..bytes.len().min(4096)];
    let mut output = OpenOptions::new().write(true).open("/dev/vdb")?;
    output.write_all(b"IBPDFDBG")?;
    output.write_all(&(bytes.len() as u32).to_be_bytes())?;
    output.write_all(bytes)?;
    output.sync_all()
}
