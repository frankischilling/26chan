#![cfg(feature = "database-tests")]

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use board_domain::poster_id::PosterIdKey;
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::{
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-quota-password";
const FLOOD: &str = "Error: You cannot delete posts this often.";

fn key() -> PosterIdKey {
    PosterIdKey::parse(&"73".repeat(32)).unwrap()
}

#[derive(Clone, Copy)]
enum Route {
    Modern,
    Legacy,
    Multipart,
}

impl Route {
    fn success(self) -> StatusCode {
        match self {
            Self::Modern => StatusCode::SEE_OTHER,
            _ => StatusCode::OK,
        }
    }
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    boards: [String; 2],
    peers: [SocketAddr; 4],
    hash: String,
    app: Router,
}

impl Fixture {
    fn router(public: PgPool, configured_key: bool) -> Router {
        board_public::routers_with_options(
            public,
            board_public::PublicRouterOptions {
                origin: ORIGIN.into(),
                production: false,
                media: None,
                limits: board_config::PublicRequestLimits::from_lookup(|name| {
                    (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "120".into())
                })
                .unwrap(),
                proxy_uid: None,
                poster_id_key: configured_key.then(|| Arc::new(key())),
                tripcode_key: None,
                country_database: None,
            },
        )
        .0
    }

    async fn post(&self, board: usize, attached: bool) -> i64 {
        let id = board_store::create_post(
            &self.public,
            &self.boards[board],
            0,
            &board_store::NewPost {
                name: "Anonymous".into(),
                subject: "Owned quota fixture".into(),
                comment: "Synthetic quota selection".into(),
                deletion_hash: self.hash.clone(),
                sage: false,
            },
        )
        .await
        .unwrap();
        if attached {
            // Metadata-only owned fixture; no upload job or filesystem changes.
            sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$2,'quota.png',100,10,10,false)")
                .bind(id).bind(format!("{id:032x}")).execute(&self.owner).await.unwrap();
        }
        id
    }

    async fn deleted(&self, id: i64) -> bool {
        sqlx::query_scalar("SELECT deleted FROM content.posts WHERE id=$1")
            .bind(id)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn mutated(&self, id: i64, file_only: bool) -> bool {
        if file_only {
            assert!(
                !self.deleted(id).await,
                "File deletion must preserve the post"
            );
            sqlx::query_scalar("SELECT file_deleted FROM content.post_media WHERE post_id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap()
        } else {
            self.deleted(id).await
        }
    }

    async fn count(&self, peer: SocketAddr) -> i32 {
        let identity = key().public_deletion_rate_identity(peer.ip());
        sqlx::query_scalar("SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
            .bind(identity.as_bytes().as_slice()).fetch_optional(&self.owner).await.unwrap().unwrap_or(0)
    }

    #[allow(clippy::too_many_arguments)]
    async fn submit(
        &self,
        app: &Router,
        board: usize,
        peer: Option<SocketAddr>,
        route: Route,
        ids: &[i64],
        password: &str,
        file_only: bool,
        cookie: Option<&str>,
        forwarded: Option<&str>,
    ) -> (StatusCode, String) {
        let mut fields: Vec<(String, String)> = match route {
            Route::Modern => vec![
                ("no".into(), ids[0].to_string()),
                ("password".into(), password.into()),
            ],
            _ => {
                let mut fields = vec![
                    ("mode".into(), "usrdel".into()),
                    ("pwd".into(), password.into()),
                ];
                fields.extend(ids.iter().map(|id| (id.to_string(), "delete".into())));
                fields
            }
        };
        if file_only {
            fields.push(match route {
                Route::Modern => ("file_only".into(), "true".into()),
                _ => ("onlyimgdel".into(), "on".into()),
            });
        }
        let (content_type, body) = match route {
            Route::Multipart => {
                let mut body = String::new();
                for (name, value) in &fields {
                    body.push_str(&format!("--quota\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
                }
                body.push_str("--quota--\r\n");
                ("multipart/form-data; boundary=quota", body)
            }
            _ => (
                "application/x-www-form-urlencoded",
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(&fields)
                    .finish(),
            ),
        };
        let suffix = match route {
            Route::Modern => "delete",
            _ => "imgboard.php",
        };
        let mut request = Request::post(format!("/{}/{suffix}", self.boards[board]))
            .header("origin", ORIGIN)
            .header("content-type", content_type);
        if let Some(peer) = peer {
            request = request.extension(ConnectInfo(peer));
        }
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        if let Some(ip) = forwarded {
            request = request
                .header("x-forwarded-for", ip)
                .header("x-real-ip", ip)
                .header("x-board-client-ip", ip)
                .header("forwarded", format!("for={ip}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn shared_identity(&self) {
        let peer = self.peers[0];
        // Each router has independent in-memory HTTP limits. The persisted quota
        // must still be shared across router instances, forms, boards and cookies.
        let other_router = Self::router(self.public.clone(), true);
        let cookies = [
            None,
            Some("board-anon=a1_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some("board-anon=a1_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ];
        for (i, route) in [Route::Modern, Route::Legacy, Route::Multipart]
            .into_iter()
            .enumerate()
        {
            let board = i % 2;
            let id = self.post(board, false).await;
            let (status, body) = self
                .submit(
                    &other_router,
                    board,
                    Some(SocketAddr::new(peer.ip(), 14000 + i as u16)),
                    route,
                    &[id],
                    PASSWORD,
                    false,
                    cookies[i],
                    Some("198.51.100.17"),
                )
                .await;
            assert_eq!(status, route.success(), "{body}");
            assert!(self.deleted(id).await);
            assert_eq!(self.count(peer).await, i as i32 + 1);
        }
        for route in [Route::Modern, Route::Legacy, Route::Multipart] {
            let id = self.post(1, false).await;
            let (status, body) = self.submit(&self.app, 1, Some(peer), route, &[id], PASSWORD, false,
                Some("board-anon=a1_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"), Some("203.0.113.99")).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(body.contains(FLOOD), "{body}");
            assert!(!self.deleted(id).await);
            assert_eq!(self.count(peer).await, 3);
        }
        // Source checks the actor quota before target existence, age or authority.
        // In particular, a flooded legacy single-missing selection must not fall
        // through to the normal successful "Updating index" response.
        for route in [Route::Modern, Route::Legacy, Route::Multipart] {
            let old = self.post(0, false).await;
            sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '30 days' WHERE id=$1")
                .bind(old).execute(&self.owner).await.unwrap();
            let invalid = self.post(0, false).await;
            for (id, password) in [
                (old, PASSWORD),
                (invalid, "wrong-password"),
                (i64::MAX, PASSWORD),
            ] {
                let (status, body) = self
                    .submit(
                        &self.app,
                        0,
                        Some(peer),
                        route,
                        &[id],
                        password,
                        false,
                        None,
                        None,
                    )
                    .await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
                assert!(body.contains(FLOOD), "{body}");
                assert!(!body.contains("Updating index"), "{body}");
                assert!(!body.contains("Error: Password incorrect."), "{body}");
                assert!(
                    !body.contains("Error: You cannot delete a post this old."),
                    "{body}"
                );
                assert_eq!(self.count(peer).await, 3);
            }
            assert!(!self.deleted(old).await);
            assert!(!self.deleted(invalid).await);
        }
        // Preserve source single-missing behavior when there is no actor flood.
        for route in [Route::Legacy, Route::Multipart] {
            let (status, body) = self
                .submit(
                    &self.app,
                    0,
                    Some(self.peers[2]),
                    route,
                    &[i64::MAX],
                    PASSWORD,
                    false,
                    None,
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(body.contains("Updating index"), "{body}");
            assert_eq!(self.count(self.peers[2]).await, 0);
        }
        let other = self.peers[1];
        let id = self.post(0, false).await;
        let (status, body) = self
            .submit(
                &self.app,
                0,
                Some(other),
                Route::Modern,
                &[id],
                PASSWORD,
                false,
                cookies[1],
                Some("198.51.100.17"),
            )
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        assert!(self.deleted(id).await);
        assert_eq!(self.count(other).await, 1);
        assert_eq!(self.count(peer).await, 3);
        let identity = key().public_deletion_rate_identity(peer.ip());
        let error = sqlx::query(
            "SELECT events FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
        )
        .bind(identity.as_bytes().as_slice())
        .fetch_optional(&self.public)
        .await
        .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
    }

    async fn batches(&self) {
        for (group, (route, file_only)) in [
            (Route::Legacy, false),
            (Route::Multipart, false),
            (Route::Legacy, true),
            (Route::Multipart, true),
        ]
        .into_iter()
        .enumerate()
        {
            let peer = self.peers[group];
            let mut ids = Vec::new();
            for _ in 0..5 {
                ids.push(self.post(0, file_only).await);
            }
            let (status, body) = self
                .submit(
                    &self.app,
                    0,
                    Some(peer),
                    route,
                    &ids,
                    PASSWORD,
                    file_only,
                    None,
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            for id in ids {
                assert!(self.mutated(id, file_only).await);
            }
            assert_eq!(
                self.count(peer).await,
                1,
                "Many selections charge one request"
            );
            for (expected, missing) in [(2, true), (3, false)] {
                let first = self.post(0, file_only).await;
                let denied = if missing {
                    i64::MAX
                } else {
                    let id = self.post(0, file_only).await;
                    sqlx::query("UPDATE post_secrets.deletion SET password_hash='invalid-other-owner' WHERE post_id=$1")
                        .bind(id).execute(&self.owner).await.unwrap();
                    id
                };
                let last = self.post(0, file_only).await;
                let (status, body) = self
                    .submit(
                        &self.app,
                        0,
                        Some(peer),
                        route,
                        &[first, denied, last],
                        PASSWORD,
                        file_only,
                        None,
                        None,
                    )
                    .await;
                assert_eq!(status, StatusCode::FORBIDDEN);
                assert!(
                    body.contains(if missing {
                        "Error: You cannot delete a post this old."
                    } else {
                        "Error: Password incorrect."
                    }),
                    "{body}"
                );
                assert!(self.mutated(first, file_only).await);
                assert!(!self.mutated(last, file_only).await);
                if !missing {
                    assert!(!self.mutated(denied, file_only).await);
                }
                assert_eq!(
                    self.count(peer).await,
                    expected,
                    "A later failure preserves exactly one charge"
                );
            }
            let id = self.post(0, file_only).await;
            let (status, body) = self
                .submit(
                    &self.app,
                    0,
                    Some(peer),
                    route,
                    &[id],
                    PASSWORD,
                    file_only,
                    None,
                    None,
                )
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(body.contains(FLOOD), "{body}");
            assert!(!self.mutated(id, file_only).await);
            assert_eq!(self.count(peer).await, 3);
        }
    }

    async fn failures(&self) {
        for (group, route) in [Route::Modern, Route::Legacy, Route::Multipart]
            .into_iter()
            .enumerate()
        {
            let peer = self.peers[group];
            for file_only in [false, true] {
                let id = self.post(0, file_only).await;
                let (status, body) = self
                    .submit(
                        &self.app,
                        0,
                        Some(peer),
                        route,
                        &[id],
                        "wrong-password",
                        file_only,
                        None,
                        None,
                    )
                    .await;
                assert_eq!(status, StatusCode::FORBIDDEN);
                assert!(body.contains("Error: Password incorrect."), "{body}");
                assert!(!self.mutated(id, file_only).await);
                assert_eq!(self.count(peer).await, 0);
                sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
                    .bind(id)
                    .execute(&self.owner)
                    .await
                    .unwrap();
                let (status, body) = self
                    .submit(
                        &self.app,
                        0,
                        Some(peer),
                        route,
                        &[id],
                        PASSWORD,
                        file_only,
                        None,
                        None,
                    )
                    .await;
                assert_eq!(status, StatusCode::FORBIDDEN);
                assert!(
                    body.contains("Error: You cannot delete this post."),
                    "{body}"
                );
                assert!(!self.mutated(id, file_only).await);
                assert_eq!(self.count(peer).await, 0);
            }
            // A missing attachment must roll back any attempted reservation too.
            let id = self.post(0, false).await;
            assert_eq!(
                self.submit(
                    &self.app,
                    0,
                    Some(peer),
                    route,
                    &[id],
                    PASSWORD,
                    true,
                    None,
                    None
                )
                .await
                .0,
                StatusCode::NOT_FOUND
            );
            assert!(!self.deleted(id).await);
            assert_eq!(self.count(peer).await, 0);
            assert_eq!(
                self.submit(
                    &self.app,
                    0,
                    Some(peer),
                    route,
                    &[id],
                    PASSWORD,
                    false,
                    None,
                    None
                )
                .await
                .0,
                route.success()
            );
            assert!(self.deleted(id).await);
            assert_eq!(self.count(peer).await, 1);
        }
    }

    async fn unavailable(&self) {
        let without_key = Self::router(self.public.clone(), false);
        for route in [Route::Modern, Route::Legacy, Route::Multipart] {
            for (app, peer) in [(&without_key, Some(self.peers[0])), (&self.app, None)] {
                for file_only in [false, true] {
                    let id = self.post(0, file_only).await;
                    let (status, body) = self
                        .submit(
                            app,
                            0,
                            peer,
                            route,
                            &[id],
                            PASSWORD,
                            file_only,
                            None,
                            Some("198.51.100.17"),
                        )
                        .await;
                    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
                    assert!(body.contains("Public deletion is unavailable."), "{body}");
                    assert!(!self.mutated(id, file_only).await);
                    assert_eq!(self.count(self.peers[0]).await, 0);
                }
            }
        }
    }
}

enum Scenario {
    SharedIdentity,
    Batches,
    Failures,
    Unavailable,
}

async fn run(scenario: Scenario) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(role, "board_public");
    let seed = OsRng.next_u64();
    let board_nonce = seed & 0x0fff_ffff;
    let boards = [
        format!("dq{board_nonce:07x}a"),
        format!("dq{board_nonce:07x}b"),
    ];
    let peers = std::array::from_fn(|group| {
        SocketAddr::new(
            Ipv6Addr::new(
                0x2001,
                0xdb8,
                0x85,
                group as u16,
                (seed >> 48) as u16,
                (seed >> 32) as u16,
                (seed >> 16) as u16,
                seed as u16,
            )
            .into(),
            12345,
        )
    });
    for board in &boards {
        // Explicit policy for owned synthetic boards only; production defaults remain intact.
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,deletion_known_min_seconds,deletion_unknown_min_seconds,deletion_max_seconds) VALUES($1,'Deletion quota','Owned fixtures',200,100,100,1000,10,100,0,0,86400)")
            .bind(board).execute(&owner).await.unwrap();
    }
    let fixture = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        boards: boards.clone(),
        peers,
        hash: Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string(),
        app: Fixture::router(public.clone(), true),
    };
    // Join errors are rethrown only after deleting this fixture's own rows.
    let outcome = tokio::spawn(async move {
        match scenario {
            Scenario::SharedIdentity => fixture.shared_identity().await,
            Scenario::Batches => fixture.batches().await,
            Scenario::Failures => fixture.failures().await,
            Scenario::Unavailable => fixture.unavailable().await,
        }
    })
    .await;
    for board in boards {
        for statement in [
            "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(statement)
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
        }
    }
    for peer in peers {
        let identity = key().public_deletion_rate_identity(peer.ip());
        sqlx::query("DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
            .bind(identity.as_bytes().as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}

#[tokio::test]
async fn persisted_quota_uses_trusted_peer_across_routes_boards_cookies_and_routers() {
    run(Scenario::SharedIdentity).await;
}
#[tokio::test]
async fn legacy_batches_charge_once_including_partial_success_and_file_only() {
    run(Scenario::Batches).await;
}
#[tokio::test]
async fn failed_authority_policy_and_file_mutations_do_not_charge() {
    run(Scenario::Failures).await;
}
#[tokio::test]
async fn missing_key_or_trusted_peer_fails_closed_without_mutating() {
    run(Scenario::Unavailable).await;
}
