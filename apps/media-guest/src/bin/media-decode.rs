#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
fn decode() -> std::io::Result<()> {
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
    };
    if rustix::process::getuid().as_raw() != 1000 {
        return Err(std::io::Error::other("decoder identity rejected"));
    }
    let mut input = File::open("/dev/vda")?;
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    let size = u64::from_be_bytes(header);
    if !(1..=8 * 1024 * 1024).contains(&size) {
        return Err(std::io::Error::other("input size rejected"));
    }
    let mut bytes = vec![0; size as usize];
    input.read_exact(&mut bytes)?;
    let pixels = board_media_guest::decode_image(&bytes)?;
    let mut output = OpenOptions::new().write(true).open("/dev/vdb")?;
    output.write_all(&pixels)?;
    output.sync_all()?;
    Ok(())
}

fn main() {
    #[cfg(target_os = "linux")]
    if decode().is_ok() {
        return;
    }
    std::process::exit(1);
}
