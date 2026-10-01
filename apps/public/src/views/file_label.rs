pub struct FileLabel {
    pub text: String,
    pub shortened: bool,
}

pub fn label(filename: &str, op: bool) -> FileLabel {
    let (stem, extension) = filename
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or((filename, String::new()), |(stem, ext)| {
            (stem, format!(".{ext}"))
        });
    let shortened = stem.encode_utf16().count() > if op { 40 } else { 30 };
    let text = if shortened {
        let units: Vec<_> = stem.encode_utf16().take(if op { 35 } else { 25 }).collect();
        String::from_utf16_lossy(&units) + "(...)" + &extension
    } else {
        filename.to_owned()
    };
    FileLabel { text, shortened }
}

pub fn size(bytes: i64) -> String {
    let bytes = bytes.max(0) as u128;
    if bytes >= 1_048_576 {
        let rounded = (bytes * 100 + 524_288) / 1_048_576;
        let fraction = rounded % 100;
        let number = if fraction == 0 {
            (rounded / 100).to_string()
        } else if fraction.is_multiple_of(10) {
            format!("{}.{}", rounded / 100, fraction / 10)
        } else {
            format!("{}.{fraction:02}", rounded / 100)
        };
        format!("{number} MB")
    } else if bytes > 1024 {
        format!("{} KB", (bytes + 512) / 1024)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels_and_sizes_match_released_formatter_vectors() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../../docs/public-file-reference.json"))
                .unwrap();
        for case in reference["cases"].as_array().unwrap() {
            let filename = format!("{}.png", case["filename"].as_str().unwrap());
            let label = super::label(&filename, case["kind"] == "op");
            assert_eq!(label.text, case["label"].as_str().unwrap());
            assert_eq!(label.shortened, !case["title"].is_null());
            assert_eq!(
                format!("{} PNG", super::size(case["bytes"].as_i64().unwrap())),
                case["mobile_info"].as_str().unwrap()
            );
        }
    }
}
