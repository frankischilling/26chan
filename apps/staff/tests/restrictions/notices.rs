use super::*;

async fn saved(
    owner: &PgPool,
    board: &str,
    post: i64,
) -> (Vec<String>, String, chrono::DateTime<chrono::Utc>) {
    sqlx::query_as("SELECT p.staff_notices,p.comment,t.modified_at FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=$1 AND p.id=$2")
        .bind(board).bind(post).fetch_one(owner).await.unwrap()
}
async fn private_state(owner: &PgPool, board: &str) -> serde_json::Value {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let value=sqlx::query_scalar("SELECT jsonb_build_array((SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM restrictions.bans b WHERE board=$1),(SELECT jsonb_agg(to_jsonb(a) ORDER BY a.id) FROM restrictions.audit a JOIN restrictions.bans b ON b.id=a.ban_id WHERE b.board=$1),(SELECT to_jsonb(c) FROM restrictions.capacity c WHERE singleton))")
        .bind(board).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    value
}

pub(super) async fn exercise(
    owner: &PgPool,
    app: &Router,
    board: &str,
    post: i64,
    account: i64,
    csrf: &str,
    cookie: &str,
) {
    let path = format!("/ban?board={board}&target={post}");
    let base = format!(
        "board={board}&target={post}&csrf={csrf}&scope=local&kind=finite&seconds=60&public_reason=Owned+notice+ban&private_reason=Owned+private"
    );
    let original = saved(owner, board, post).await;
    assert!(original.0.is_empty());
    let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned notice report') RETURNING id")
        .bind(board).bind(post).fetch_one(owner).await.unwrap();
    let mut ids = Vec::new();
    let custom = "<script>notice</script> & \"quoted\" [spoiler]literal[/spoiler]";
    for (role, flags, all, warning, custom_text, expected) in [
        (
            "moderator",
            false,
            false,
            false,
            None,
            board_domain::post_notice::BANNED,
        ),
        (
            "moderator",
            false,
            false,
            true,
            Some(custom),
            board_domain::post_notice::WARNED,
        ),
        (
            "moderator",
            true,
            false,
            false,
            Some(custom),
            board_domain::post_notice::BANNED,
        ),
        ("moderator", true, true, false, Some(custom), custom),
        ("manager", false, false, true, Some("0"), "0"),
        ("manager", false, false, false, Some(custom), custom),
    ] {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,flags=CASE WHEN $3 THEN ARRAY['banmsg'] ELSE '{}' END,allow_boards=CASE WHEN $4 THEN ARRAY['all'] ELSE ARRAY[$5] END WHERE id=$1")
            .bind(account).bind(role).bind(flags).bind(all).bind(board).execute(owner).await.unwrap();
        let page = request(app, &path, None, cookie).await;
        assert_eq!(page.0, StatusCode::OK);
        assert_eq!(
            page.2.contains("name=\"custombanmsg\""),
            role == "manager" || (flags && all)
        );
        let mut form = if warning {
            base.replace("kind=finite&seconds=60", "kind=warning&seconds=0")
        } else {
            base.clone()
        };
        form.push_str("&banmsg=1");
        if let Some(text) = custom_text {
            form.push('&');
            form.push_str(
                &url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("custombanmsg", text)
                    .finish(),
            );
        }
        let before = saved(owner, board, post).await;
        let response = request(app, "/ban", Some(&form), cookie).await;
        assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", response.2);
        let id = response.1["location"]
            .to_str()
            .unwrap()
            .split("restriction=")
            .nth(1)
            .unwrap()
            .parse::<i64>()
            .unwrap();
        ids.push(id);
        let after = saved(owner, board, post).await;
        assert_eq!(after.0.len(), before.0.len() + 1);
        assert_eq!(after.0.last().unwrap(), expected);
        assert_eq!(&after.0[..before.0.len()], before.0.as_slice());
        assert_eq!(
            after.1, original.1,
            "Notice rewrote immutable comment bytes"
        );
        assert!(after.2 > before.2);
    }
    let cleared: (bool, Option<i64>) = sqlx::query_as(
        "SELECT auto_cleared_at IS NOT NULL,group_cleared_by FROM content.reports WHERE id=$1",
    )
    .bind(report)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(cleared, (true, None));
    // The separate report bound also refuses the entire issuance transaction.
    sqlx::query("INSERT INTO content.reports(board,post_id,reason) SELECT $1,$2,'Owned notice overflow' FROM generate_series(1,10000)")
        .bind(board).bind(post).execute(owner).await.unwrap();
    let before_overflow = saved(owner, board, post).await;
    let private_overflow = private_state(owner, board).await;
    assert_eq!(
        request(app, "/ban", Some(&format!("{base}&banmsg=1")), cookie)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(saved(owner, board, post).await, before_overflow);
    assert_eq!(private_state(owner, board).await, private_overflow);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.reports WHERE board=$1 AND post_id=$2 AND reason='Owned notice overflow' AND auto_cleared_at IS NOT NULL")
        .bind(board).bind(post).fetch_one(owner).await.unwrap(),0);
    sqlx::query("DELETE FROM content.reports WHERE board=$1 AND post_id=$2 AND reason='Owned notice overflow'")
        .bind(board).bind(post).execute(owner).await.unwrap();
    // Repeated messages remain separate, and the final permitted slot works.
    let form = format!("{base}&banmsg=1");
    for _ in ids.len()..board_domain::post_notice::MAX_NOTICES {
        let response = request(app, "/ban", Some(&form), cookie).await;
        assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", response.2);
        ids.push(
            response.1["location"]
                .to_str()
                .unwrap()
                .split("restriction=")
                .nth(1)
                .unwrap()
                .parse()
                .unwrap(),
        );
    }
    let before = saved(owner, board, post).await;
    let private = private_state(owner, board).await;
    assert_eq!(
        request(app, "/ban", Some(&form), cookie).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(saved(owner, board, post).await, before);
    assert_eq!(private_state(owner, board).await, private);
    for id in ids {
        let unban = format!("board={board}&target={post}&csrf={csrf}&id={id}");
        assert_eq!(
            request(app, "/ban/unban", Some(&unban), cookie).await.0,
            StatusCode::SEE_OTHER
        );
    }
    assert_eq!(
        saved(owner, board, post).await,
        before,
        "Unban must not remove earlier notices"
    );
    sqlx::query("DELETE FROM content.reports WHERE id=$1")
        .bind(report)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE staff_identity.accounts SET role='moderator',flags='{}',allow_boards=ARRAY[$2] WHERE id=$1")
        .bind(account).bind(board).execute(owner).await.unwrap();
}
