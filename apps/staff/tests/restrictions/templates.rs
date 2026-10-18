use super::*;
type Issued = (
    i64,
    String,
    Option<Vec<u8>>,
    bool,
    bool,
    String,
    String,
    i64,
    i64,
);

pub(super) async fn exercise(
    owner: &PgPool,
    app: &Router,
    board: &str,
    post: i64,
    csrf: &str,
    cookie: &str,
) {
    let mut setup = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *setup)
        .await
        .unwrap();
    let template: i64 = sqlx::query_scalar("INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason) VALUES(1900000000+floor(random()*1000000)::bigint,'<Owned template>','manager',true,false,60,'Owned template public','Owned template private') RETURNING id")
        .fetch_one(&mut *setup).await.unwrap();
    setup.commit().await.unwrap();
    let path = format!("/ban?board={board}&target={post}");
    let form = format!("board={board}&target={post}&csrf={csrf}&template={template}");
    assert!(
        !request(app, &path, None, cookie)
            .await
            .2
            .contains("&#60;Owned template&#62;")
    );
    assert_eq!(
        request(app, "/ban/template", Some(&form), cookie).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(app, "/ban/template", Some(&form), "").await.0,
        StatusCode::UNAUTHORIZED
    );
    let bad = form.replace(&format!("csrf={csrf}"), "csrf=wrong");
    assert_eq!(
        request(app, "/ban/template", Some(&bad), cookie).await.0,
        StatusCode::FORBIDDEN
    );
    let mut config = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *config)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.templates SET minimum_role='moderator' WHERE id=$1")
        .bind(template)
        .execute(&mut *config)
        .await
        .unwrap();
    config.commit().await.unwrap();
    let page = request(app, &path, None, cookie).await;
    assert_eq!(page.0, StatusCode::OK);
    assert!(page.2.contains("&#60;Owned template&#62;"), "{}", page.2);
    assert!(!page.2.contains("<Owned template>"));
    assert!(page.2.contains("All boards, Ban for 60 seconds"));
    assert!(page.2.contains("Owned template public"));
    assert!(!page.2.contains("Owned template private"));
    // The template request cannot replace its configured reason or duration.
    assert!(
        request(
            app,
            "/ban/template",
            Some(&(form.clone() + "&public_reason=forged")),
            cookie
        )
        .await
        .0
        .is_client_error()
    );
    assert!(
        request(
            app,
            "/ban/template",
            Some(&form.replace(&format!("template={template}"), "template=0")),
            cookie
        )
        .await
        .0
        .is_client_error()
    );
    let response = request(app, "/ban/template", Some(&form), cookie).await;
    assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", response.2);
    let ban: i64 = response.1["location"]
        .to_str()
        .unwrap()
        .split("restriction=")
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut check = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *check)
        .await
        .unwrap();
    let row: Issued = sqlx::query_as("SELECT template_id,upload_observation,normalized_upload_md5,global,warning,public_reason,private_reason,extract(epoch FROM ends_at-starts_at)::bigint,charged_bytes FROM restrictions.bans WHERE id=$1")
        .bind(ban).fetch_one(&mut *check).await.unwrap();
    assert_eq!(
        row,
        (
            template,
            "absent".into(),
            None,
            true,
            false,
            "Owned template public".into(),
            "Owned template private".into(),
            60,
            (512 + "Owned template public".len() + "Owned template private".len() + 64) as i64
        )
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM restrictions.bans WHERE template_id=$1")
            .bind(template)
            .fetch_one(&mut *check)
            .await
            .unwrap();
    assert_eq!(count, 1, "Denied template requests must not create bans");
    check.commit().await.unwrap();
    for (warning, seconds) in [(true, Some(0_i64)), (false, None)] {
        let mut config = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *config)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.templates SET warning=$2,seconds=$3 WHERE id=$1")
            .bind(template)
            .bind(warning)
            .bind(seconds)
            .execute(&mut *config)
            .await
            .unwrap();
        config.commit().await.unwrap();
        let response = request(app, "/ban/template", Some(&form), cookie).await;
        assert_eq!(response.0, StatusCode::SEE_OTHER, "{}", response.2);
        let configured: i64 = response.1["location"]
            .to_str()
            .unwrap()
            .split("restriction=")
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let mut inspect = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *inspect)
            .await
            .unwrap();
        let duration: (bool,Option<i64>) = sqlx::query_as("SELECT warning,extract(epoch FROM ends_at-starts_at)::bigint FROM restrictions.bans WHERE id=$1")
            .bind(configured).fetch_one(&mut *inspect).await.unwrap();
        assert_eq!(duration, (warning, seconds));
        inspect.rollback().await.unwrap();
        let unban = format!("board={board}&target={post}&csrf={csrf}&id={configured}");
        assert_eq!(
            request(app, "/ban/unban", Some(&unban), cookie).await.0,
            StatusCode::SEE_OTHER
        );
    }
    let mut check = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *check)
        .await
        .unwrap();
    // Removing a configured preset cannot erase a past authorized selection.
    sqlx::query("DELETE FROM restrictions.templates WHERE id=$1")
        .bind(template)
        .execute(&mut *check)
        .await
        .unwrap();
    check.commit().await.unwrap();
    assert!(
        request(app, "/ban/template", Some(&form), cookie)
            .await
            .0
            .is_client_error()
    );
    let unban = format!("board={board}&target={post}&csrf={csrf}&id={ban}");
    assert_eq!(
        request(app, "/ban/unban", Some(&unban), cookie).await.0,
        StatusCode::SEE_OTHER
    );
    let mut check = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *check)
        .await
        .unwrap();
    let retained: (bool, i64, String) = sqlx::query_as(
        "SELECT active,template_id,upload_observation FROM restrictions.bans WHERE id=$1",
    )
    .bind(ban)
    .fetch_one(&mut *check)
    .await
    .unwrap();
    assert_eq!(retained, (false, template, "absent".into()));
    check.rollback().await.unwrap();
}
