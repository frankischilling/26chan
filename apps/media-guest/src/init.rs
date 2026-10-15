use std::{
    fs, io,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Command, Stdio},
};

pub fn main() {
    // Never mount filesystems or power off a normal host invocation.
    if rustix::process::getpid() != rustix::process::Pid::INIT {
        eprintln!("guest init must be process 1");
        std::process::exit(1);
    }
    if boot().is_err() {
        eprintln!("guest initialization or decoding failed");
    }
    // Firecracker exits when the guest requests the keyboard-controller reset
    // selected by reboot=k. A generic ACPI poweroff can leave this guest halted.
    let _ = rustix::system::reboot(rustix::system::RebootCommand::Restart);
    loop {
        std::thread::park();
    }
}

fn boot() -> io::Result<()> {
    use rustix::{
        mount::{MountFlags, mount},
        process::{Resource, Rlimit, setrlimit},
    };
    // The kernel's initial rootfs mode is not an artifact permission contract.
    // Make the root explicitly unwritable before starting the worker identity.
    fs::set_permissions("/", fs::Permissions::from_mode(0o755))?;
    for (path, kind) in [("/dev", "devtmpfs"), ("/proc", "proc"), ("/sys", "sysfs")] {
        fs::create_dir_all(path)?;
        mount(
            kind,
            path,
            kind,
            MountFlags::NOSUID | MountFlags::NOEXEC,
            None,
        )?;
    }
    let kind = board_media_guest::paired::kernel_input_kind(&fs::read_to_string("/proc/cmdline")?)?;
    let pdf = kind == board_media_guest::paired::InputKind::PdfV4;
    let video = kind == board_media_guest::paired::InputKind::VideoV5;
    let still = matches!(
        kind,
        board_media_guest::paired::InputKind::StillV6 | board_media_guest::paired::InputKind::GifV7
    );
    if pdf || video || still {
        fs::create_dir_all("/tmp")?;
        mount(
            "tmpfs",
            "/tmp",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            Some(if video || still {
                c"size=24m,mode=0700,uid=1000,gid=1000"
            } else {
                c"size=16m,mode=0700,uid=1000,gid=1000"
            }),
        )?;
    }
    fs::set_permissions("/dev/vda", fs::Permissions::from_mode(0o444))?;
    fs::set_permissions("/dev/vdb", fs::Permissions::from_mode(0o666))?;
    for (resource, maximum) in [
        (Resource::Core, 0),
        (Resource::Cpu, if still { 20 } else { 5 }),
        (
            Resource::As,
            if still {
                1536 * 1024 * 1024
            } else {
                96 * 1024 * 1024
            },
        ),
        (Resource::Nproc, 16),
        (Resource::Nofile, 32),
        // Ghostscript may spool its bounded stdin to the private tmpfs. The
        // output disk remains independently fixed by the broker and validator.
        (
            Resource::Fsize,
            if still {
                10 * 1024 * 1024
            } else if pdf || video {
                8 * 1024 * 1024
            } else {
                kind.output_bytes()
            },
        ),
    ] {
        setrlimit(
            resource,
            Rlimit {
                current: Some(maximum),
                maximum: Some(maximum),
            },
        )?;
    }
    rustix::thread::set_no_new_privs(true)?;
    let status = Command::new("/worker")
        .arg(kind.argument())
        .env_clear()
        .uid(1000)
        .gid(1000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(io::Error::other("decoder rejected input"));
    }
    Ok(())
}
