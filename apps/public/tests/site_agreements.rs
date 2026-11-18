#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use board_store::site_submissions::admin::agreements;
use sqlx::PgPool;
use tower::ServiceExt;

const PATH: &str = "/janitorapp/agreement";
const ORIGIN: &str = "http://127.0.0.1:3000";

async fn send(web: &Router, origin: &str, cookie: &str, form: Option<String>) -> Response {
    web.clone()
        .oneshot(
            Request::builder()
                .uri(PATH)
                .method(if form.is_some() { "POST" } else { "GET" })
                .header(header::ORIGIN, origin)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn html(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 150_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn nonce(html: &str) -> &str {
    html.split("name=\"csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}

#[tokio::test]
async fn selected_applicant_can_accept_only_the_current_reviewed_agreement() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let campaign: i64 = sqlx::query_scalar(
        "INSERT INTO submission_private.application_campaigns
         (id, starts_at, ends_at, boards, questions, field_limit, answer_limit, capacity)
         VALUES(nextval('content.post_number'),1,2,ARRAY['g'],ARRAY['One','Two','Three','Four'],100,100,10)
         RETURNING id",
    ).fetch_one(&owner).await.unwrap();
    let application: i64 = sqlx::query_scalar(
        "INSERT INTO submission_private.applications
         (campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at)
         VALUES($1,25,0,1,'Owned','g',ARRAY['a','b','c','d'],1,1) RETURNING id",
    )
    .bind(campaign)
    .fetch_one(&owner)
    .await
    .unwrap();

    let db = owner.clone();
    let result = tokio::spawn(async move {
        let mut operator = db.acquire().await.unwrap();
        assert!(matches!(
            agreements::issue(&mut operator, application, "Owned terms").await,
            Err(board_store::site_submissions::admin::Error::AgreementUnavailable),
        ));
        sqlx::query("UPDATE submission_private.applications SET state=2 WHERE id=$1")
            .bind(application).execute(&db).await.unwrap();

        let revoked = agreements::issue(&mut operator, application, "Revoked terms").await.unwrap();
        assert!(agreements::revoke(&mut operator, application).await.unwrap());
        assert!(board_store::site_submissions::agreements::terms(&public, &revoked).await.unwrap().is_none());
        let expired = agreements::issue(&mut operator, application, "Expired terms").await.unwrap();
        sqlx::query(
            "UPDATE submission_private.application_agreements
             SET issued_at=clock_timestamp()-interval '2 days', expires_at=clock_timestamp()-interval '1 day'
             WHERE application_id=$1",
        ).bind(application).execute(&db).await.unwrap();
        assert!(board_store::site_submissions::agreements::terms(&public, &expired).await.unwrap().is_none());
        assert!(!board_store::site_submissions::agreements::accept(&public, &expired).await.unwrap());

        let obsolete = agreements::issue(&mut operator, application, "Old terms").await.unwrap();
        let invite = agreements::issue(&mut operator, application, "Owned terms\n<script>literal</script>").await.unwrap();
        assert!(sqlx::query("SELECT terms FROM submission_private.application_agreements")
            .fetch_optional(&public).await.is_err());

        let (web, _) = board_public::routers(public, ORIGIN.into(), false);
        let start = send(&web, ORIGIN, "", None).await;
        assert_eq!(start.status(), StatusCode::OK);
        let cookie = start.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
        let start = html(start).await;
        let initial = nonce(&start);
        let review = |code| Some(format!("code={code}&csrf={initial}"));
        assert_eq!(send(&web, ORIGIN, &cookie, review(obsolete.token())).await.status(), StatusCode::NOT_FOUND);
        let direct = format!("code={}&csrf={initial}&accept=yes", invite.token());
        assert_eq!(send(&web, ORIGIN, &cookie, Some(direct)).await.status(), StatusCode::FORBIDDEN);
        let response = send(&web, ORIGIN, &cookie, review(invite.token())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()["referrer-policy"], "strict-origin");
        let preview = html(response).await;
        assert!(preview.contains("Owned terms\n&#60;script&#62;literal&#60;/script&#62;"));
        let confirmation = nonce(&preview);
        let response = send(&web, ORIGIN, &cookie, Some(format!("code={}&csrf={confirmation}&accept=yes", obsolete.token()))).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let accept = Some(format!("code={}&csrf={confirmation}&accept=yes", invite.token()));
        assert_eq!(send(&web, "https://other.invalid", &cookie, accept.clone()).await.status(), StatusCode::FORBIDDEN);

        let (a, b) = tokio::join!(
            send(&web, ORIGIN, &cookie, accept.clone()),
            send(&web, ORIGIN, &cookie, accept),
        );
        let mut statuses = [a.status(), b.status()];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::NOT_FOUND]);
        let accepted = html(if a.status() == StatusCode::OK { a } else { b }).await;
        assert!(accepted.contains("Your acceptance was recorded."));
        assert!(!accepted.contains(invite.token()));
        assert!(!agreements::revoke(&mut operator, application).await.unwrap());
        assert!(matches!(
            agreements::issue(&mut operator, application, "Replacement terms").await,
            Err(board_store::site_submissions::admin::Error::AgreementUnavailable),
        ));
        let recorded: (i16, bool) = sqlx::query_as(
            "SELECT a.state,g.accepted_at IS NOT NULL
             FROM submission_private.applications a JOIN submission_private.application_agreements g ON g.application_id=a.id
             WHERE a.id=$1",
        ).bind(application).fetch_one(&db).await.unwrap();
        assert_eq!(recorded, (4, true));
    }).await;

    for query in [
        "DELETE FROM submission_private.application_agreements WHERE application_id=$1",
        "DELETE FROM submission_private.applications WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(application)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM submission_private.application_campaigns WHERE id=$1")
        .bind(campaign)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
