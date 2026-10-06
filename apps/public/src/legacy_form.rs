use crate::{
    AppState,
    handlers::{self, AppError, DeleteForm, PostForm},
    posting_form::{MAX_FORM_FIELDS, PostingForm, Rejection, normalize_multipart},
};
use askama::Template;
use axum::{
    Extension, Form,
    body::Body,
    extract::{FromRequest, Path, Request, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, de};
use std::{collections::BTreeSet, fmt};

const INVALID: &str = "Invalid posting or deletion form.";

enum Submission {
    Post(Box<PostForm>),
    Delete(Deletion),
}

pub(crate) struct LegacyForm(Submission);

struct Deletion {
    // Preserve submitted order, including IDs larger than JavaScript's safe integer.
    posts: Vec<i64>,
    password: String,
    file_only: bool,
}

// Enforce the field-count bound while deserializing, before an attacker can
// allocate a vector for every small pair in the otherwise bounded request.
struct Fields(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Fields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded form")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = sequence.next_element()? {
                    if fields.len() == MAX_FORM_FIELDS {
                        return Err(de::Error::custom("too many form fields"));
                    }
                    fields.push(field);
                }
                Ok(Fields(fields))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

fn invalid() -> Rejection {
    Rejection::Multipart(StatusCode::UNPROCESSABLE_ENTITY, INVALID)
}

fn deletion(fields: Vec<(String, String)>) -> Result<Deletion, Rejection> {
    let mut posts = Vec::new();
    let mut password = None;
    let mut file_only = false;
    for (name, value) in fields {
        match name.as_str() {
            "mode" if value == "usrdel" => {}
            "pwd" | "password" if password.is_none() => password = Some(value),
            "onlyimgdel" if value == "on" => file_only = true,
            _ if value == "delete" => {
                let id = name.parse::<i64>().map_err(|_| invalid())?;
                if id <= 0 || id.to_string() != name {
                    return Err(invalid());
                }
                posts.push(id);
            }
            _ => return Err(invalid()),
        }
    }
    if posts.is_empty() {
        return Err(invalid());
    }
    Ok(Deletion {
        posts,
        password: password.unwrap_or_default(),
        file_only,
    })
}

impl<S: Send + Sync> FromRequest<S> for LegacyForm {
    type Rejection = Rejection;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mut query_mode = None;
        for (name, value) in
            url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
        {
            if name == "mode" && query_mode.replace(value.into_owned()).is_some() {
                return Err(invalid());
            }
        }
        let request = normalize_multipart(request, state, true).await?;
        let Form(Fields(mut fields)) = Form::<Fields>::from_request(request, state)
            .await
            .map_err(Rejection::Form)?;
        let mut names = BTreeSet::new();
        if fields.iter().any(|(name, _)| !names.insert(name.as_str())) {
            return Err(invalid());
        }
        if let Some(query_mode) = query_mode {
            match fields.iter().find(|(name, _)| name == "mode") {
                Some((_, mode)) if *mode != query_mode => return Err(invalid()),
                Some(_) => {}
                None => fields.push(("mode".into(), query_mode)),
            }
        }
        if fields
            .iter()
            .any(|(name, mode)| name == "mode" && mode == "usrdel")
        {
            return deletion(fields).map(|form| Self(Submission::Delete(form)));
        }
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&fields)
            .finish();
        let request = Request::post("/")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(encoded))
            .expect("literal internal request");
        Form::<PostForm>::from_request(request, state)
            .await
            .map(|Form(form)| Self(Submission::Post(Box::new(form))))
            .map_err(Rejection::Form)
    }
}

#[derive(Template)]
#[template(path = "delete_success.html")]
struct Deleted<'a> {
    board: &'a str,
}

pub(crate) async fn submit(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Extension(start): Extension<crate::security::RequestStart>,
    Extension(peer): Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    form: Result<LegacyForm, Rejection>,
) -> Response {
    match form {
        Ok(LegacyForm(Submission::Post(form))) => {
            handlers::post(
                State(state),
                Path(board),
                Extension(start),
                Extension(peer),
                headers,
                Ok(PostingForm(*form)),
            )
            .await
        }
        Ok(LegacyForm(Submission::Delete(form))) => {
            let identity = match handlers::deletion_rate_identity(&state, peer) {
                Ok(identity) => identity,
                Err(error) => return error.into_response(),
            };
            if let Err(error) =
                board_store::public_deletion_quota_precheck(&state.pool, &identity).await
            {
                return AppError::from(error).into_response();
            }
            // UserPwd captures its time once before the source deletion loop.
            // Never recompute known-age eligibility because an earlier item waited.
            let session =
                match crate::anonymous_session::Session::resolve(&state, &headers, peer.0).await {
                    Ok(session) => session,
                    Err(error) => return error.into_response(),
                };
            let context = board_store::PublicDeletionContext {
                request_start: start.0,
                session: (!session.posting.minted).then_some(session.posting),
            };
            let mut batch = board_store::PublicDeletionBatch::new(board, context, identity);
            delete_selection(&state, &mut batch, form).await
        }
        Err(error) => {
            let format = crate::posting_response::Format::from_headers(&headers);
            format.finish(format.invalid_form(error))
        }
    }
}

async fn delete_selection(
    state: &AppState,
    batch: &mut board_store::PublicDeletionBatch,
    form: Deletion,
) -> Response {
    let board = batch.slug().to_owned();
    // Source user_delete processes selections sequentially. Each operation
    // commits independently and rechecks authority/policy under its own
    // storage lock. A later failure must not undo an earlier deletion.
    // The shared form-field cap also bounds work per legacy request.
    let multiple = form.posts.len() > 1;
    for no in form.posts {
        if let Err(error) = handlers::delete_with_context(
            state,
            batch,
            DeleteForm {
                no,
                password: form.password.clone(),
                file_only: form.file_only,
            },
        )
        .await
        {
            if error.0 == StatusCode::NOT_FOUND {
                match board_store::public_deletion_target_exists(&state.pool, &board, no).await {
                    Ok(false) if multiple => {
                        // Source delete_post(die=false) falls through to
                        // the upper-age error for a missing manual target.
                        return AppError(
                            StatusCode::FORBIDDEN,
                            board_domain::public_deletion::Rejection::TooOld.message(),
                        )
                        .into_response();
                    }
                    Ok(false) => break, // Source single missing: updating_index().
                    Ok(true) => {}      // Missing authority/file is still an error.
                    Err(error) => return AppError::from(error).into_response(),
                }
            }
            return error.into_response();
        }
    }
    crate::output::html(state, &Deleted { board: &board }).unwrap_or_else(AppError::into_response)
}

#[cfg(all(test, feature = "database-tests"))]
#[path = "legacy_batch_clock_tests.rs"]
mod batch_clock_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::DefaultBodyLimit, routing::post};
    use tower::ServiceExt;

    fn request(uri: &str, fields: &[(&str, &str)], multipart: bool) -> Request {
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
        Request::post(uri)
            .header(CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn released_client_fields_keep_the_exact_id_and_password_in_both_encodings() {
        for multipart in [false, true] {
            for uri in ["/test/imgboard.php", "/test/imgboard.php?mode=usrdel"] {
                let fields = [
                    ("mode", "usrdel"),
                    ("9223372036854775807", "delete"),
                    ("pwd", "owned password + & 😀"),
                    ("onlyimgdel", "on"),
                ];
                let Ok(LegacyForm(Submission::Delete(form))) =
                    LegacyForm::from_request(request(uri, &fields, multipart), &()).await
                else {
                    panic!("valid client deletion was rejected")
                };
                assert_eq!(form.posts, [i64::MAX]);
                assert_eq!(form.password, "owned password + & 😀");
                assert!(form.file_only);
            }
            let fields = [("17", "delete"), ("password", "owned-password")];
            let Ok(LegacyForm(Submission::Delete(form))) = LegacyForm::from_request(
                request("/test/imgboard.php?mode=usrdel", &fields, multipart),
                &(),
            )
            .await
            else {
                panic!("query deletion was rejected")
            };
            assert_eq!(form.posts, [17]);
            assert!(!form.file_only);
        }
    }

    #[tokio::test]
    async fn multiple_selections_preserve_order_and_exact_ids_with_a_bounded_count() {
        for multipart in [false, true] {
            let fields = [
                ("mode", "usrdel"),
                ("9223372036854775807", "delete"),
                ("pwd", "owned-password"),
                ("17", "delete"),
                ("9007199254740993", "delete"),
                ("onlyimgdel", "on"),
            ];
            let Ok(LegacyForm(Submission::Delete(form))) =
                LegacyForm::from_request(request("/test/imgboard.php", &fields, multipart), &())
                    .await
            else {
                panic!("valid batch was rejected")
            };
            assert_eq!(form.posts, [i64::MAX, 17, 9007199254740993]);
            assert_eq!(form.password, "owned-password");
            assert!(form.file_only);

            let ids: Vec<_> = (1..=MAX_FORM_FIELDS).map(|id| id.to_string()).collect();
            let mut fields = vec![("mode", "usrdel"), ("pwd", "owned-password")];
            fields.extend(
                ids[..MAX_FORM_FIELDS - 2]
                    .iter()
                    .map(|id| (id.as_str(), "delete")),
            );
            assert!(
                LegacyForm::from_request(request("/test/imgboard.php", &fields, multipart), &())
                    .await
                    .is_ok()
            );
            fields.push((ids[MAX_FORM_FIELDS - 2].as_str(), "delete"));
            assert!(
                LegacyForm::from_request(request("/test/imgboard.php", &fields, multipart), &())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn posting_retains_its_typed_fields_and_modes() {
        for multipart in [false, true] {
            for mode in ["regist", "post"] {
                let fields = [
                    ("mode", mode),
                    ("pwd", "owned-password"),
                    ("com", "owned & mode=usrdel"),
                ];
                assert!(matches!(
                    LegacyForm::from_request(
                        request("/test/imgboard.php", &fields, multipart),
                        &(),
                    )
                    .await,
                    Ok(LegacyForm(Submission::Post(_)))
                ));
            }
        }
    }

    #[tokio::test]
    async fn source_hidden_password_may_be_empty_or_absent_before_cookie_authorization() {
        for multipart in [false, true] {
            for fields in [
                vec![("mode", "usrdel"), ("17", "delete")],
                vec![("mode", "usrdel"), ("17", "delete"), ("pwd", "")],
            ] {
                let Ok(LegacyForm(Submission::Delete(form))) = LegacyForm::from_request(
                    request("/test/imgboard.php", &fields, multipart),
                    &(),
                )
                .await
                else {
                    panic!("Expected source deletion form");
                };
                assert_eq!(form.posts, [17]);
                assert!(form.password.is_empty());
                assert!(!form.file_only);
            }
        }
    }

    #[tokio::test]
    async fn ambiguous_fields_and_unavailable_authority_are_rejected_before_storage() {
        let invalid_fields = [
            vec![("mode", "usrdel"), ("pwd", "owned-password")],
            vec![
                ("mode", "arcdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
                ("password", "owned-password"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
                ("mode", "usrdel"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
                ("17", "delete"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
                ("admin", "1"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
                ("onlyimgdel", "false"),
            ],
            vec![
                ("mode", "usrdel"),
                ("17", "other"),
                ("pwd", "owned-password"),
            ],
        ];
        for multipart in [false, true] {
            for fields in &invalid_fields {
                assert!(
                    LegacyForm::from_request(request("/test/imgboard.php", fields, multipart), &())
                        .await
                        .is_err()
                );
            }
            for id in ["0", "-1", "+17", "017", "9223372036854775808", "1.0"] {
                let fields = [
                    ("mode", "usrdel"),
                    (id, "delete"),
                    ("pwd", "owned-password"),
                ];
                assert!(
                    LegacyForm::from_request(
                        request("/test/imgboard.php", &fields, multipart),
                        &()
                    )
                    .await
                    .is_err()
                );
            }
            let fields = [
                ("mode", "usrdel"),
                ("17", "delete"),
                ("pwd", "owned-password"),
            ];
            for uri in [
                "/test/imgboard.php?mode=regist",
                "/test/imgboard.php?mode=usrdel&mode=usrdel",
            ] {
                assert!(
                    LegacyForm::from_request(request(uri, &fields, multipart), &())
                        .await
                        .is_err()
                );
            }
        }
    }

    #[tokio::test]
    async fn too_many_fields_and_actual_streamed_body_overflow_are_bounded() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let fields = vec![("pwd", "owned-password"); MAX_FORM_FIELDS + 1];
        assert!(
            LegacyForm::from_request(request("/test/imgboard.php", &fields, false), &())
                .await
                .is_err()
        );
        let app = Router::new()
            .route(
                "/test/imgboard.php",
                post(|form: Result<LegacyForm, Rejection>| async {
                    match form {
                        Ok(_) => StatusCode::NO_CONTENT,
                        Err(error) => error.status(),
                    }
                }),
            )
            .layer(DefaultBodyLimit::max(262_144));
        for multipart in [false, true] {
            let consumed = Arc::new(AtomicUsize::new(0));
            let counter = consumed.clone();
            let prefix = if multipart {
                "--owned\r\nContent-Disposition: form-data; name=\"pwd\"\r\n\r\n"
            } else {
                "pwd="
            };
            let chunks = std::iter::once(bytes::Bytes::from_static(prefix.as_bytes()))
                .chain((0..400).map(|_| bytes::Bytes::from(vec![b'x'; 1024])))
                .map(move |chunk| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, std::io::Error>(chunk)
                });
            let request = Request::post("/test/imgboard.php")
                .header(
                    CONTENT_TYPE,
                    if multipart {
                        "multipart/form-data; boundary=owned"
                    } else {
                        "application/x-www-form-urlencoded"
                    },
                )
                .body(Body::from_stream(futures_util::stream::iter(chunks)))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
            assert!(consumed.load(Ordering::SeqCst) < 400);
        }
    }
}
