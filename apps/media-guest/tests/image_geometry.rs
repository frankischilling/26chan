use board_media_guest::image_geometry::{self, StillFormat};
use serde_json::Value;

fn pair(value: &Value) -> (u32, u32) {
    (
        value[0].as_u64().unwrap().try_into().unwrap(),
        value[1].as_u64().unwrap().try_into().unwrap(),
    )
}

#[test]
fn geometry_matches_independent_source_fixture() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/source-image-presentation.json"
    ))
    .unwrap();
    for case in fixture["cases"]["thumbnail"].as_array().unwrap() {
        let actual = image_geometry::thumbnail(pair(&case["dimensions"]), pair(&case["bounds"]));
        let expected = pair(&case["expected"]);
        if expected.0 == 0 || expected.1 == 0 {
            assert!(
                actual.is_err(),
                "source cannot allocate zero-size thumbnail: {}",
                case["id"]
            );
        } else {
            assert_eq!(actual.unwrap(), expected, "{}", case["id"]);
        }
    }
    for case in fixture["cases"]["mobile"].as_array().unwrap() {
        let format = match case["extension"].as_str().unwrap() {
            ".jpg" => StillFormat::Jpeg,
            ".png" => StillFormat::Png,
            _ => {
                // The typed API cannot manufacture a still-image format from
                // an unrecognized extension or a GIF input.
                assert!(case["expected"].is_null());
                continue;
            }
        };
        let actual = image_geometry::mobile(
            format,
            pair(&case["dimensions"]),
            case["bytes"].as_u64().unwrap(),
        )
        .unwrap();
        let expected = (!case["expected"].is_null()).then(|| pair(&case["expected"]));
        assert_eq!(actual, expected, "{}", case["id"]);
    }
}
