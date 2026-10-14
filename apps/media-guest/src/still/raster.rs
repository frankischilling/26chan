use std::io;

/// Fully decoded, unrotated pixels kept inside the isolated worker.
pub(super) struct Raster {
    pub width: u32,
    pub height: u32,
    channels: usize,
    pixels: Vec<u8>,
}

impl Raster {
    pub fn new(width: u32, height: u32, channels: usize, pixels: Vec<u8>) -> io::Result<Self> {
        if !(1..=10_000).contains(&width)
            || !(1..=10_000).contains(&height)
            || !matches!(channels, 1..=4)
            || pixels.len() != width as usize * height as usize * channels
        {
            return Err(io::Error::other("decoded image geometry rejected"));
        }
        Ok(Self {
            width,
            height,
            channels,
            pixels,
        })
    }

    fn rgba(&self, x: u32, y: u32) -> [u8; 4] {
        let offset = (y as usize * self.width as usize + x as usize) * self.channels;
        let pixel = &self.pixels[offset..offset + self.channels];
        match self.channels {
            1 => [pixel[0], pixel[0], pixel[0], 255],
            2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
            3 => [pixel[0], pixel[1], pixel[2], 255],
            4 => [pixel[0], pixel[1], pixel[2], pixel[3]],
            _ => unreachable!("constructor validates channels"),
        }
    }

    /// Area-weighted RGB resampling with alpha composited over a fixed backdrop.
    /// No EXIF orientation, ICC conversion, or gamma correction is applied here.
    pub fn resize(&self, width: u32, height: u32, background: [u8; 3]) -> io::Result<Vec<u8>> {
        if !(1..=1024).contains(&width)
            || !(1..=1024).contains(&height)
            || width > self.width
            || height > self.height
        {
            return Err(io::Error::other("image derivative geometry rejected"));
        }
        let mut output = Vec::with_capacity(width as usize * height as usize * 3);
        let scale_x = f64::from(self.width) / f64::from(width);
        let scale_y = f64::from(self.height) / f64::from(height);
        for y in 0..height {
            let top = f64::from(y) * scale_y;
            let bottom = f64::from(y + 1) * scale_y;
            for x in 0..width {
                let left = f64::from(x) * scale_x;
                let right = f64::from(x + 1) * scale_x;
                let mut color = [0.0; 3];
                let mut coverage = 0.0;
                for sy in top.floor() as u32..(bottom.ceil() as u32).min(self.height) {
                    let vertical =
                        (bottom.min(f64::from(sy + 1)) - top.max(f64::from(sy))).max(0.0);
                    for sx in left.floor() as u32..(right.ceil() as u32).min(self.width) {
                        let horizontal =
                            (right.min(f64::from(sx + 1)) - left.max(f64::from(sx))).max(0.0);
                        let weight = horizontal * vertical;
                        let pixel = self.rgba(sx, sy);
                        let alpha = f64::from(pixel[3]) / 255.0;
                        for channel in 0..3 {
                            color[channel] += weight
                                * (f64::from(pixel[channel]) * alpha
                                    + f64::from(background[channel]) * (1.0 - alpha));
                        }
                        coverage += weight;
                    }
                }
                if !coverage.is_finite() || coverage <= 0.0 {
                    return Err(io::Error::other("image derivative coverage rejected"));
                }
                for value in color {
                    output.push((value / coverage).round().clamp(0.0, 255.0) as u8);
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_rgb_does_not_bleed_into_background() {
        let raster = Raster::new(2, 1, 4, vec![255, 0, 0, 0, 0, 0, 0, 255]).unwrap();
        assert_eq!(
            raster.resize(1, 1, [240, 224, 214]).unwrap(),
            [120, 112, 107]
        );
    }

    #[test]
    fn fractional_source_pixels_contribute_by_coverage() {
        let raster = Raster::new(3, 1, 3, vec![0, 0, 0, 255, 255, 255, 0, 0, 0]).unwrap();
        assert_eq!(raster.resize(2, 1, [0, 0, 0]).unwrap(), [85; 6]);
    }

    #[test]
    fn identity_preserves_rgb_and_expands_gray() {
        let raster = Raster::new(2, 1, 3, vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(raster.resize(2, 1, [255; 3]).unwrap(), [1, 2, 3, 4, 5, 6]);
        let raster = Raster::new(2, 1, 1, vec![10, 240]).unwrap();
        assert_eq!(
            raster.resize(2, 1, [0; 3]).unwrap(),
            [10, 10, 10, 240, 240, 240]
        );
    }

    #[test]
    fn mismatched_buffers_and_unbounded_derivatives_reject() {
        assert!(Raster::new(2, 1, 3, vec![1, 2, 3]).is_err());
        assert!(Raster::new(0, 1, 3, vec![]).is_err());
        assert!(Raster::new(1, 1, 5, vec![0; 5]).is_err());
        let raster = Raster::new(2, 1, 3, vec![0; 6]).unwrap();
        assert!(raster.resize(3, 1, [0; 3]).is_err());
        assert!(raster.resize(0, 1, [0; 3]).is_err());
    }
}
