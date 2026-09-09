use board_media_guest::decode_png;

fn png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
    bytes
}

#[test]
fn converts_rgb_and_grayscale_to_exact_rgba_protocol() {
    let red = png(1, 1, png::ColorType::Rgb, &[255, 0, 0]);
    assert_eq!(
        decode_png(&red).unwrap(),
        b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff"
    );
    let gray = png(1, 1, png::ColorType::GrayscaleAlpha, &[80, 120]);
    assert_eq!(&decode_png(&gray).unwrap()[16..], &[80, 80, 80, 120]);
}

#[test]
fn rejects_non_png_and_oversize_images() {
    assert!(decode_png(b"harmless non-image bytes").is_err());
    let wide = png(1025, 1, png::ColorType::Grayscale, &[0; 1025]);
    assert!(decode_png(&wide).is_err());
}

#[test]
fn preserves_rgba_and_rejects_truncated_png() {
    let input = png(1, 1, png::ColorType::Rgba, &[10, 20, 30, 40]);
    assert_eq!(&decode_png(&input).unwrap()[16..], &[10, 20, 30, 40]);
    assert!(decode_png(&input[..input.len() / 2]).is_err());
}
