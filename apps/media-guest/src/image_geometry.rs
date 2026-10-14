//! Geometry from the pinned image pipeline. These helpers do not admit a file,
//! select board policy, decode an image, or grant publication authority.
use std::io;

fn dimensions((width, height): (u32, u32), maximum: u32) -> io::Result<()> {
    if (1..=maximum).contains(&width) && (1..=maximum).contains(&height) {
        Ok(())
    } else {
        Err(io::Error::other("image geometry outside fixed limits"))
    }
}

/// Source thumbnail arithmetic uses floating-point scale followed by floor.
/// A zero-sized result is rejected rather than silently clamped or enlarged.
pub fn thumbnail(size: (u32, u32), bounds: (u32, u32)) -> io::Result<(u32, u32)> {
    dimensions(size, 10_000)?;
    dimensions(bounds, 250)?;
    let output = if size.0 > bounds.0 || size.1 > bounds.1 {
        let scale =
            (f64::from(bounds.0) / f64::from(size.0)).min(f64::from(bounds.1) / f64::from(size.1));
        (
            (f64::from(size.0) * scale).floor() as u32,
            (f64::from(size.1) * scale).floor() as u32,
        )
    } else {
        size
    };
    dimensions(output, 250)?;
    Ok(output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StillFormat {
    Png,
    Jpeg,
}

/// Called only when the authenticated board policy enables mobile resizing.
/// `processed_bytes` describes the accepted file after its cleanup policy.
pub fn mobile(
    format: StillFormat,
    size: (u32, u32),
    processed_bytes: u64,
) -> io::Result<Option<(u32, u32)>> {
    dimensions(size, 10_000)?;
    if processed_bytes == 0 || processed_bytes > 10 * 1024 * 1024 {
        return Err(io::Error::other("image byte length outside fixed limits"));
    }
    if format == StillFormat::Png && processed_bytes <= 524_288 {
        return Ok(None);
    }
    if (size.0 <= 1024 && size.1 <= 1024) || u64::from(size.0) * u64::from(size.1) <= 524_288 {
        return Ok(None);
    }
    let ratio = f64::from(size.0) / f64::from(size.1);
    let output = if ratio > 1.0 {
        (1024, (1024.0 / ratio).round() as u32)
    } else {
        ((1024.0 * ratio).round() as u32, 1024)
    };
    dimensions(output, 1024)?;
    Ok(Some(output))
}

/// Backgrounds used before resampling transparent PNG pixels into a JPEG.
pub fn thumbnail_background(blue: bool, thread: bool) -> [u8; 3] {
    match (blue, thread) {
        (false, true) => [0xff, 0xff, 0xee],
        (false, false) => [0xf0, 0xe0, 0xd6],
        (true, true) => [0xee, 0xf2, 0xff],
        (true, false) => [0xd6, 0xda, 0xf0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_floor_and_no_upscale() {
        assert_eq!(thumbnail((640, 480), (250, 250)).unwrap(), (250, 187));
        assert_eq!(thumbnail((640, 480), (125, 125)).unwrap(), (125, 93));
        assert_eq!(thumbnail((640, 480), (200, 200)).unwrap(), (200, 150));
        assert_eq!(thumbnail((32, 20), (250, 250)).unwrap(), (32, 20));
        assert_eq!(thumbnail((800, 600), (200, 100)).unwrap(), (133, 100));
        assert!(thumbnail((1, 10_000), (250, 250)).is_err());
        assert!(thumbnail((10_000, 1), (125, 125)).is_err());
    }

    #[test]
    fn mobile_thresholds_and_positive_half_rounding() {
        assert_eq!(mobile(StillFormat::Jpeg, (1024, 512), 200).unwrap(), None);
        assert_eq!(mobile(StillFormat::Jpeg, (2048, 256), 200).unwrap(), None);
        assert_eq!(
            mobile(StillFormat::Png, (2048, 1024), 524_288).unwrap(),
            None
        );
        assert_eq!(
            mobile(StillFormat::Png, (2048, 1024), 524_289).unwrap(),
            Some((1024, 512))
        );
        assert_eq!(
            mobile(StillFormat::Jpeg, (2048, 257), 200).unwrap(),
            Some((1024, 129))
        );
        assert_eq!(
            mobile(StillFormat::Jpeg, (257, 2048), 200).unwrap(),
            Some((129, 1024))
        );
        assert_eq!(
            mobile(StillFormat::Jpeg, (10_000, 10_000), 200).unwrap(),
            Some((1024, 1024))
        );
    }

    #[test]
    fn invalid_geometry_and_lengths_are_rejected() {
        for size in [(0, 1), (1, 0), (10_001, 1), (1, 10_001)] {
            assert!(thumbnail(size, (250, 250)).is_err());
            assert!(mobile(StillFormat::Jpeg, size, 200).is_err());
        }
        assert!(thumbnail((1, 1), (251, 250)).is_err());
        assert!(thumbnail((1, 1), (0, 250)).is_err());
        for length in [0, 10 * 1024 * 1024 + 1, u64::MAX] {
            assert!(mobile(StillFormat::Png, (2048, 1024), length).is_err());
        }
    }

    #[test]
    fn source_background_colors() {
        assert_eq!(thumbnail_background(false, true), [255, 255, 238]);
        assert_eq!(thumbnail_background(false, false), [240, 224, 214]);
        assert_eq!(thumbnail_background(true, true), [238, 242, 255]);
        assert_eq!(thumbnail_background(true, false), [214, 218, 240]);
    }
}
