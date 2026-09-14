//! Generate owned JPEG test assets from constant pixels, without any decoder.
#![forbid(unsafe_code)]

use jpeg_encoder::{ColorType, Encoder};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("one existing fixture directory required".into());
    }
    let directory = PathBuf::from(&args[0]);
    if !directory.is_dir() {
        return Err("existing fixture directory required".into());
    }
    for (name, color, pixels, progressive, width) in [
        ("baseline", ColorType::Rgb, vec![255, 0, 0], false, 1u16),
        ("progressive", ColorType::Rgb, vec![255, 0, 0], true, 1),
        ("grayscale", ColorType::Luma, vec![80], false, 1),
        ("cmyk", ColorType::Cmyk, vec![0, 255, 255, 0], false, 1),
        ("too-wide", ColorType::Luma, vec![80; 1025], false, 1025),
    ] {
        let mut bytes = vec![];
        let mut encoder = Encoder::new(&mut bytes, 100);
        encoder.set_progressive(progressive);
        encoder.encode(&pixels, width, 1, color)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.jpg")))?;
        file.write_all(&bytes)?;
        println!("{name}.jpg: {} bytes", bytes.len());
    }
    let mut bytes = vec![];
    let mut encoder = Encoder::new(&mut bytes, 100);
    encoder.set_progressive(true);
    encoder.encode(&[230, 40, 25].repeat(64), 8, 8, ColorType::Rgb)?;
    let position = 785 % bytes.len();
    bytes[position] = 34;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("minimized-progressive.jpg"))?;
    file.write_all(&bytes)?;
    println!("minimized-progressive.jpg: {} bytes", bytes.len());
    Ok(())
}
