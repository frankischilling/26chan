use crate::handlers::PostForm;
use axum::{
    Form,
    body::Body,
    extract::{FromRequest, Multipart, Request, rejection::FormRejection},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use std::collections::BTreeSet;

pub struct PostingForm(pub PostForm);

pub enum Rejection {
    Form(FormRejection),
    Multipart(StatusCode, &'static str),
}

impl Rejection {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Form(error) => error.status(),
            Self::Multipart(status, _) => *status,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Form(_) => "Invalid posting form.",
            Self::Multipart(_, message) => message,
        }
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        match self {
            Self::Form(error) => error.into_response(),
            Self::Multipart(status, message) => (status, message).into_response(),
        }
    }
}

const TEXT_FIELDS: &[&str] = &[
    "name",
    "sub",
    "com",
    "password",
    "pwd",
    "resto",
    "email",
    "upload_id",
    "upload_capability",
    "spoiler",
    "awt",
    "track",
    "mode",
    "MAX_FILE_SIZE",
    "hasjs",
    "textonly",
];
const INVALID: &str = "Invalid posting form.";
const FILE_INTAKE: &str = "Upload files through the isolated upload form before posting.";

impl<S: Send + Sync> FromRequest<S> for PostingForm {
    type Rejection = Rejection;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let multipart = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("multipart/form-data"));
        if !multipart {
            return Form::<PostForm>::from_request(request, state)
                .await
                .map(|Form(form)| Self(form))
                .map_err(Rejection::Form);
        }
        // Multipart retains the route's 262,144-byte streaming body limit.
        // It never hands a file to a decoder or to the content store.
        let mut multipart = Multipart::from_request(request, state)
            .await
            .map_err(|error| Rejection::Multipart(error.status(), INVALID))?;
        let mut names = BTreeSet::new();
        let mut encoded = String::new();
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|error| Rejection::Multipart(error.status(), INVALID))?
        {
            let name = field.name().unwrap_or("").to_owned();
            if (!TEXT_FIELDS.contains(&name.as_str()) && name != "upfile")
                || !names.insert(name.clone())
            {
                return Err(Rejection::Multipart(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    INVALID,
                ));
            }
            if name == "upfile" {
                // Browsers submit an empty file part when no file was selected.
                if field.file_name().is_some_and(|name| !name.is_empty()) {
                    return Err(Rejection::Multipart(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        FILE_INTAKE,
                    ));
                }
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|error| Rejection::Multipart(error.status(), INVALID))?
                {
                    if !chunk.is_empty() {
                        return Err(Rejection::Multipart(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            FILE_INTAKE,
                        ));
                    }
                }
                continue;
            }
            if field.file_name().is_some() {
                return Err(Rejection::Multipart(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    INVALID,
                ));
            }
            let bytes = field
                .bytes()
                .await
                .map_err(|error| Rejection::Multipart(error.status(), INVALID))?;
            let value = std::str::from_utf8(&bytes)
                .map_err(|_| Rejection::Multipart(StatusCode::UNPROCESSABLE_ENTITY, INVALID))?;
            if !encoded.is_empty() {
                encoded.push('&');
            }
            url::form_urlencoded::Serializer::new(&mut encoded).append_pair(&name, value);
        }
        // A bounded internal conversion shares the exact typed URL-form parser,
        // including duplicate aliases and deny_unknown_fields. Percent encoding
        // expands each input byte by at most three; no raw request is replayed.
        if encoded.len() > 3 * 262_144 {
            return Err(Rejection::Multipart(StatusCode::PAYLOAD_TOO_LARGE, INVALID));
        }
        let request = Request::post("/")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(encoded))
            .expect("literal internal request");
        Form::<PostForm>::from_request(request, state)
            .await
            .map(|Form(form)| Self(form))
            .map_err(Rejection::Form)
    }
}

pub fn checkbox<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    use serde::Deserialize;
    match String::deserialize(deserializer)?.as_str() {
        "on" | "true" => Ok(true),
        "" | "false" => Ok(false),
        _ => Err(serde::de::Error::custom("invalid checkbox value")),
    }
}

#[derive(serde::Deserialize)]
pub enum Mode {
    #[serde(rename = "regist", alias = "post")]
    Regist,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::DefaultBodyLimit;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn body(fields: &[(&str, &[u8], Option<&str>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value, filename) in fields {
            let filename = filename
                .map(|name| format!("; filename=\"{name}\""))
                .unwrap_or_default();
            body.extend_from_slice(
                format!(
                    "--owned\r\nContent-Disposition: form-data; name=\"{name}\"{filename}\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(value);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--owned--\r\n");
        body
    }

    fn request(body: Body) -> Request {
        Request::post("/test/imgboard.php")
            .header("origin", "http://127.0.0.1:3000")
            .header("accept", "application/json")
            .header(CONTENT_TYPE, "multipart/form-data; boundary=owned")
            .body(body)
            .unwrap()
    }

    async fn parser() -> axum::Router {
        // An isolated parser endpoint exposes no post values or storage access.
        axum::Router::new()
            .route(
                "/test/imgboard.php",
                axum::routing::post(|form: Result<PostingForm, Rejection>| async {
                    match form {
                        Ok(_) => StatusCode::NO_CONTENT.into_response(),
                        Err(error) => crate::posting_response::Format::Json.invalid_form(error),
                    }
                }),
            )
            .layer(DefaultBodyLimit::max(262_144))
    }

    #[tokio::test]
    async fn source_fields_and_empty_file_are_accepted_without_relaxing_typed_validation() {
        let app = parser().await;
        let valid = [
            ("pwd", b"owned-password".as_slice(), None),
            ("mode", b"regist".as_slice(), None),
            ("com", "Unicode 😀 & + % \r\nsecond".as_bytes(), None),
            ("MAX_FILE_SIZE", b"99999999999999999999999".as_slice(), None),
            ("hasjs", b"".as_slice(), None),
            ("textonly", b"on".as_slice(), None),
            ("upfile", b"".as_slice(), Some("")),
        ];
        assert_eq!(
            app.clone()
                .oneshot(request(Body::from(body(&valid))))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let mut alternate_mode = valid;
        alternate_mode[1].1 = b"post";
        assert_eq!(
            app.clone()
                .oneshot(request(Body::from(body(&alternate_mode))))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        for extra in [
            ("pwd", b"duplicate".as_slice(), None),
            ("password", b"ambiguous-alias".as_slice(), None),
            ("mode", b"usrdel".as_slice(), None),
            ("resto", b"9223372036854775808".as_slice(), None),
            ("name", b"invalid\xffutf8".as_slice(), None),
            ("name", b"file-in-text".as_slice(), Some("text.txt")),
            ("admin", b"not-authority".as_slice(), None),
            ("spoiler", b"yes".as_slice(), None),
            ("upfile", b"".as_slice(), Some("")),
        ] {
            let mut fields = valid.to_vec();
            fields.push(extra);
            let response = app
                .clone()
                .oneshot(request(Body::from(body(&fields))))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                serde_json::json!({"error": INVALID})
            );
        }
        for (value, filename) in [(b"".as_slice(), "selected.png"), (b"x".as_slice(), "")] {
            let fields = [
                ("pwd", b"owned-password".as_slice(), None),
                ("upfile", value, Some(filename)),
            ];
            let response = app
                .clone()
                .oneshot(request(Body::from(body(&fields))))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                serde_json::json!({"error": FILE_INTAKE})
            );
        }
        let fields = [
            ("pwd", b"owned-password".as_slice(), None),
            ("mode", b"usrdel".as_slice(), None),
        ];
        assert_eq!(
            app.clone()
                .oneshot(request(Body::from(body(&fields))))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut missing_boundary = request(Body::empty());
        missing_boundary
            .headers_mut()
            .insert(CONTENT_TYPE, "multipart/form-data".parse().unwrap());
        assert_eq!(
            app.clone()
                .oneshot(missing_boundary)
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            app.oneshot(request(Body::from("--owned\r\ninvalid")))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn actual_stream_limit_stops_reading_oversized_fields() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let consumed = Arc::new(AtomicUsize::new(0));
        let counter = consumed.clone();
        let chunks = std::iter::once(bytes::Bytes::from_static(
            b"--owned\r\nContent-Disposition: form-data; name=\"com\"\r\n\r\n",
        ))
        .chain((0..400).map(|_| bytes::Bytes::from(vec![b'x'; 1024])))
        .chain(std::iter::once(bytes::Bytes::from_static(
            b"\r\n--owned--\r\n",
        )))
        .map(move |chunk| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(chunk)
        });
        let response = parser()
            .await
            .oneshot(request(Body::from_stream(futures_util::stream::iter(
                chunks,
            ))))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(
            consumed.load(Ordering::SeqCst) < 400,
            "must reject before consuming the complete stream"
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(64))]
        #[test]
        fn bounded_arbitrary_text_does_not_change_the_field_structure(value in ".{0,300}") {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async {
                let fields = [("pwd", b"owned-password".as_slice(), None), ("com", value.as_bytes(), None)];
                assert_eq!(parser().await.oneshot(request(Body::from(body(&fields)))).await.unwrap().status(), StatusCode::NO_CONTENT);
            });
        }
    }
}
