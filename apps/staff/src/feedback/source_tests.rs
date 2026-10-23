use super::*;
#[test]
fn native_review_controls_match_all_source_template_states() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/site-submission-reference.json"
    )))
    .unwrap();
    let mut compared = 0;
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["input"]["template"] == "feedback-review.tpl.php")
    {
        let input = &case["input"];
        let state = input["answered"].as_i64().unwrap_or(0) as i32;
        let html = if input["mode"] == "read" {
            Detail {
                saved: SavedDetail {
                    id: 7,
                    profile: 1,
                    category: 1,
                    category_label: "Owned".into(),
                    subject: "Owned".into(),
                    message: "Literal <body>".into(),
                    state: state as i16,
                    reply: None,
                    reply_capcode: None,
                    created_at: 1,
                    updated_at: 1,
                    address: "192.0.2.225".into(),
                    user_agent: "Owned".into(),
                    language: "en".into(),
                },
                date: "Owned date".into(),
                back: "/feedback".into(),
            }
            .render()
            .unwrap()
        } else {
            let rows = if input["empty"].as_bool().unwrap() {
                vec![]
            } else {
                vec![Row {
                    saved: SavedRow {
                        id: 7,
                        category: 1,
                        category_label: "Owned".into(),
                        subject: "Owned".into(),
                        message: "Literal <body>".into(),
                        reply: None,
                        reply_capcode: None,
                        updated_at: 1,
                    },
                    date: "Owned date".into(),
                    href: "/feedback?action=read&id=7".into(),
                }]
            };
            Page {
                profile: 1,
                status: state,
                category: String::new(),
                csrf: "owned-form-token".into(),
                recent: true,
                can_dismiss: input["can_dismiss"].as_bool().unwrap(),
                is_admin: input["admin"].as_bool().unwrap(),
                rows,
                categories: vec![],
                draft_count: 0,
                next: String::new(),
                all: String::new(),
                answered: String::new(),
                dismissed: String::new(),
                drafts: String::new(),
            }
            .render()
            .unwrap()
        };
        let expected: Vec<_> = case["output"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| action["class"].as_str().unwrap())
            .collect();
        let actual: Vec<_> = html
            .split("class=\"")
            .skip(1)
            .filter_map(|part| part.split('"').next())
            .filter(|class| class.starts_with("button btn-other "))
            .collect();
        assert_eq!(actual, expected, "{input}");
        assert!(
            !html.contains("<textarea"),
            "Source reply form is commented out: {input}"
        );
        assert!(!html.contains("Literal <body>"));
        compared += 1;
    }
    assert_eq!(compared, 64);
}
