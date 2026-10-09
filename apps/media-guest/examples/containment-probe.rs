#![forbid(unsafe_code)]

fn probe_text(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> {
    // Only the disposable qualification guest recognizes this exact envelope.
    // It reaches live-VM cancellation through the coordinator's format gate.
    if bytes == b"GIF89a26chan-test-probe:sleep" {
        Ok("sleep")
    } else {
        std::str::from_utf8(bytes)
    }
}

#[cfg(target_os = "linux")]
fn probe() -> std::io::Result<()> {
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        net::{SocketAddr, TcpStream, UdpSocket},
        os::unix::net::UnixStream,
        process::{Command, Stdio},
        time::Duration,
    };
    if std::env::args().nth(1).as_deref() == Some("--child") {
        std::thread::sleep(Duration::from_secs(10));
        return Ok(());
    }
    // This qualification worker has its own tiny command framing. It never
    // replaces production media-decode in decode.json or synthesizes candidates.
    let mode = std::env::args().nth(1).unwrap_or_else(|| "image-v1".into());
    // GIF-v3 and image-v1 use the same tiny length-prefixed probe envelope.
    // The production GIF worker still receives its own trusted mode argument.
    let kind = if mode == "gif-v3" {
        board_media_guest::paired::InputKind::ImageV1
    } else {
        board_media_guest::paired::InputKind::parse(&mode)?
    };
    let mut input = File::open("/dev/vda")?;
    if kind == board_media_guest::paired::InputKind::PairedV2 {
        let mut magic = [0; 8];
        input.read_exact(&mut magic)?;
        if &magic != b"IBJOB002" {
            return Err(std::io::Error::other("probe version rejected"));
        }
    }
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);
    if length > 4096 {
        return Err(std::io::Error::other("probe input too large"));
    }
    if kind == board_media_guest::paired::InputKind::PairedV2 {
        let mut binding = [0; 32];
        input.read_exact(&mut binding)?;
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    let text = probe_text(&bytes).map_err(std::io::Error::other)?;
    let text = if kind == board_media_guest::paired::InputKind::PairedV2 {
        text.trim_end()
    } else {
        text
    };
    let mut lines = text.lines();
    let mut checks = Vec::new();
    match lines.next() {
        Some("boundaries") => {
            for line in lines {
                if checks.len() == 32 {
                    return Err(std::io::Error::other("too many boundary checks"));
                }
                let (operation, target) = line
                    .split_once('\t')
                    .ok_or_else(|| std::io::Error::other("invalid boundary check"))?;
                let denied = match operation {
                    "tcp" | "udp" => {
                        let endpoint: SocketAddr = target.parse().map_err(std::io::Error::other)?;
                        if operation == "tcp" {
                            TcpStream::connect_timeout(&endpoint, Duration::from_millis(200))
                                .is_err()
                        } else {
                            // A reply proves access even if it is malformed. The allowed
                            // host control separately requires the exact DNS response.
                            let exchange = || -> std::io::Result<()> {
                                let bind = if endpoint.is_ipv4() {
                                    "0.0.0.0:0"
                                } else {
                                    "[::]:0"
                                };
                                let socket = UdpSocket::bind(bind)?;
                                socket.set_read_timeout(Some(Duration::from_millis(200)))?;
                                socket.set_write_timeout(Some(Duration::from_millis(200)))?;
                                socket.connect(endpoint)?;
                                socket.send(b"\x01\x02\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07witness\x07invalid\x00\x00\x01\x00\x01")?;
                                socket.recv(&mut [0; 512])?;
                                Ok(())
                            };
                            exchange().is_err()
                        }
                    }
                    "read" | "write" | "unix" => {
                        if !std::path::Path::new(target).is_absolute() {
                            return Err(std::io::Error::other("boundary path is not absolute"));
                        }
                        match operation {
                            "read" => File::open(target).is_err(),
                            "write" => OpenOptions::new().write(true).open(target).is_err(),
                            _ => UnixStream::connect(target).is_err(),
                        }
                    }
                    _ => return Err(std::io::Error::other("unknown boundary operation")),
                };
                checks.push(denied);
            }
            if checks.is_empty() {
                return Err(std::io::Error::other("empty boundary checks"));
            }
        }
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
            output.seek(SeekFrom::Start(kind.output_bytes()))?;
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

#[test]
fn cancellation_envelope_is_exact_and_keeps_plain_probe_commands() {
    assert_eq!(
        probe_text(b"GIF89a26chan-test-probe:sleep").unwrap(),
        "sleep"
    );
    assert_eq!(probe_text(b"sleep").unwrap(), "sleep");
    assert_eq!(probe_text(b"inspect").unwrap(), "inspect");
    assert_eq!(
        probe_text(b"GIF89a26chan-test-probe:sleep-extra").unwrap(),
        "GIF89a26chan-test-probe:sleep-extra"
    );
    assert!(probe_text(&[0xff]).is_err());
}
