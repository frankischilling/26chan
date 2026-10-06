//! Body-only proof; HTTP status, cache and authorization need separate handler tests.

#[test]
fn source_index_body_has_exact_delay_message_and_styling() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/legacy-index.json")).unwrap();
    assert_eq!(fixture["source_message"], "Updating index...");
    assert_eq!(
        fixture["files"]["config/global_strings.ini"],
        "3b3b60083db74fb1693af6f0824cccd6ec3ca446242474008cf9be474c65cd17"
    );
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let style = "font-family:times,serif;font-size:36pt;text-align:center;width:100%;height:300px;";
    for row in rows {
        let referer = row["referer"].as_str().unwrap();
        let scheme = if referer.to_ascii_lowercase().contains("https") {
            "https"
        } else {
            "http"
        };
        let target = format!(
            "{scheme}://boards.{}/{}/",
            row["domain_stub"].as_str().unwrap(),
            row["board"].as_str().unwrap()
        );
        assert_eq!(row["refresh_seconds"], 2);
        assert_eq!(row["target"], target);
        assert_eq!(row["title"], "Updating index...");
        assert_eq!(row["text"], "Updating index...");
        assert_eq!(row["style"], style);
        assert_eq!(
            row["body"],
            format!(
                "<!doctype html><head><meta http-equiv=\"refresh\" content=\"2;URL={target}\"><title>Updating index...</title></head><body><table style=\"{style}\"><td><strong>Updating index...</strong></td></table>"
            )
        );
    }
}
