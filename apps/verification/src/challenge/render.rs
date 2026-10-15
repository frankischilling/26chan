use board_domain::challenge_secrets::GeneratedAnswer;
const WIDTH: usize = 240;
const HEIGHT: usize = 80;
const OFFSET: usize = 80;
const MAX_PNG: usize = 65_536;

pub(crate) struct Image {
    pub foreground: Vec<u8>,
    pub background: Option<Vec<u8>>,
    pub offset_max: u16,
}
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }
    fn range(&mut self, max: u32) -> u32 {
        self.next() % max
    }
}
fn glyph(ch: u8) -> Option<[u8; 7]> {
    Some(match ch {
        b'A' => [14, 17, 17, 31, 17, 17, 17],
        b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'G' => [14, 17, 16, 23, 17, 17, 14],
        b'H' => [17, 17, 17, 31, 17, 17, 17],
        b'J' => [7, 2, 2, 2, 18, 18, 12],
        b'K' => [17, 18, 20, 24, 20, 18, 17],
        b'M' => [17, 27, 21, 21, 17, 17, 17],
        b'N' => [17, 25, 25, 21, 19, 19, 17],
        b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'R' => [30, 17, 17, 30, 20, 18, 17],
        b'S' => [15, 16, 16, 14, 1, 1, 30],
        b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'V' => [17, 17, 17, 17, 17, 10, 4],
        b'W' => [17, 17, 17, 21, 21, 27, 17],
        b'X' => [17, 17, 10, 4, 10, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4],
        b'0' => [14, 17, 19, 21, 25, 17, 14],
        b'2' => [14, 17, 1, 2, 4, 8, 31],
        b'4' => [2, 6, 10, 18, 31, 2, 2],
        b'8' => [14, 17, 17, 14, 17, 17, 14],
        _ => return None,
    })
}
fn paint(pixels: &mut [u8], width: usize, x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < HEIGHT {
        let offset = (y as usize * width + x as usize) * 4;
        pixels[offset..offset + 4].copy_from_slice(&color);
    }
}
fn canvas(width: usize, noise: &mut Noise) -> Vec<u8> {
    let mut pixels = vec![0; width * HEIGHT * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        let shade = 235 + noise.range(20) as u8;
        pixel.copy_from_slice(&[shade, shade, shade.saturating_sub(8), 255]);
    }
    pixels
}
fn encode(width: usize, pixels: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width as u32, HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(pixels).ok()?;
        writer.finish().ok()?;
    }
    (output.len() <= MAX_PNG).then_some(output)
}

/// Fixed-size, self-generated raster output. Render noise is domain-separated
/// from answer generation; it is not used as cryptographic authentication.
pub(crate) fn render(answer: &GeneratedAnswer, seed: [u8; 32], slider: bool) -> Option<Image> {
    let characters = answer.as_str().as_bytes();
    if !(4..=8).contains(&characters.len()) {
        return None;
    }
    let mut noise = Noise(u64::from_be_bytes(seed[..8].try_into().ok()?) | 1);
    let mut foreground = if slider {
        vec![0; WIDTH * HEIGHT * 4]
    } else {
        canvas(WIDTH, &mut noise)
    };
    let background_width = WIDTH + OFFSET;
    let mut background = slider.then(|| canvas(background_width, &mut noise));
    let alignment = 8 + noise.range((OFFSET - 16) as u32) as usize;
    let advance = 32.min(WIDTH / characters.len());
    let start = (WIDTH - characters.len() * advance) / 2;
    let center = if characters.len() > 6 {
        advance / 2
    } else {
        10
    };
    for (index, ch) in characters.iter().enumerate() {
        let pattern = glyph(*ch)?;
        let angle = (noise.range(25) as f32 - 12.0).to_radians();
        let (sin, cos) = angle.sin_cos();
        let origin_x = (start + index * advance + center) as f32;
        let origin_y = 36.0 + noise.range(15) as f32 - 7.0;
        let color = [
            25 + noise.range(45) as u8,
            20 + noise.range(40) as u8,
            35 + noise.range(45) as u8,
            255,
        ];
        for (row, bits) in pattern.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                for dy in 0..5 {
                    for dx in 0..4 {
                        let x = (column * 4 + dx) as f32 - 10.0;
                        let y = (row * 5 + dy) as f32 - 17.0;
                        let px = (origin_x + x * cos - y * sin).round() as i32;
                        let py = (origin_y + x * sin + y * cos).round() as i32;
                        if slider && (row + column) % 3 == 0 {
                            paint(
                                background.as_mut()?,
                                background_width,
                                px + alignment as i32,
                                py,
                                color,
                            );
                        } else {
                            paint(&mut foreground, WIDTH, px, py, color);
                        }
                    }
                }
            }
        }
    }
    // Thin crossing strokes leave the glyph interiors readable. All work and
    // allocations are bounded by the fixed canvases, not request dimensions.
    for _ in 0..5 {
        let y = noise.range(HEIGHT as u32) as f32;
        let slope = (noise.range(31) as f32 - 15.0) / 100.0;
        for x in 0..WIDTH {
            let py = (y + slope * x as f32 + 3.0 * (x as f32 / 19.0).sin()).round() as i32;
            paint(&mut foreground, WIDTH, x as i32, py, [110, 110, 120, 190]);
        }
    }
    Some(Image {
        foreground: encode(WIDTH, &foreground)?,
        background: match background {
            Some(bytes) => Some(encode(background_width, &bytes)?),
            None => None,
        },
        offset_max: if slider { OFFSET as u16 } else { 0 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::challenge_secrets::{TokenKind, VerificationKey, VerificationToken};
    fn check_png(bytes: &[u8], width: u32) {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(bytes.len() <= MAX_PNG);
        let mut offset = 8;
        let mut header = false;
        let mut end = false;
        while offset < bytes.len() {
            let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &bytes[offset + 4..offset + 8];
            assert!(
                matches!(kind, b"IHDR" | b"IDAT" | b"IEND"),
                "no text/metadata channels"
            );
            if kind == b"IHDR" {
                assert_eq!(
                    u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()),
                    width
                );
                assert_eq!(
                    u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()),
                    HEIGHT as u32
                );
                header = true;
            }
            if kind == b"IEND" {
                assert_eq!(size, 0);
                end = true;
            }
            offset += 12 + size;
        }
        assert_eq!(offset, bytes.len());
        assert!(header && end);
    }
    #[test]
    fn fixed_rasters_have_no_text_payload_or_unbounded_dimensions() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        for length in 4..=8 {
            let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
            let answer = key.challenge_answer(&token, length).unwrap();
            for slider in [false, true] {
                let image = render(&answer, [7; 32], slider).unwrap();
                check_png(&image.foreground, WIDTH as u32);
                if slider {
                    check_png(image.background.as_ref().unwrap(), (WIDTH + OFFSET) as u32);
                    assert_eq!(image.offset_max, 80);
                } else {
                    assert!(image.background.is_none());
                    assert_eq!(image.offset_max, 0);
                }
                assert_ne!(
                    image.foreground,
                    render(&answer, [8; 32], slider).unwrap().foreground
                );
            }
        }
    }
}
