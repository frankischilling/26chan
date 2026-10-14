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
        // GD loads PNG opacity into seven-bit inverse alpha. Its resampler
        // rounds the averaged alpha before blending into the opaque backdrop.
        for y in 0..height {
            let top = y as f32 * self.height as f32 / height as f32;
            let bottom = (y + 1) as f32 * self.height as f32 / height as f32;
            for x in 0..width {
                let left = x as f32 * self.width as f32 / width as f32;
                let right = (x + 1) as f32 * self.width as f32 / width as f32;
                let mut color = [0.0f32; 3];
                let mut alpha = 0.0f32;
                let mut opacity = 0.0f32;
                let mut coverage = 0.0f32;
                for sy in top.floor() as u32..(bottom.ceil() as u32).min(self.height) {
                    let vertical = (bottom.min((sy + 1) as f32) - top.max(sy as f32)).max(0.0);
                    for sx in left.floor() as u32..(right.ceil() as u32).min(self.width) {
                        let horizontal =
                            (right.min((sx + 1) as f32) - left.max(sx as f32)).max(0.0);
                        let weight = horizontal * vertical;
                        let pixel = self.rgba(sx, sy);
                        let inverse_alpha = f32::from(127 - (pixel[3] >> 1));
                        let opaque_weight = (127.0 - inverse_alpha) * weight;
                        for channel in 0..3 {
                            color[channel] += f32::from(pixel[channel]) * opaque_weight;
                        }
                        alpha += inverse_alpha * weight;
                        opacity += opaque_weight;
                        coverage += weight;
                    }
                }
                if !coverage.is_finite() || coverage <= 0.0 {
                    return Err(io::Error::other("image derivative coverage rejected"));
                }
                for value in &mut color {
                    *value /= coverage;
                }
                alpha /= coverage;
                if opacity != 0.0 {
                    opacity /= coverage;
                    for value in &mut color {
                        *value /= opacity;
                    }
                }
                let inverse_alpha = (alpha + 0.5).min(127.0) as u32;
                for (channel, value) in color.into_iter().enumerate() {
                    let source = (value + 0.5).min(255.0) as u32;
                    let blended = (source * (127 - inverse_alpha)
                        + u32::from(background[channel]) * inverse_alpha)
                        / 127;
                    output.push(blended as u8);
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
    fn captured_gd_pixels_cover_alpha_quantization_and_fractional_resampling() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/gd-still-resampling.json"
        ))
        .unwrap();
        assert_eq!(reference["version"], "2.3.3");
        let cases = reference["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 65);
        for (index, case) in cases.iter().enumerate() {
            let width = case["size"][0].as_u64().unwrap() as u32;
            let height = case["size"][1].as_u64().unwrap() as u32;
            let mut pixels = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    let alpha = if case["alpha"] == "mixed" {
                        (x * 19 + y * 37) % 256
                    } else {
                        case["alpha"].as_u64().unwrap() as u32
                    };
                    pixels.extend_from_slice(&[
                        ((x * 17 + y * 23) % 256) as u8,
                        ((x * 31 + y * 11) % 256) as u8,
                        ((x * 53 + y * 7) % 256) as u8,
                        alpha as u8,
                    ]);
                }
            }
            let background = std::array::from_fn(|i| case["background"][i].as_u64().unwrap() as u8);
            let raster = Raster::new(width, height, 4, pixels).unwrap();
            let actual = raster
                .resize(
                    case["output"][0].as_u64().unwrap() as u32,
                    case["output"][1].as_u64().unwrap() as u32,
                    background,
                )
                .unwrap();
            let expected: Vec<u8> = case["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(actual, expected, "GD reference case {index}");
        }
    }

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
