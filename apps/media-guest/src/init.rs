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
    fs::set_permissions("/dev/vda", fs::Permissions::from_mode(0o444))?;
    fs::set_permissions("/dev/vdb", fs::Permissions::from_mode(0o666))?;
    for (resource, maximum) in [
        (Resource::Core, 0),
        (Resource::Cpu, 5),
        (Resource::As, 96 * 1024 * 1024),
        (Resource::Nproc, 16),
        (Resource::Nofile, 32),
        (Resource::Fsize, 4_194_816),
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
