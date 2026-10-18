use super::*;
use board_domain::content_admission::network::hints::{BrowserHeaders, BrowserKey, DerivedHints};

async fn configure(f: &Fixture, id: i64, rule: Option<&str>, global: bool, warning: bool) {
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.templates SET source_rule=$2,global=$3,warning=$4,seconds=CASE WHEN $4 THEN 0 ELSE 60 END WHERE id=$1")
        .bind(id).bind(rule).bind(global).bind(warning).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM restrictions.automatic_ranges WHERE board=$1")
        .bind(&f.board)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn issue(f: &Fixture, target: i64, template: i64) -> i64 {
    sqlx::query_scalar("SELECT content.issue_post_template_restriction($1,$2,600,$3,$4,$5)")
        .bind(&f.session)
        .bind(&f.csrf)
        .bind(&f.board)
        .bind(target)
        .bind(template)
        .fetch_one(&f.staff)
        .await
        .unwrap()
}

pub(super) async fn automatic_templates(f: &Fixture) {
    let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
    let mut mobile = f.clone();
    mobile.hints = Some(DerivedHints::derive(
        Some(&key),
        Some(BrowserHeaders {
            user_agent: Some(b"Android/1"),
            ..Default::default()
        }),
        None,
    ));
    let op = mobile
        .write(0, None, true, &post(), 71, Utc::now())
        .await
        .unwrap();
    let reply = mobile
        .write(op, None, true, &post(), 72, Utc::now())
        .await
        .unwrap();
    let unknown = f
        .write(0, None, true, &post(), 73, Utc::now())
        .await
        .unwrap();
    let mut ipv6 = mobile.clone();
    ipv6.peer_override = Some("2001:db8::1".parse().unwrap());
    let ipv6_post = ipv6
        .write(0, None, true, &post(), 74, Utc::now())
        .await
        .unwrap();
    let mut setup = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *setup)
        .await
        .unwrap();
    let template: i64 = sqlx::query_scalar("INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason) VALUES(1900000000+floor(random()*1000000)::bigint,'Owned range template','moderator',true,false,60,'Owned public','Owned private') RETURNING id")
        .fetch_one(&mut *setup).await.unwrap();
    setup.commit().await.unwrap();
    f.templates.lock().unwrap().push(template);

    // Source strpos is case-sensitive and independent of submitted ban scope.
    // Missing rule evidence preserves old presets without inventing a match.
    for (rule, global, warning, expected) in [
        (None, true, false, 0),
        (Some(""), true, false, 0),
        (Some("g1"), true, false, 0),
        (Some("Global1"), true, false, 0),
        (Some("global1"), false, false, 1),
        (Some("prefixglobal2"), true, false, 1),
        (Some("global1"), true, true, 0),
    ] {
        configure(f, template, rule, global, warning).await;
        let ban = issue(f, op, template).await;
        assert_eq!(
            automatic_stats(f).await,
            (expected, 0),
            "rule={rule:?} global={global} warning={warning}"
        );
        if expected == 1 {
            let mut tx = f.owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_restriction_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let row: (i64,i64,bool) = sqlx::query_as("SELECT parent,source_ban_id,prefix=h.automatic_prefix AND browser=h.automatic_browser FROM restrictions.automatic_ranges e JOIN post_secrets.posting_history h ON h.post_id=$2 WHERE e.board=$1")
                .bind(&f.board).bind(op).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(row, (0, ban, true));
            tx.rollback().await.unwrap();
            issue(f, op, template).await;
            assert_eq!(
                automatic_stats(f).await,
                (1, 0),
                "Repeated issuance deduplicates its range"
            );
        }
    }
    configure(f, template, Some("global1"), true, false).await;
    issue(f, op, template).await;
    let mut age = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *age)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.automatic_ranges SET created_at=floor(extract(epoch FROM clock_timestamp()))::bigint-3600 WHERE board=$1")
        .bind(&f.board).execute(&mut *age).await.unwrap();
    age.commit().await.unwrap();
    issue(f, op, template).await;
    assert_eq!(
        automatic_stats(f).await,
        (2, 0),
        "The exact one-hour boundary is not a duplicate"
    );
    configure(f, template, Some("global1"), false, false).await;
    issue(f, unknown, template).await;
    issue(f, ipv6_post, template).await;
    assert_eq!(
        automatic_stats(f).await,
        (0, 0),
        "Absent stored comparison evidence must not create a range"
    );
    let ban = issue(f, reply, template).await;
    let mut inspect = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *inspect)
        .await
        .unwrap();
    let row: (i64, i64) = sqlx::query_as(
        "SELECT parent,source_ban_id FROM restrictions.automatic_ranges WHERE board=$1",
    )
    .bind(&f.board)
    .fetch_one(&mut *inspect)
    .await
    .unwrap();
    assert_eq!(row, (op, ban));
    inspect.rollback().await.unwrap();

    // The newly connected producer must feed the real public admission path.
    assert!(matches!(
        mobile.write_public(op, None, &post(), 75, Utc::now()).await,
        Err(StoreError::AutomaticRangeRejected { .. })
    ));
    mobile
        .write_public(0, None, &post(), 76, Utc::now())
        .await
        .unwrap();

    configure(f, template, Some("global1"), true, false).await;
    let mut tx = f.staff.begin().await.unwrap();
    let rolled_back: i64 =
        sqlx::query_scalar("SELECT content.issue_post_template_restriction($1,$2,600,$3,$4,$5)")
            .bind(&f.session)
            .bind(&f.csrf)
            .bind(&f.board)
            .bind(op)
            .bind(template)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(automatic_stats(f).await, (0, 0));
    let mut inspect = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *inspect)
        .await
        .unwrap();
    let retained: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM restrictions.bans WHERE id=$1) OR EXISTS(SELECT 1 FROM restrictions.audit WHERE ban_id=$1)")
        .bind(rolled_back).fetch_one(&mut *inspect).await.unwrap();
    assert!(
        !retained,
        "Rollback must discard the ban and its audit with the event"
    );
    inspect.rollback().await.unwrap();
    for pool in [&f.public, &f.staff, &f.auth] {
        for statement in [
            "SELECT source_rule FROM restrictions.templates",
            "SELECT restrictions.record_automatic_range('g',1,1)",
        ] {
            let error = sqlx::query(statement).execute(pool).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }
    }
}
