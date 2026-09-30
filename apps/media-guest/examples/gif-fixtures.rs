//! Generate owned GIF fixtures from constant indexed pixels, without a decoder.
#![forbid(unsafe_code)]

use std::{borrow::Cow, fs::OpenOptions, io::Write, path::PathBuf};

fn frame(width: u16, height: u16) -> gif::Frame<'static> {
    gif::Frame {
        width,
        height,
        buffer: Cow::Owned(vec![0; usize::from(width) * usize::from(height)]),
        ..Default::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("one existing fixture directory required".into());
    }
    let directory = PathBuf::from(&args[0]);
    if !directory.is_dir() {
        return Err("existing fixture directory required".into());
    }
    let transparent = gif::Frame {
        width: 2,
        height: 1,
        transparent: Some(1),
        buffer: Cow::Borrowed(&[0, 1]),
        ..Default::default()
    };
    let interlaced = gif::Frame {
        width: 2,
        height: 8,
        interlaced: true,
        buffer: Cow::Owned(
            [0, 4, 2, 6, 1, 3, 5, 7]
                .into_iter()
                .flat_map(|row| [row % 2; 2])
                .collect(),
        ),
        ..Default::default()
    };
    for (name, width, height, frames) in [
        ("static", 1, 1, vec![frame(1, 1)]),
        ("transparent", 2, 1, vec![transparent]),
        ("interlaced", 2, 8, vec![interlaced]),
        ("animated", 1, 1, vec![frame(1, 1), frame(1, 1)]),
        ("partial", 2, 1, vec![frame(1, 1)]),
        ("too-wide", 1025, 1, vec![frame(1025, 1)]),
    ] {
        let mut encoder = gif::Encoder::new(Vec::new(), width, height, &[255, 0, 0, 0, 0, 255])?;
        for frame in frames {
            encoder.write_frame(&frame)?;
        }
        let bytes = encoder.into_inner()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.gif")))?;
        file.write_all(&bytes)?;
        println!("{name}.gif: {} bytes", bytes.len());
    }
    Ok(())
}
