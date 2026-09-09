#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
fn probe() -> std::io::Result<()> {
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        net::{SocketAddr, TcpStream},
        process::{Command, Stdio},
        time::Duration,
    };
    if std::env::args().nth(1).as_deref() == Some("--child") {
        std::thread::sleep(Duration::from_secs(10));
        return Ok(());
    }
    let mut input = File::open("/dev/vda")?;
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);
    if length > 4096 {
        return Err(std::io::Error::other("probe input too large"));
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    let text = std::str::from_utf8(&bytes).map_err(std::io::Error::other)?;
    let mut lines = text.lines();
    let mut checks = Vec::new();
    match lines.next() {
        Some("inspect") => {
            checks.push(rustix::process::getuid().as_raw() == 1000);
            checks.push(std::env::vars_os().next().is_none());
            checks.push(fs::read_dir("/sys/class/net")?.count() == 1);
            let status = fs::read_to_string("/proc/self/status")?;
            checks.push(
                status
                    .lines()
                    .any(|line| line == "CapEff:\t0000000000000000"),
            );
            checks.push(status.lines().any(|line| line == "NoNewPrivs:\t1"));
            checks.push(
                [
                    "/dev/kvm",
                    "/dev/vdc",
                    "/run/docker.sock",
                    "/var/run/docker.sock",
                    "/run/26chan-media-jobs",
                    "/etc/26chan",
                    "/host",
                    "/other-job",
                ]
                .iter()
                .all(|path| File::open(path).is_err()),
            );
            checks.push(OpenOptions::new().write(true).open("/dev/vda").is_err());
            checks.push(File::create("/unauthorized-storage").is_err());
            checks.push((0..=2).all(|fd| {
                fs::read_link(format!("/proc/self/fd/{fd}"))
                    .is_ok_and(|path| path == std::path::Path::new("/dev/null"))
            }));
            for address in lines {
                let endpoint: SocketAddr = address.parse().map_err(std::io::Error::other)?;
                checks.push(
                    TcpStream::connect_timeout(&endpoint, Duration::from_millis(200)).is_err(),
                );
            }
        }
        Some("memory") => {
            let mut bounded = Vec::<u8>::new();
            checks.push(bounded.try_reserve_exact(128 * 1024 * 1024).is_err());
        }
        Some("process") => {
            let mut children = Vec::new();
            let mut denied = false;
            for _ in 0..20 {
                match Command::new("/worker")
                    .arg("--child")
                    .env_clear()
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => children.push(child),
                    Err(error) => {
                        denied = error.raw_os_error() == Some(11);
                        break;
                    }
                }
            }
            checks.push(!children.is_empty() && denied && children.len() < 16);
            for mut child in children {
                child.kill()?;
                child.wait()?;
            }
        }
        Some("disk") => {
            let mut output = OpenOptions::new().write(true).open("/dev/vdb")?;
            output.seek(SeekFrom::Start(4_194_816))?;
            checks.push(output.write(&[1]).is_err());
        }
        Some("sleep") => {
            std::thread::sleep(Duration::from_secs(25));
            return Ok(());
        }
        Some("cpu") => {
            // Bounded even if the CPU limit regresses: the host enforces 15s and
            // the probe independently stops after 8s of wall-clock time.
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(8) {
                std::hint::black_box(42u64.wrapping_mul(17));
            }
            return Ok(());
        }
        _ => return Err(std::io::Error::other("unknown probe")),
    }
    let mut output = OpenOptions::new().write(true).open("/dev/vdb")?;
    output.write_all(b"IBRGBA01")?;
    output.write_all(&(checks.len() as u32).to_be_bytes())?;
    output.write_all(&1u32.to_be_bytes())?;
    for check in checks {
        output.write_all(&[
            if check { 0 } else { 255 },
            if check { 255 } else { 0 },
            0,
            255,
        ])?;
    }
    output.sync_all()?;
    Ok(())
}

fn main() {
    #[cfg(target_os = "linux")]
    if probe().is_ok() {
        return;
    }
    std::process::exit(1);
}
