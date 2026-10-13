#![cfg(target_os = "linux")]

use board_media_guest::pdf;

// Owned vector-only PDF files. No external document, font or image is included.
fn document(streams: &[&str], width: u32, height: u32) -> Vec<u8> {
    document_with_filter(streams, width, height, None)
}

fn document_with_filter(
    streams: &[&str],
    width: u32,
    height: u32,
    filtered_page: Option<usize>,
) -> Vec<u8> {
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_owned()];
    let kids = (0..streams.len())
        .map(|index| format!("{} 0 R", index * 2 + 3))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Count {} /Kids [{kids}] >>",
        streams.len()
    ));
    for (index, stream) in streams.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Courier >> >> >> /Contents {} 0 R >>",
            index * 2 + 4
        ));
        let filter = if filtered_page == Some(index) {
            " /Filter /FlateDecode"
        } else {
            ""
        };
        objects.push(format!(
            "<< /Length {}{filter} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}

const RED: &str = "1 0 0 rg 0 0 72 36 re f";
const BLUE: &str = "0 0 1 rg 0 0 72 36 re f";

#[test]
fn first_page_preview_retains_aspect_and_ignores_later_page_colors() {
    let one = pdf::decode(&document(&[RED], 72, 36)).unwrap();
    let two = pdf::decode(&document(&[RED, BLUE], 72, 36)).unwrap();
    assert_eq!(one, two);
    assert_eq!(&one[..8], b"IBRGBA01");
    assert_eq!(&one[8..16], &[0, 0, 0, 72, 0, 0, 0, 36]);
    assert_eq!(one.len(), 16 + 72 * 36 * 4);
    for pixel in one[16..].chunks_exact(4) {
        assert!(pixel[0] >= 250 && pixel[1] <= 5 && pixel[2] <= 5);
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn later_invalid_page_is_rejected_before_first_page_preview() {
    assert!(
        pdf::decode(&document_with_filter(
            &[RED, "not compressed"],
            72,
            36,
            Some(1)
        ))
        .is_err()
    );
    assert!(pdf::decode(&document(&[], 72, 36)).is_err());
    assert!(pdf::decode(b"%PDF-1.4\nnot a document\n").is_err());
    assert!(pdf::decode(&document(&[RED], 1025, 36)).is_err());
    assert!(pdf::decode(&vec![b'x'; 8 * 1024 * 1024 + 1]).is_err());
    assert!(pdf::decode(&document(&[BLUE], 72, 36)).is_ok());
}

#[test]
fn private_disk_requires_complete_length_zero_padding_and_eof() {
    let source = document(&[RED], 72, 36);
    let mut disk = (source.len() as u64).to_be_bytes().to_vec();
    disk.extend_from_slice(&source);
    disk.resize(disk.len().div_ceil(512) * 512, 0);
    let expected = pdf::decode(&source).unwrap();
    assert_eq!(pdf::decode_disk(disk.as_slice()).unwrap(), expected);
    let mut trailing = disk.clone();
    trailing.push(0);
    assert!(pdf::decode_disk(trailing.as_slice()).is_err());
    assert!(pdf::decode_disk(&disk[..disk.len() - 1]).is_err());
    *disk.last_mut().unwrap() = 1;
    assert!(pdf::decode_disk(disk.as_slice()).is_err());
    assert!(pdf::decode_disk(0_u64.to_be_bytes().as_slice()).is_err());
}

#[test]
fn base_font_text_renders_without_host_font_configuration() {
    let blank = document(&[""], 144, 72);
    let text = document(&["BT /F1 18 Tf 8 24 Td (Owned text) Tj ET"], 144, 72);
    let blank = pdf::decode(&blank).unwrap();
    let text = pdf::decode(&text).unwrap();
    assert_eq!(&blank[..16], &text[..16]);
    assert_ne!(blank, text);
}

#[test]
fn password_protected_document_is_rejected_without_interactive_input() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // The fixed installed renderer encrypts only this test's owned fixture.
    let mut child = Command::new("/usr/bin/gs")
        .args([
            "-q",
            "-dSAFER",
            "-dBATCH",
            "-dNOPAUSE",
            "-sDEVICE=pdfwrite",
            "-sOwnerPassword=owned-owner",
            "-sUserPassword=owned-reader",
            "-sOutputFile=%stdout",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&document(&[RED], 72, 36))
        .unwrap();
    let encrypted = child.wait_with_output().unwrap();
    assert!(encrypted.status.success());
    assert!(encrypted.stdout.starts_with(b"%PDF-"));
    assert!(pdf::decode(&encrypted.stdout).is_err());
    assert!(pdf::decode(&document(&[RED], 72, 36)).is_ok());
}
