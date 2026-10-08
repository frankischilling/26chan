//! Source target selection only; local-origin and strict-parser adaptations are separate.

#[test]
fn source_res_redirect_preserves_lookup_ids_and_records_legacy_scheme_choice() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/res-redirect.json")).unwrap();
    assert_eq!(
        fixture["files"]["imgboard.php"],
        "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
    );
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 20);
    for row in rows {
        let post = row["post"].as_str().unwrap();
        let parent = row["parent"].as_str().unwrap();
        let headers = row["headers"].as_array().unwrap();
        assert_eq!(headers[0], "Cache-Control: public, max-age=2");
        if row["kind"] == "missing" {
            assert_eq!(row["status"], 404);
            assert_eq!(headers.len(), 1);
            assert_eq!(row["body"], "");
            continue;
        }
        let referer = row["referer"].as_str().unwrap();
        let scheme = if referer.is_empty() || referer.to_ascii_lowercase().contains("https") {
            "https"
        } else {
            "http"
        };
        let thread = if parent == "0" { post } else { parent };
        let target = format!("{scheme}://boards.4chan.org/g/thread/{thread}#p{post}");
        assert_eq!(row["status"], 301);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[1], format!("Location: {target}"));
        assert_eq!(
            row["body"],
            format!("<meta http-equiv=\"refresh\" content=\"0;URL={target}\">")
        );
        if row["kind"] == "large_reply" {
            assert!(target.contains("/9007199254740993#p9223372036854775807"));
        }
    }
}
