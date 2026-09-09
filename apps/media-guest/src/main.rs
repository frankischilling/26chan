#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
mod init;

#[cfg(target_os = "linux")]
fn main() {
    init::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("media guest requires Linux");
    std::process::exit(1);
}
