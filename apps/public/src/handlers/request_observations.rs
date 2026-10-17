use super::*;
use axum::{
    body::Body,
    extract::{FromRequest, Request},
    http::header::CONTENT_TYPE,
};

fn request(fields: &[(&str, &str)], multipart: bool) -> Request {
    let (content_type, body) = if multipart {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned--\r\n");
        ("multipart/form-data; boundary=owned", body)
    } else {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().copied())
                .finish(),
        )
    };
    Request::post("/owned")
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn public_post_and_report_keep_absent_and_empty_challenge_fields_distinct() {
    for (challenge, answer) in [
        (None, None),
        (Some(""), None),
        (None, Some("")),
        (Some(""), Some("")),
        (Some("noop"), Some("0")),
        (Some("owned"), Some("answer")),
    ] {
        let mut fields = Vec::new();
        if let Some(value) = challenge {
            fields.push(("t-challenge", value));
        }
        if let Some(value) = answer {
            fields.push(("t-response", value));
        }
        for multipart in [false, true] {
            let parsed =
                crate::posting_form::PostingForm::from_request(request(&fields, multipart), &())
                    .await;
            let Ok(crate::posting_form::PostingForm(form)) = parsed else {
                panic!("owned posting form must parse");
            };
            assert_eq!(form.captcha_challenge.as_deref(), challenge);
            assert_eq!(form.captcha_answer.as_deref(), answer);
        }
        fields.extend([("no", "1"), ("reason", "owned")]);
        let axum::Form(form) = axum::Form::<ReportForm>::from_request(request(&fields, false), &())
            .await
            .unwrap();
        assert_eq!(form.captcha_challenge.as_deref(), challenge);
        assert_eq!(form.captcha_answer.as_deref(), answer);
    }
}

#[tokio::test]
async fn optional_observations_do_not_admit_duplicate_challenge_fields() {
    for name in ["t-challenge", "t-response"] {
        for multipart in [false, true] {
            let fields = [(name, "one"), (name, "two")];
            assert!(
                crate::posting_form::PostingForm::from_request(request(&fields, multipart), &())
                    .await
                    .is_err()
            );
        }
        let fields = [
            (name, "one"),
            (name, "two"),
            ("no", "1"),
            ("reason", "owned"),
        ];
        assert!(
            axum::Form::<ReportForm>::from_request(request(&fields, false), &())
                .await
                .is_err()
        );
    }
}
