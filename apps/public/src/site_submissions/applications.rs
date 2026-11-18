//! Native application forms; contact-email matching never implies mailbox proof.
use super::bounded_header;
use crate::{
    AppState, anonymous_session::Session, handlers::AppError, security::RequestPeer,
    verification_form::PostingSecret,
};
use askama::Template;
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use board_domain::{
    challenge_secrets::{ReceiptToken, TokenKind, VerificationToken},
    site_submissions::{
        ContactEmail, applications as domain,
        ownership::{Capability, Kind},
    },
};
use board_store::site_submissions::{self as store, applications as owned, feedback::Proof};
use board_verification_client::protocol;
use std::collections::BTreeMap;
pub(crate) const SCRIPT: &str = "/static/application-form.v1.js";
#[derive(Clone, Copy)]
pub(crate) struct ApplicationPage(pub bool);
const DATA: &[&str] = &[
    "firstname",
    "handle",
    "email",
    "age",
    "tz",
    "hours",
    "times",
    "board1",
    "board2",
    "q1",
    "q2",
    "q3",
    "q4",
];
#[derive(Template)]
#[template(path = "site-application.html")]
struct Page {
    mode: u8,
    profile: Option<store::ApplicationProfile>,
    values: BTreeMap<String, String>,
    csrf: String,
    token: String,
    image: String,
    wait: u16,
    owner: String,
    auth_email: String,
    revision: i32,
    origin: String,
    error: Option<&'static str>,
}
impl Page {
    fn new(origin: &str) -> Self {
        Self {
            mode: 0,
            profile: None,
            values: BTreeMap::new(),
            csrf: String::new(),
            token: String::new(),
            image: String::new(),
            wait: 0,
            owner: String::new(),
            auth_email: String::new(),
            revision: 0,
            origin: origin.into(),
            error: None,
        }
    }
    fn value(&self, name: &str) -> &str {
        self.values.get(name).map_or("", String::as_str)
    }
    fn is_zone(&self, zone: i16) -> bool {
        self.value("tz") == zone.to_string()
    }
    fn zones(&self) -> Vec<(i16, String)> {
        domain::TIMEZONES
            .map(|n| (n, format!("UTC{n:+}")))
            .collect()
    }
    fn text_fields(&self) -> [(&str, &str, &str); 3] {
        [
            ("firstname", "First Name", "text"),
            ("handle", "Online Nickname / Handle", "text"),
            ("email", "Email (must be valid)", "email"),
        ]
    }
    fn questions(&self) -> [(&str, &str); 4] {
        let p = &self.profile.as_ref().expect("form profile").form.questions;
        [("q1", &p[0]), ("q2", &p[1]), ("q3", &p[2]), ("q4", &p[3])]
    }
}
fn bad() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid application form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Application verification is unavailable. Try again.",
    )
}
fn expired() -> AppError {
    AppError(StatusCode::FORBIDDEN, "This form has expired. Reload it.")
}
pub(super) async fn page(State(state): State<AppState>, request: Request) -> Response {
    match render(&state, request).await {
        Ok(response) => response,
        Err(error) => {
            let mut page = Page::new(&state.origin);
            page.mode = 6;
            page.error = Some(error.1);
            match output(&state, &page, HeaderMap::new()) {
                Ok(mut response) => {
                    *response.status_mut() = error.0;
                    response
                }
                Err(_) => error.into_response(),
            }
        }
    }
}
fn capability(query: Option<&str>) -> Result<Option<Capability>, AppError> {
    match query {
        None => Ok(None),
        Some(query) => {
            let value = query.strip_prefix("id=").ok_or_else(bad)?;
            Capability::parse(Kind::Application, value)
                .map(Some)
                .ok_or_else(bad)
        }
    }
}
fn desktop(headers: &HeaderMap) -> Result<bool, AppError> {
    let agent = bounded_header(headers, header::USER_AGENT, 512)?.to_ascii_lowercase();
    Ok(!["mobile", "android", "iphone", "ipad", "ipod"]
        .iter()
        .any(|part| agent.contains(part)))
}
async fn read(
    request: Request,
    names: &[&str],
) -> Result<crate::verification_form::Fields, AppError> {
    if request
        .headers()
        .get_all(header::CONTENT_TYPE)
        .iter()
        .count()
        != 1
        || request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| {
                !matches!(
                    v,
                    "application/x-www-form-urlencoded"
                        | "application/x-www-form-urlencoded;charset=UTF-8"
                        | "application/x-www-form-urlencoded; charset=UTF-8"
                )
            })
    {
        return Err(bad());
    }
    let bytes = to_bytes(request.into_body(), 786432).await.map_err(|_| {
        AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Application form is too large.",
        )
    })?;
    crate::verification_form::Fields::parse(&bytes, names, 786432).map_err(|_| bad())
}
fn submission(
    profile: &domain::Profile,
    values: &BTreeMap<String, String>,
) -> Result<domain::Submission, AppError> {
    let get = |name: &str| values.get(name).map_or("", String::as_str);
    domain::Submission::prepare(
        profile,
        domain::Input {
            firstname: get("firstname"),
            handle: get("handle"),
            email: get("email"),
            age: get("age").parse().map_err(|_| bad())?,
            timezone: get("tz").parse().map_err(|_| bad())?,
            hours: get("hours").parse().map_err(|_| bad())?,
            times: get("times"),
            board1: get("board1"),
            board2: get("board2"),
            answers: [get("q1"), get("q2"), get("q3"), get("q4")],
        },
    )
    .map_err(|_| bad())
}
fn populate(page: &mut Page, editable: owned::Editable) {
    page.revision = editable.revision;
    let f = editable.fields;
    page.auth_email = f.identity.email.recipient().into();
    page.values = BTreeMap::from([
        ("firstname".into(), f.identity.firstname),
        ("handle".into(), f.identity.handle),
        ("email".into(), f.identity.email.recipient().into()),
        ("age".into(), f.review.age.to_string()),
        ("tz".into(), f.review.timezone.to_string()),
        ("hours".into(), f.review.hours.to_string()),
        ("times".into(), f.review.times),
        ("board1".into(), f.review.board1),
        ("board2".into(), f.review.board2.unwrap_or_default()),
    ]);
    for (i, answer) in f.review.answers.into_iter().enumerate() {
        page.values.insert(format!("q{}", i + 1), answer);
    }
}
async fn render(state: &AppState, request: Request) -> Result<Response, AppError> {
    let mut page = Page::new(&state.origin);
    let Some(profile) = store::application_profile(&state.pool).await? else {
        if request.method() == Method::POST {
            return Err(AppError(StatusCode::CONFLICT, "Applications are closed."));
        }
        return output(state, &page, HeaderMap::new());
    };
    page.profile = Some(profile);
    if !desktop(request.headers())? {
        page.mode = 1;
        return output(state, &page, HeaderMap::new());
    }
    let from_query = capability(request.uri().query())?;
    let peer = request
        .extensions()
        .get::<RequestPeer>()
        .and_then(|p| p.0)
        .ok_or_else(unavailable)?;
    let headers = request.headers().clone();
    let session = Session::resolve(state, &headers, Some(peer)).await?;
    let existing = PostingSecret::read(&headers, state.production).map_err(|_| bad())?;
    let service = state.verification.as_ref().ok_or_else(unavailable)?;
    let secret = if request.method() == Method::POST {
        if session.posting.minted {
            return Err(expired());
        }
        existing.ok_or_else(expired)?
    } else {
        existing
            .map(Ok)
            .unwrap_or_else(PostingSecret::generate)
            .map_err(|_| unavailable())?
    };
    page.csrf = secret.nonce();
    let mut context = protocol::ChallengeContext {
        session: crate::signin::trusted(&session, state, Some(peer))?,
        purpose: protocol::Purpose::SiteApplication,
        board: None,
        target: page.profile.as_ref().unwrap().id,
        form_nonce: page.csrf.clone(),
        email_token: None,
    };
    let mut cap = from_query;
    if let Some(cap) = &cap {
        let target = owned::target(&state.pool, cap)
            .await?
            .ok_or(AppError(StatusCode::NOT_FOUND, "Application not found."))?;
        if target.campaign != context.target {
            return Err(bad());
        }
        context.target = target.id;
        context.purpose = protocol::Purpose::SiteApplicationAuth;
        page.owner = cap.token().into();
        page.mode = 2;
    } else {
        page.mode = 3;
    }
    if request.method() == Method::POST {
        let authentication = cap.is_some();
        let names = if authentication {
            vec!["auth_email", "_cf_fuid", "challenge", "answer"]
        } else {
            let mut names = DATA.to_vec();
            names.extend([
                "action",
                "_cf_fuid",
                "campaign",
                "challenge",
                "answer",
                "id",
                "auth_email",
                "revision",
            ]);
            names
        };
        let fields = read(request, &names).await?;
        if !secret.matches(fields.get("_cf_fuid")) {
            return Err(expired());
        }
        if !authentication {
            for name in DATA {
                if !fields.has(name) {
                    return Err(bad());
                }
                page.values.insert((*name).into(), fields.get(name).into());
            }
            if fields.get("campaign") != page.profile.as_ref().unwrap().id.to_string() {
                return Err(expired());
            }
            if fields.has("id") {
                cap = Some(Capability::parse(Kind::Application, fields.get("id")).ok_or_else(bad)?);
                let target = owned::target(&state.pool, cap.as_ref().unwrap())
                    .await?
                    .ok_or(AppError(StatusCode::NOT_FOUND, "Application not found."))?;
                if target.campaign != context.target {
                    return Err(bad());
                }
                context.target = target.id;
                context.purpose = protocol::Purpose::SiteApplicationEdit;
                page.mode = 4;
                page.owner = fields.get("id").into();
                page.auth_email = ContactEmail::parse(fields.get("auth_email"))
                    .map_err(|_| bad())?
                    .recipient()
                    .into();
                page.revision = fields.get("revision").parse().map_err(|_| bad())?;
                if !(1..=1000000).contains(&page.revision) {
                    return Err(bad());
                }
            } else if fields.has("auth_email") || fields.has("revision") {
                return Err(bad());
            }
            if !matches!(fields.get("action"), "submit" | "challenge") {
                return Err(bad());
            }
        }
        // A refresh echoes only submitted fields; it never reads private answers.
        if authentication || fields.get("action") == "submit" {
            let prepared = if authentication {
                None
            } else {
                Some(submission(
                    &page.profile.as_ref().unwrap().form,
                    &page.values,
                )?)
            };
            let email = if authentication {
                Some(ContactEmail::parse(fields.get("auth_email")).map_err(|_| bad())?)
            } else {
                None
            };
            let answer = fields.get("answer");
            if answer.is_empty()
                || answer.len() > 64
                || answer.chars().any(char::is_control)
                || VerificationToken::parse(TokenKind::Challenge, fields.get("challenge")).is_none()
            {
                return Err(bad());
            }
            let protocol::AnswerResult::Verified {
                receipt,
                peer_hash,
                context_hash,
            } = service
                .check(&protocol::Answer {
                    context: context.clone(),
                    challenge: fields.get("challenge").into(),
                    answer: answer.into(),
                })
                .await
                .map_err(|_| unavailable())?
            else {
                return Err(AppError(
                    StatusCode::BAD_REQUEST,
                    "Verification failed. Reload the form and try again.",
                ));
            };
            let receipt = ReceiptToken::parse(&receipt)
                .ok_or_else(unavailable)?
                .storage_hash();
            let proof = Proof {
                session: &session.posting.fingerprints.token,
                receipt: &receipt,
                peer: &peer_hash,
                context: &context_hash,
            };
            if authentication {
                let editable = owned::authenticate(
                    &state.pool,
                    cap.as_ref().unwrap(),
                    email.as_ref().unwrap(),
                    page.profile.as_ref().unwrap(),
                    &proof,
                )
                .await?
                .ok_or(AppError(
                    StatusCode::FORBIDDEN,
                    "Application authentication failed.",
                ))?;
                populate(&mut page, editable);
                page.mode = 4;
                context.purpose = protocol::Purpose::SiteApplicationEdit;
            } else {
                if let Some(cap) = &cap {
                    let email = ContactEmail::parse(&page.auth_email).map_err(|_| bad())?;
                    owned::edit(&state.pool,cap,&email,page.revision,prepared.as_ref().unwrap(),&proof).await?.ok_or(AppError(StatusCode::CONFLICT,"Application changed or is no longer editable. Reload the private edit URL."))?;
                } else {
                    let created =
                        Capability::generate(Kind::Application).map_err(|_| unavailable())?;
                    owned::submit(
                        &state.pool,
                        page.profile.as_ref().unwrap().id,
                        &created,
                        prepared.as_ref().unwrap(),
                        &proof,
                    )
                    .await?;
                    page.owner = created.token().into();
                }
                page.mode = 5;
                return output(state, &page, HeaderMap::new());
            }
        }
    }
    let issued = service
        .posting_challenge(&protocol::PostingChallengeRequest {
            context,
            ticket: None,
            ticket_answer: None,
        })
        .await
        .map_err(|_| unavailable())?;
    if issued.ticket_challenge {
        return Err(unavailable());
    }
    match issued.challenge {
        protocol::Challenge::Ready {
            token,
            image,
            slider: false,
            background: None,
            ..
        } if crate::posting_challenge::page::valid_png(&image)
            && VerificationToken::parse(TokenKind::Challenge, &token).is_some() =>
        {
            page.token = token;
            page.image = image;
        }
        protocol::Challenge::Wait {
            seconds,
            ticket: None,
        } => {
            page.wait = seconds.max(1);
        }
        _ => return Err(unavailable()),
    }
    let mut cookies = HeaderMap::new();
    secret.append(&mut cookies, state.production);
    session.append(&mut cookies, state.production);
    output(state, &page, cookies)
}
fn output(state: &AppState, page: &Page, cookies: HeaderMap) -> Result<Response, AppError> {
    let mut response = crate::output::html(state, page)?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
        .extensions_mut()
        .insert(ApplicationPage(matches!(page.mode, 3 | 4)));
    if !page.image.is_empty() {
        response
            .extensions_mut()
            .insert(crate::posting_challenge::page::CaptchaPage);
    }
    for cookie in cookies.get_all(header::SET_COOKIE) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.clone());
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendered_application_controls_match_source_gate_states() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/site-submission-reference.json"
        )))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["input"]["template"] == "janitorapp.tpl.php")
        {
            let input = &case["input"];
            let state = domain::form_state(
                input["open"].as_bool().unwrap(),
                input["desktop"].as_bool().unwrap(),
                input["need_email"].as_bool().unwrap(),
                input["existing"].as_bool().unwrap(),
            );
            let mut page = Page::new("https://owned.invalid");
            page.profile = Some(store::ApplicationProfile {
                id: 1,
                form: domain::Profile {
                    boards: vec!["g".into(), "a".into()],
                    questions: std::array::from_fn(|i| format!("Owned {i}")),
                    field_limit: 150,
                    answer_limit: 2000,
                },
            });
            page.mode = match state {
                domain::FormState::Closed => 0,
                domain::FormState::DesktopRequired => 1,
                domain::FormState::AuthenticateEmail => 2,
                domain::FormState::New => 3,
                domain::FormState::Edit => 4,
            };
            let html = page.render().unwrap();
            let actual: Vec<_> = html
                .split('<')
                .filter(|tag| {
                    ["input ", "select ", "textarea ", "button "]
                        .iter()
                        .any(|p| tag.starts_with(p))
                })
                .filter_map(|tag| {
                    let tag = tag.split('>').next().unwrap();
                    if tag.contains("value=\"challenge\"") {
                        return None;
                    }
                    let name = tag.split("name=\"").nth(1)?.split('"').next()?;
                    if ["answer", "challenge", "campaign", "revision"].contains(&name)
                        || (page.mode == 2 && name == "_cf_fuid")
                    {
                        None
                    } else {
                        Some(name)
                    }
                })
                .collect();
            let expected: Vec<_> = case["output"]["forms"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|f| f["fields"].as_array().unwrap())
                .filter_map(|f| f["name"].as_str())
                .collect();
            assert_eq!(actual, expected, "{input}");
            assert_eq!(html.contains("data-edit"), page.mode == 4);
            if matches!(page.mode, 3 | 4) {
                for zone in -12..=13 {
                    assert!(html.contains(&format!("value=\"{zone}\">UTC{zone:+}")));
                }
            }
            count += 1;
        }
        assert_eq!(count, 32);
    }
    #[tokio::test]
    async fn application_input_is_bounded_exact_and_non_lossy() {
        for raw in ["a=one&a=two", "other=x", "a=%ff", "a=%", "a=%00"] {
            let request = Request::post("/janitorapp")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(axum::body::Body::from(raw))
                .unwrap();
            assert!(read(request, &["a"]).await.is_err());
        }
        let request = Request::post("/janitorapp")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(
                "a=%3Cb%3Eliteral%3C%2Fb%3E%0D%0Asecond",
            ))
            .unwrap();
        assert_eq!(
            read(request, &["a"]).await.unwrap().get("a"),
            "<b>literal</b>\r\nsecond"
        );
        let cap = Capability::generate(Kind::Application).unwrap();
        assert!(
            capability(Some(&format!("id={}", cap.token())))
                .unwrap()
                .is_some()
        );
        for query in [
            format!("id={}&id={}", cap.token(), cap.token()),
            format!("id={}&extra=x", cap.token()),
            "id=bad".into(),
        ] {
            assert!(capability(Some(&query)).is_err());
        }
    }
}
