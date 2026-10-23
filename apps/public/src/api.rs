mod directory_data;

use crate::{
    AppState,
    handlers::AppError,
    views::{Comment, PostView},
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use board_store::{Board, Post, Thread};
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn response(
    limits: &crate::security::Limits,
    value: impl Serialize,
    modified: Option<DateTime<Utc>>,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let bytes = crate::output::json(limits.response_writer(usize::MAX), &value)?;
    bytes_response(bytes, modified, headers)
}

pub(crate) fn bytes_response(
    bytes: board_http::EncodedResponse,
    modified: Option<DateTime<Utc>>,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks() {
        hash.update(chunk);
    }
    let etag = format!("\"{:x}\"", hash.finalize());
    let unchanged = if let Some(candidate) = headers.get("if-none-match") {
        candidate.to_str().is_ok_and(|c| {
            c.split(',').any(|tag| {
                let tag = tag.trim().trim_start_matches("W/");
                tag == etag || tag == "*"
            })
        })
    } else {
        // Strict comparison deliberately revalidates same-second changes.
        modified.is_some_and(|m| {
            headers
                .get("if-modified-since")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| httpdate::parse_http_date(h).ok())
                .is_some_and(|since| std::time::SystemTime::from(m) < since)
        })
    };
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([("content-type", "application/json")], bytes.into_body()).into_response()
    };
    response.headers_mut().insert(
        "etag",
        HeaderValue::from_str(&etag).map_err(|_| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid cache validator.",
            )
        })?,
    );
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("public, max-age=0, must-revalidate"),
    );
    if let Some(modified) = modified {
        let value = httpdate::fmt_http_date(modified.into());
        response.headers_mut().insert(
            "last-modified",
            HeaderValue::from_str(&value).map_err(|_| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid modification date.",
                )
            })?,
        );
    }
    Ok(response)
}

struct OrderedFlags(Vec<(&'static str, &'static str)>);
impl Serialize for OrderedFlags {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (code, label) in &self.0 {
            map.serialize_entry(code, label)?;
        }
        map.end()
    }
}
#[derive(Serialize)]
struct BoardMetadata {
    #[serde(flatten)]
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    board_flags: Option<OrderedFlags>,
}
#[derive(Serialize)]
struct BoardDirectory {
    boards: Vec<BoardMetadata>,
}
pub async fn boards(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let mut directory = board_store::boards(&state.pool).await?;
    directory.sort_by(|a, b| a.slug.cmp(&b.slug));
    let boards = directory
        .iter()
        .filter(|board| board.json_enabled)
        .map(|board| {
            let mut metadata = board_metadata(board);
            finalize_directory_metadata(&mut metadata.value);
            metadata
        })
        .collect();
    response(&state.limits, BoardDirectory { boards }, None, &headers)
}

// rebuild_boards_json applies these transformations after catalog-info RPC.
// Keep them out of the legacy catalog-info object itself.
fn finalize_directory_metadata(value: &mut Value) {
    if value["board"] == "vp" {
        value["title"] = json!("Pokémon");
    }
    if let Some(text) = value["meta_description"].as_str()
        && !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
    {
        let number = text.bytes().fold(0_i64, |number, byte| {
            number
                .saturating_mul(10)
                .saturating_add(i64::from(byte - b'0'))
        });
        value["meta_description"] = json!(number);
    }
}

fn board_metadata(board: &Board) -> BoardMetadata {
    let mut value = json!({
    "board": board.slug, "title": board.title, "ws_board": i32::from(board.worksafe),
    "per_page": board.threads_per_page, "pages": (board.thread_limit + board.threads_per_page - 1) / board.threads_per_page,
    "max_filesize": board.max_image_bytes, "max_webm_filesize": board.max_video_bytes, "max_webm_duration": board.max_video_duration,
    "max_comment_chars": board.max_comment_chars, "bump_limit": board.bump_limit, "image_limit": board.image_limit,
    "cooldowns": {
        "threads": board.posting_thread_seconds,
        "replies": board.posting_reply_seconds,
        "images": board.posting_image_seconds
    },
    "meta_description": directory_data::description(&board.slug, &board.description)
    });
    // Source catalog-info describes configured policy, not deployment
    // health. Upload handlers still require their isolated media boundary.
    if board.text_only {
        value["text_only"] = json!(1);
    }
    if board.archives_enabled() {
        value["is_archived"] = json!(1);
    }
    if board.require_subject || board.text_only {
        value["require_subject"] = json!(1);
    }
    if board.user_ids {
        value["user_ids"] = json!(1);
    }
    if board.comment_code_spacing {
        value["code_tags"] = json!(1);
    }
    if board.comment_sjis_spacing {
        value["sjis_tags"] = json!(1);
    }
    if board.math_tags {
        value["math_tags"] = json!(1);
    }
    if board.oekaki {
        value["oekaki"] = json!(1);
    }
    if board.country_flags {
        value["country_flags"] = json!(1);
    }
    let flags = (!board.board_flags.is_empty()).then(|| OrderedFlags(board.flag_options()));
    if board.forced_anon {
        value["forced_anon"] = json!(1);
    }
    if board.comment_spoiler_cleanup {
        value["spoilers"] = json!(1);
        if board.custom_spoiler_count > 0 {
            value["custom_spoilers"] = json!(board.custom_spoiler_count);
        }
    }
    if board.video_audio {
        value["webm_audio"] = json!(1);
    }
    if board.min_image_width > 1 {
        value["min_image_width"] = json!(board.min_image_width);
    }
    if board.min_image_height > 1 {
        value["min_image_height"] = json!(board.min_image_height);
    }
    BoardMetadata {
        value,
        board_flags: flags,
    }
}

pub(crate) async fn catalog_info(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    if !crate::legacy_report::safe_board(slug) {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid board."));
    }
    let board = board_store::board(&state.pool, slug).await?;
    require_json(&board, true)?;
    if board.staff_only {
        return Err(AppError(StatusCode::NOT_FOUND, "Board not found."));
    }
    let mut metadata = board_metadata(&board);
    let fields = metadata
        .value
        .as_object_mut()
        .expect("board metadata object");
    fields.remove("board");
    fields.remove("title");
    response(&state.limits, metadata, None, headers)
}

fn tail_op_json(
    thread: &Thread,
    board: &Board,
    replies: usize,
    images: usize,
    original: &Value,
) -> Value {
    let mut op = json!({"no": thread.id, "replies": replies, "images": images,
            "bumplimit": i32::from(board_domain::bump::limited(thread.sticky, thread.permaage, replies as u64, board.bump_limit as u32)),
            "imagelimit": i32::from(board_domain::image_limit::json_limited(thread.sticky, thread.permaage, thread.undead, images as u64, board.image_limit as u32))});
    for key in [
        "sticky",
        "sticky_cap",
        "closed",
        "archived",
        "unique_ips",
        "custom_spoiler",
    ] {
        if let Some(value) = original.get(key) {
            op[key] = value.clone();
        }
    }
    op
}

struct QuoteContext<'a> {
    targets: &'a board_store::QuoteTargets,
    current_thread: Option<i64>,
}

fn post_json(
    post: Post,
    thread: &Thread,
    board: &Board,
    replies: usize,
    images: usize,
    unique_ips: Option<i32>,
    quotes: QuoteContext<'_>,
) -> Result<Value, AppError> {
    let post = PostView::resolved(post, quotes.targets, quotes.current_thread);
    let comment = Comment {
        staff_notices: &post.post.staff_notices,
        lines: &post.lines,
        board: &board.slug,
        dice_result: post.post.separate_dice_result(),
        fortune_text: post.post.separate_fortune_text(),
        fortune_color: post.post.separate_fortune_color(),
        drawing: post.drawing_annotation(),
    }
    .render_json(post.post.comment_format)?;
    let op = post.post.id == thread.id;
    let mut value = json!({ "no": post.post.id, "resto": if op { 0 } else { thread.id },
        "now": post.now, "time": post.post.created_at.timestamp() });
    let identity = board_domain::capcode::json_identity(
        &post.post.name,
        post.post.trip.as_deref(),
        post.post.capcode.as_deref(),
        board.forced_anon,
        board.meta_board,
    );
    if let Some(name) = identity.name {
        value["name"] = json!(board_domain::source_html_entities(name));
    }
    if let Some(id) = json_poster_id(&post.post, op, thread.archived_at.is_some()) {
        value["id"] = json!(id);
    }
    if let Some(trip) = identity.trip {
        value["trip"] = json!(trip);
    }
    if (1..10000).contains(&post.post.since4pass) {
        value["since4pass"] = json!(post.post.since4pass);
    }
    if let Some(capcode) = &post.post.capcode {
        value["capcode"] = json!(capcode);
    }
    if post.post.capcode.is_none() {
        if !board.board_flags.is_empty()
            && let Some(code) = &post.post.board_flag
            && let Some(flag) = board_domain::board_flags::flag(&board.board_flag_type, code)
        {
            value["board_flag"] = json!(code);
            value["flag_name"] = json!(flag.display);
        } else if board.country_flags
            && let (Some(code), Some(name)) = (&post.post.country, &post.post.country_name)
        {
            value["country"] = json!(code);
            value["country_name"] = json!(name);
        }
    }
    if !comment.is_empty() {
        value["com"] = json!(comment);
    }
    let mut subject = post.post.subject.as_str();
    if op
        && board.upload_board
        && let Some((prefix, title)) = subject.split_once('|')
        && !prefix.is_empty()
        && prefix.bytes().all(|byte| byte.is_ascii_digit())
    {
        let tags = [
            "Hentai", "Japanese", "Anime", "Game", "Other", "Loop", "Porn",
        ];
        value["tag"] = json!(
            prefix
                .parse::<usize>()
                .ok()
                .and_then(|index| tags.get(index))
        );
        subject = title;
    }
    if !subject.is_empty() {
        value["sub"] = json!(board_domain::source_html_entities(subject));
    }
    if op {
        if board.comment_spoiler_cleanup {
            value["custom_spoiler"] = json!(board.custom_spoiler_count);
        }
        value["replies"] = json!(replies);
        value["images"] = json!(images);
        if board.show_thread_uniques
            && let Some(count) = unique_ips.filter(|count| *count > 0)
        {
            value["unique_ips"] = json!(count);
        }
        let context = crate::semantic_thread::context(&post, board)?;
        value["semantic_url"] = json!(context);
        if thread.sticky {
            value["sticky"] = json!(1);
            if thread.undead {
                value["sticky_cap"] = json!(board.reply_limit);
            }
        }
        if thread.closed || thread.archived_at.is_some() {
            value["closed"] = json!(1);
        }
        if let Some(archived_at) = thread.archived_at {
            value["archived"] = json!(1);
            value["archived_on"] = json!(archived_at.timestamp());
        }
        if !thread.sticky && !thread.permaage {
            value["bumplimit"] = json!(i32::from(board_domain::bump::limited(
                thread.sticky,
                thread.permaage,
                replies as u64,
                board.bump_limit as u32,
            )));
        }
        if !thread.sticky {
            value["imagelimit"] = json!(i32::from(board_domain::image_limit::json_limited(
                thread.sticky,
                thread.permaage,
                thread.undead,
                images as u64,
                board.image_limit as u32,
            )));
        }
    }
    if post.post.image_spoiler {
        value["spoiler"] = json!(1);
    }
    if post
        .post
        .attachment
        .as_ref()
        .is_some_and(|file| file.has_mobile)
    {
        value["m_img"] = json!(1);
    }
    if post.post.file_deleted() {
        value["filedeleted"] = json!(1);
    } else if let Some(file) = post.post.attachment {
        value["tim"] = json!(file.tim);
        value["filename"] = json!(board_domain::file_name::basename(&file.filename));
        value["ext"] = json!(format!(".{}", file.output_format.extension()));
        value["fsize"] = json!(file.bytes);
        value["w"] = json!(file.width);
        value["h"] = json!(file.height);
        if let Some(md5) = file.md5 {
            value["md5"] = json!(md5);
        }
        if let (Some(width), Some(height)) = (file.thumbnail_width, file.thumbnail_height) {
            value["tn_w"] = json!(width);
            value["tn_h"] = json!(height);
        }
    }
    Ok(value)
}

fn json_poster_id(post: &board_store::Post, op: bool, archived: bool) -> Option<&str> {
    if archived || post.poster_id.is_none() {
        return None;
    }
    if op && post.capcode.is_none() {
        post.json_op_poster_id
            .as_deref()
            .or_else(|| post.poster_id.as_deref().filter(|id| *id != "Heaven"))
    } else {
        post.poster_id.as_deref()
    }
}

// A read-only alias lets browser CSP allow watcher requests by path without
// granting connect access to the public posting routes or other origins.
pub async fn watcher_thread(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let id = key
        .strip_suffix(".json")
        .and_then(|id| id.parse::<i64>().ok())
        .filter(|id| *id > 0)
        .ok_or(AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    thread(&state, &board, id, &headers).await
}

pub async fn thread(
    state: &AppState,
    slug: &str,
    id: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    thread_selection(state, slug, id, headers, false).await
}

pub async fn thread_selection(
    state: &AppState,
    slug: &str,
    id: i64,
    headers: &HeaderMap,
    tail: bool,
) -> Result<Response, AppError> {
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
        replies,
        images,
        unique_ips,
        tail_size,
        tail_id,
        quote_targets,
    } = board_store::thread_snapshot_selection(&state.pool, slug, id, tail).await?;
    require_json(&board, true)?;
    let capcode_replies = if board.meta_board && !tail {
        badge_reply_groups(
            posts
                .iter()
                .filter(|post| post.id != thread.id)
                .filter_map(|post| post.capcode.as_deref().map(|capcode| (post.id, capcode))),
        )?
    } else {
        None
    };
    let mut posts: Vec<Value> = posts
        .into_iter()
        .map(|post| {
            post_json(
                post,
                &thread,
                &board,
                replies,
                images,
                unique_ips,
                QuoteContext {
                    targets: &quote_targets,
                    current_thread: Some(thread.id),
                },
            )
        })
        .collect::<Result<_, _>>()?;
    if tail {
        let original = &posts[0];
        let mut op = tail_op_json(&thread, &board, replies, images, original);
        op["tail_size"] = json!(tail_size);
        op["tail_id"] = json!(tail_id);
        posts[0] = op;
    } else if tail_size > 0 {
        posts[0]["tail_size"] = json!(tail_size);
    }
    if let Some(groups) = capcode_replies {
        posts[0]["capcode_replies"] = groups;
    }
    response(
        &state.limits,
        json!({"posts": posts}),
        (!quote_targets.has_dependencies()).then_some(thread.http_modified_at),
        headers,
    )
}

pub async fn archive(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = board_store::archive_snapshot(&state.pool, slug).await?;
    require_json(&snapshot.board, true)?;
    let ids: Vec<_> = snapshot.entries.into_iter().map(|entry| entry.id).collect();
    response(&state.limits, json!(ids), None, headers)
}

fn preview_thread(
    board: &Board,
    preview: board_store::ThreadPreview,
    quote_targets: &board_store::QuoteTargets,
) -> Result<Vec<Value>, AppError> {
    let posts = preview.posts;
    if posts.is_empty() {
        return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
    }
    let replies = checked_reply_count(preview.visible_posts)?;
    let images = usize::try_from(preview.visible_images)
        .map_err(|_| AppError(StatusCode::SERVICE_UNAVAILABLE, "Invalid image count."))?;
    let capcode_replies = if board.meta_board {
        badge_reply_groups(
            preview
                .capcode_replies
                .iter()
                .map(|(id, badge)| (*id, badge.as_str())),
        )?
    } else {
        None
    };
    let mut posts: Vec<_> = posts
        .into_iter()
        .map(|post| {
            post_json(
                post,
                &preview.thread,
                board,
                replies,
                images,
                None,
                QuoteContext {
                    targets: quote_targets,
                    current_thread: None,
                },
            )
        })
        .collect::<Result<_, _>>()?;
    if let Some(groups) = capcode_replies {
        posts[0]["capcode_replies"] = groups;
    }
    // Index and catalog share the same preview omissions. Only full/tail
    // thread responses carry unique_ips in the source JSON contract.
    let shown_replies = posts.len() - 1;
    if replies > shown_replies {
        posts[0]["omitted_posts"] = json!(replies - shown_replies);
        let shown_images = posts
            .iter()
            .skip(1)
            .filter(|post| post.get("ext").is_some())
            .count();
        posts[0]["omitted_images"] = json!(images.saturating_sub(shown_images));
    }
    Ok(posts)
}

fn badge_reply_groups<'a>(
    replies: impl IntoIterator<Item = (i64, &'a str)>,
) -> Result<Option<Value>, AppError> {
    let groups = board_domain::capcode::capcode_reply_groups(replies).map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Badge reply groups exceed the thread limit.",
        )
    })?;
    Ok((!groups.is_empty()).then(|| json!(groups)))
}

fn checked_reply_count(total: i64) -> Result<usize, AppError> {
    usize::try_from(total)
        .ok()
        .and_then(|count| count.checked_sub(1))
        .ok_or(AppError(StatusCode::NOT_FOUND, "Thread not found."))
}

pub async fn thread_list(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot =
        board_store::board_snapshot(&state.pool, slug, board_store::BoardSelection::All, None)
            .await?;
    let board = snapshot.board;
    require_json(&board, board.json_threads_enabled)?;
    let threads = snapshot.threads;
    let mut pages = Vec::new();
    for (index, chunk) in threads.chunks(board.threads_per_page as usize).enumerate() {
        let mut entries = Vec::new();
        for preview in chunk {
            let thread = &preview.thread;
            let count = preview.visible_posts;
            if count == 0 {
                continue;
            }
            entries.push(json!({"no": thread.id, "last_modified": thread.modified_at.timestamp(), "replies": count-1}));
        }
        pages.push(json!({"page": index+1, "threads": entries}));
    }
    // An ETag covers removal of the most recently modified thread too.
    response(&state.limits, json!(pages), None, headers)
}

fn catalog_entry(posts: &[Value], modified: DateTime<Utc>) -> Value {
    let mut op = posts[0].clone();
    op["last_modified"] = json!(modified.timestamp());
    if op["replies"].as_u64().is_some_and(|replies| replies > 0) {
        op["last_replies"] = json!(posts[1..]);
    }
    op
}

pub async fn catalog(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = board_store::source_json_board_snapshot(
        &state.pool,
        slug,
        board_store::BoardSelection::All,
    )
    .await?;
    let board = snapshot.board;
    require_json(&board, board.json_catalog_enabled)?;
    let mut pages = Vec::new();
    let mut threads = snapshot.threads.into_iter().peekable();
    while threads.peek().is_some() {
        let mut entries = Vec::new();
        for preview in threads.by_ref().take(board.threads_per_page as usize) {
            let modified = preview.thread.modified_at;
            let posts = match preview_thread(&board, preview, &snapshot.quote_targets) {
                Ok(posts) => posts,
                Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
                Err(error) => return Err(error),
            };
            entries.push(catalog_entry(&posts, modified));
        }
        pages.push(json!({"page": pages.len()+1, "threads": entries}));
    }
    response(&state.limits, json!(pages), None, headers)
}

pub async fn index(
    state: &AppState,
    slug: &str,
    page: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = board_store::source_json_board_snapshot(
        &state.pool,
        slug,
        board_store::BoardSelection::Page(page),
    )
    .await?;
    let board = snapshot.board;
    require_json(&board, board.json_indexes_enabled)?;
    let mut entries = Vec::new();
    for preview in snapshot.threads {
        let total = preview.visible_posts;
        let sticky = preview.thread.sticky;
        let undead = preview.thread.undead;
        let mut posts = match preview_thread(&board, preview, &snapshot.quote_targets) {
            Ok(posts) => posts,
            Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
            Err(error) => return Err(error),
        };
        // generate_thread_json advertises an eligible tail on index OPs too,
        // even when this preview omits every reply. Catalog entries omit it.
        let tail_size = board_domain::thread_tail_size(
            board.json_tail_size as u16,
            sticky,
            undead,
            checked_reply_count(total)?,
        );
        if tail_size > 0 {
            posts[0]["tail_size"] = json!(tail_size);
        }
        entries.push(json!({"posts": posts}));
    }
    response(&state.limits, json!({"threads": entries}), None, headers)
}

fn require_json(board: &Board, feed_enabled: bool) -> Result<(), AppError> {
    if !board.json_enabled || !feed_enabled {
        return Err(AppError(StatusCode::NOT_FOUND, "JSON resource not found."));
    }
    Ok(())
}

#[cfg(test)]
mod poster_id_projection_tests {
    use super::json_poster_id;
    use serde_json::Value;

    #[test]
    fn id_projection_matches_every_independent_source_case() {
        let display: Value = serde_json::from_str(include_str!(
            "../../../fixtures/poster-id-display-reference.json"
        ))
        .unwrap();
        let reference: Value = serde_json::from_str(include_str!(
            "../../../fixtures/poster-id-json-reference.json"
        ))
        .unwrap();
        let cases = reference["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 448);
        for case in cases {
            let row = &display["cases"][case["display_case"].as_u64().unwrap() as usize];
            let ordinary = row["capcode"] == "none";
            let op = case["op"].as_bool().unwrap();
            let archived = case["archived"].as_bool().unwrap();
            let saved = row["expected"].as_str().map(str::to_owned);
            let post = board_store::Post {
                staff_notices: Vec::new(),
                id: if op { 42 } else { 43 },
                board: "test".into(),
                thread_id: 42,
                name: "Owned source ID".into(),
                trip: None,
                since4pass: 0,
                json_op_poster_id: (ordinary && op && saved.is_some()).then(|| "Ab12+/CD".into()),
                poster_id: saved,
                capcode: (!ordinary).then(|| row["capcode"].as_str().unwrap().into()),
                country: None,
                country_name: None,
                board_flag: None,
                board_flag_type: "pol".into(),
                flag_name: None,
                subject: String::new(),
                comment: String::new(),
                image_spoiler: false,
                comment_format: 0,
                staff_authorized_limits: false,
                wordfilter_payload: None,
                saved_presentation: None,
                dice_result: None,
                fortune_text: None,
                fortune_color: None,
                drawing_time_seconds: None,
                drawing_source_post_id: None,
                created_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
                deleted: false,
                file_deleted: false,
                attachment: None,
            };
            assert_eq!(
                json_poster_id(&post, op, archived),
                case["expected"].as_str(),
                "{row}: {case}"
            );
            if ordinary && op && row["expected"] == reference["network_label_stub"] {
                let mut historical = post.clone();
                historical.json_op_poster_id = None;
                assert_eq!(
                    json_poster_id(&historical, op, archived),
                    case["expected"].as_str()
                );
            }
            if ordinary && op && row["expected"] == "Heaven" {
                let mut historical = post.clone();
                historical.json_op_poster_id = None;
                assert_eq!(json_poster_id(&historical, op, archived), None);
                assert_eq!(historical.poster_id.as_deref(), Some("Heaven"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_membership_year_matches_source_json_omission_rules() {
        let fixture = crate::native_updater_snapshot::tests::fixture();
        for year in [0, 2020, 9999, 10000, 10017, 10018, 12024] {
            for capcode in [None, Some("mod".to_owned())] {
                let mut post = fixture.posts[0].clone();
                post.since4pass = year;
                post.capcode = capcode;
                let value = post_json(
                    post,
                    &fixture.thread,
                    &fixture.board,
                    0,
                    0,
                    None,
                    QuoteContext {
                        targets: &fixture.quote_targets,
                        current_thread: Some(fixture.thread.id),
                    },
                )
                .unwrap();
                if (1..10000).contains(&year) {
                    assert_eq!(value["since4pass"], json!(year));
                } else {
                    assert!(value.get("since4pass").is_none());
                }
                assert!(value.get("pass_id").is_none());
                assert!(value.get("entitlement_id").is_none());
            }
        }
    }

    #[test]
    fn retained_jpeg_metadata_preserves_mobile_marker_but_hides_deleted_urls() {
        let fixture = crate::native_updater_snapshot::tests::fixture();
        for (deleted, mobile) in [(false, true), (false, false), (true, true)] {
            let mut post = fixture.posts[0].clone();
            post.file_deleted = deleted;
            post.attachment = Some(board_store::post_media::PostAttachment {
                post_id: post.id,
                asset_id: "a".repeat(32),
                filename: "owned.foo-bar".into(),
                bytes: 100,
                width: 2000,
                height: 1000,
                spoiler: false,
                file_deleted: false,
                tim: 42,
                md5: None,
                thumbnail_width: Some(250),
                thumbnail_height: Some(125),
                output_format: board_store::media_assets::MediaFormat::Jpeg,
                has_replay: false,
                has_mobile: mobile,
            });
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            assert_eq!(value.get("m_img"), mobile.then_some(&json!(1)));
            if deleted {
                for key in ["ext", "filename", "w", "h"] {
                    assert!(value.get(key).is_none());
                }
            } else {
                assert_eq!(value["ext"], ".jpg");
                assert_eq!(value["filename"], "owned.foo-bar");
                assert_eq!(value["w"], 2000);
                assert_eq!(value["tn_w"], 250);
                assert_eq!(value.get("m_img"), mobile.then_some(&json!(1)));
            }
        }
    }

    #[test]
    fn gif_metadata_uses_approved_format_and_deleted_files_omit_the_url_fields() {
        let fixture = crate::native_updater_snapshot::tests::fixture();
        for (deleted, historical, source) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, false, true),
            (false, true, true),
        ] {
            let mut post = fixture.posts[0].clone();
            post.file_deleted = deleted;
            post.attachment = Some(board_store::post_media::PostAttachment {
                post_id: post.id,
                asset_id: "0".repeat(32),
                filename: "owned.png".into(),
                bytes: 100,
                width: if source { 10_000 } else { 32 },
                height: if source { 10_000 } else { 24 },
                spoiler: false,
                file_deleted: historical,
                tim: 42,
                md5: source.then(|| "BwcHBwcHBwcHBwcHBwcHBw==".into()),
                thumbnail_width: Some(if source { 250 } else { 32 }),
                thumbnail_height: Some(if source { 250 } else { 24 }),
                output_format: board_store::media_assets::MediaFormat::Gif,
                has_replay: false,
                has_mobile: false,
            });
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            if deleted || historical {
                assert_eq!(value["filedeleted"], 1);
                for key in [
                    "ext", "tim", "filename", "fsize", "w", "h", "tn_w", "tn_h", "md5",
                ] {
                    assert!(value.get(key).is_none());
                }
            } else {
                assert_eq!(value["ext"], ".gif");
                assert_eq!(value["filename"], "owned");
                assert_eq!(value["tn_w"], if source { 250 } else { 32 });
                assert_eq!(value["w"], if source { 10_000 } else { 32 });
                assert_eq!(
                    value.get("md5"),
                    source.then_some(&json!("BwcHBwcHBwcHBwcHBwcHBw=="))
                );
                assert!(value.get("m_img").is_none());
            }
        }
    }
    #[test]
    fn pdf_metadata_uses_approved_format_and_deleted_files_omit_the_url_fields() {
        let fixture = crate::native_updater_snapshot::tests::fixture();
        for (deleted, historical) in [(false, false), (true, false), (false, true)] {
            let mut post = fixture.posts[0].clone();
            post.file_deleted = deleted;
            post.attachment = Some(board_store::post_media::PostAttachment {
                post_id: post.id,
                asset_id: "0".repeat(32),
                filename: "owned.pdf".into(),
                bytes: 100,
                width: 1,
                height: 1,
                spoiler: false,
                file_deleted: historical,
                tim: 42,
                md5: None,
                thumbnail_width: Some(32),
                thumbnail_height: Some(24),
                output_format: board_store::media_assets::MediaFormat::Pdf,
                has_replay: false,
                has_mobile: false,
            });
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            if deleted || historical {
                assert_eq!(value["filedeleted"], 1);
                for key in [
                    "ext", "tim", "filename", "fsize", "w", "h", "tn_w", "tn_h", "md5",
                ] {
                    assert!(value.get(key).is_none());
                }
            } else {
                assert_eq!(value["ext"], ".pdf");
                assert_eq!(value["filename"], "owned");
                assert_eq!(value["w"], 1);
                assert_eq!(value["h"], 1);
                assert_eq!(value["tn_w"], 32);
                assert_eq!(value["tn_h"], 24);
                assert!(value.get("md5").is_none());
            }
        }
    }
    #[test]
    fn video_metadata_uses_approved_format_and_deleted_files_omit_the_url_fields() {
        for format in [
            board_store::media_assets::MediaFormat::Webm,
            board_store::media_assets::MediaFormat::Mp4,
        ] {
            let fixture = crate::native_updater_snapshot::tests::fixture();
            for (deleted, historical) in [(false, false), (true, false), (false, true)] {
                let mut post = fixture.posts[0].clone();
                post.file_deleted = deleted;
                post.attachment = Some(board_store::post_media::PostAttachment {
                    post_id: post.id,
                    asset_id: "0".repeat(32),
                    filename: "owned.different-name".into(),
                    bytes: 100,
                    width: 1,
                    height: 1,
                    spoiler: false,
                    file_deleted: historical,
                    tim: 42,
                    md5: None,
                    thumbnail_width: Some(32),
                    thumbnail_height: Some(24),
                    output_format: format,
                    has_replay: false,
                    has_mobile: false,
                });
                let value = post_json(
                    post,
                    &fixture.thread,
                    &fixture.board,
                    0,
                    0,
                    None,
                    QuoteContext {
                        targets: &fixture.quote_targets,
                        current_thread: Some(fixture.thread.id),
                    },
                )
                .unwrap();
                if deleted || historical {
                    assert_eq!(value["filedeleted"], 1);
                    for key in [
                        "ext", "tim", "filename", "fsize", "w", "h", "tn_w", "tn_h", "md5",
                    ] {
                        assert!(value.get(key).is_none());
                    }
                } else {
                    assert_eq!(value["ext"], format!(".{}", format.extension()));
                    // The source removes only ASCII-alphanumeric suffixes.
                    assert_eq!(value["filename"], "owned.different-name");
                    assert_eq!(value["w"], 1);
                    assert_eq!(value["h"], 1);
                    assert_eq!(value["tn_w"], 32);
                    assert_eq!(value["tn_h"], 24);
                    assert!(value.get("md5").is_none());
                }
            }
        }
    }

    #[test]
    fn post_level_file_deletion_serializes_without_inventing_attachment_metadata() {
        let fixture = crate::native_updater_snapshot::tests::fixture();
        for deleted in [false, true] {
            let mut post = fixture.posts[0].clone();
            post.file_deleted = deleted;
            post.comment = "Preserved text-only comment".into();
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            assert_eq!(value.get("filedeleted"), deleted.then_some(&json!(1)));
            assert_eq!(value["com"], "Preserved text-only comment");
            for key in [
                "tim", "filename", "ext", "fsize", "w", "h", "tn_w", "tn_h", "md5",
            ] {
                assert!(value.get(key).is_none(), "Unexpected {key}: {value}");
            }
        }
    }

    #[test]
    fn deletion_during_preview_does_not_wrap_reply_count() {
        assert!(checked_reply_count(0).is_err());
        assert_eq!(checked_reply_count(1).unwrap(), 0);
        assert_eq!(checked_reply_count(1001).unwrap(), 1000);
    }
    #[test]
    fn etag_precedes_date_and_same_second_dates_do_not_hide_changes() {
        let limits = crate::security::Limits::new(board_config::PublicRequestLimits::default());
        let modified = DateTime::from_timestamp(1_700_000_000, 500_000_000).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "if-modified-since",
            HeaderValue::from_static("Tue, 14 Nov 2023 22:13:20 GMT"),
        );
        assert_eq!(
            response(&limits, json!({"posts": [1]}), Some(modified), &headers)
                .unwrap()
                .status(),
            StatusCode::OK
        );
        headers.insert("if-none-match", HeaderValue::from_static("\"old\""));
        headers.insert(
            "if-modified-since",
            HeaderValue::from_static("Tue, 14 Nov 2023 22:14:20 GMT"),
        );
        assert_eq!(
            response(&limits, json!({"posts": [1]}), Some(modified), &headers)
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
}

#[cfg(test)]
mod preview_reference_tests {
    use super::*;
    use board_store::post_media::PostAttachment;

    fn post(template: &Post, id: i64, thread: i64, bytes: i64, deleted: bool) -> Post {
        let mut post = template.clone();
        post.id = id;
        post.thread_id = thread;
        post.comment = "Owned reference preview".into();
        post.attachment = (bytes > 0).then(|| PostAttachment {
            post_id: id,
            asset_id: format!("{id:032x}"),
            filename: "owned.png".into(),
            bytes,
            width: 10,
            height: 10,
            spoiler: false,
            file_deleted: deleted,
            tim: id,
            md5: None,
            thumbnail_width: Some(10),
            thumbnail_height: Some(10),
            output_format: board_store::media_assets::MediaFormat::Png,
            has_replay: false,
            has_mobile: false,
        });
        post
    }

    fn extras(op: &Value) -> Value {
        let keys = [
            "replies",
            "images",
            "omitted_posts",
            "omitted_images",
            "unique_ips",
        ];
        Value::Object(
            op.as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| keys.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn preview_json_matches_independent_source_fixture() {
        let reference: Value = serde_json::from_str(include_str!(
            "../../../fixtures/preview-policy-reference.json"
        ))
        .unwrap();
        for case in reference["cases"].as_array().unwrap() {
            let mut fixture = crate::native_updater_snapshot::tests::fixture();
            let input = &case["input"];
            let expected = &case["expected"];
            let id = input["op_id"].as_str().unwrap().parse::<i64>().unwrap();
            fixture.board.replies_shown = case["configured_limit"].as_i64().unwrap() as i32;
            fixture.thread.id = id;
            fixture.thread.sticky = case["sticky"].as_bool().unwrap();
            let limit = fixture.board.preview_reply_limit(fixture.thread.sticky);
            let mut replies: Vec<_> = input["replies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    post(
                        &fixture.posts[0],
                        row["id"].as_str().unwrap().parse().unwrap(),
                        id,
                        row["fsize"].as_i64().unwrap(),
                        row["file_deleted"].as_bool().unwrap(),
                    )
                })
                .collect();
            // The captured cache input is deliberately unordered. Store
            // snapshots supply selected posts in ascending post-ID order.
            replies.sort_by_key(|post| post.id);
            let images = replies
                .iter()
                .filter(|post| {
                    post.attachment
                        .as_ref()
                        .is_some_and(|file| !file.file_deleted)
                })
                .count();
            let start = replies.len().saturating_sub(limit);
            let op = post(
                &fixture.posts[0],
                id,
                id,
                input["op_fsize"].as_i64().unwrap(),
                false,
            );
            let selected = std::iter::once(op)
                .chain(replies[start..].iter().cloned())
                .collect();
            let preview = board_store::ThreadPreview {
                thread: fixture.thread.clone(),
                posts: selected,
                visible_posts: replies.len() as i64 + 1,
                visible_images: images as i64,
                unique_ips: Some(input["unique_ips"].as_i64().unwrap() as i32),
                latest_reply_id: replies.last().map(|post| post.id),
                catalog_last_reply: None,
                capcode_replies: Vec::new(),
            };
            let actual = preview_thread(&fixture.board, preview, &fixture.quote_targets).unwrap();
            let ids: Vec<_> = actual
                .iter()
                .map(|post| post["no"].as_i64().unwrap().to_string())
                .collect();
            assert_eq!(json!(ids), expected["index"]["post_ids"], "{}", case["id"]);
            assert_eq!(
                extras(&actual[0]),
                expected["index"]["op_extra"],
                "{}",
                case["id"]
            );
            let catalog = catalog_entry(&actual, fixture.thread.modified_at);
            assert_eq!(
                extras(&catalog),
                expected["catalog"]["op_extra"],
                "{}",
                case["id"]
            );
            let last = catalog.get("last_replies").map(|posts| {
                json!(
                    posts
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|post| post["no"].as_i64().unwrap().to_string())
                        .collect::<Vec<_>>()
                )
            });
            assert_eq!(
                last.as_ref(),
                expected["catalog"].get("last_reply_ids"),
                "{}",
                case["id"]
            );
        }
    }
}

#[cfg(test)]
mod directory_description_tests {
    use super::directory_data;
    #[test]
    fn imported_encoding_is_exact_and_operator_edits_stay_literal() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/json-directory-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let slug = case["board"].as_str().unwrap();
            let logical = case["logical_description"].as_str().unwrap();
            let raw = case["expected"]["meta_description"].as_str().unwrap();
            assert_eq!(directory_data::description(slug, logical), raw, "{slug}");
            let edited = format!("{logical} <owned & \"edit\">");
            assert_eq!(directory_data::description(slug, &edited), edited);
            assert_eq!(directory_data::description("owned", logical), logical);
        }
    }
}

#[cfg(test)]
mod source_post_state_tests {
    use super::*;

    #[test]
    fn full_post_state_matches_independently_executed_source() {
        let source: Value = serde_json::from_str(include_str!(
            "../../../fixtures/json-post-state-reference.json"
        ))
        .unwrap();
        for row in source["cases"].as_array().unwrap() {
            let mut fixture = crate::native_updater_snapshot::tests::fixture();
            fixture.thread.sticky = row["sticky"].as_bool().unwrap();
            fixture.thread.permaage = row["permaage"].as_bool().unwrap();
            fixture.thread.undead = row["undead"].as_bool().unwrap();
            fixture.thread.closed = row["closed"].as_bool().unwrap();
            fixture.thread.archived_at = None;
            fixture.board.bump_limit = 5;
            fixture.board.image_limit = 3;
            fixture.board.reply_limit = 99;
            fixture.board.comment_spoiler_cleanup = true;
            fixture.board.custom_spoiler_count = 4;
            let mut post = fixture.posts[0].clone();
            post.id = fixture.thread.id;
            post.subject = row["context"].as_str().unwrap().replace('-', " ");
            post.comment.clear();
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                row["replies"].as_u64().unwrap() as usize,
                row["images"].as_u64().unwrap() as usize,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            let tail = tail_op_json(
                &fixture.thread,
                &fixture.board,
                row["replies"].as_u64().unwrap() as usize,
                row["images"].as_u64().unwrap() as usize,
                &value,
            );
            let mut expected_tail = row["expected"]["tail"].clone();
            expected_tail["no"] = json!(fixture.thread.id);
            assert_eq!(tail, expected_tail, "tail: {row}");
            for key in [
                "bumplimit",
                "imagelimit",
                "sticky",
                "sticky_cap",
                "closed",
                "archived",
                "archived_on",
                "semantic_url",
                "custom_spoiler",
                "replies",
                "images",
            ] {
                assert_eq!(
                    value.get(key),
                    row["expected"]["full"].get(key),
                    "{key}: {row}"
                );
            }
        }
    }
}

#[cfg(test)]
mod source_media_state_tests {
    use super::*;
    #[test]
    fn retained_mobile_and_membership_fields_match_source_after_file_deletion() {
        let source: Value = serde_json::from_str(include_str!(
            "../../../fixtures/json-post-state-reference.json"
        ))
        .unwrap();
        for row in source["media_cases"].as_array().unwrap() {
            let media = &row["media"];
            // Current approved attachments have an extension. Unknown historical
            // records with no attachment cannot establish companion provenance.
            if media["ext"] == "" {
                continue;
            }
            let fixture = crate::native_updater_snapshot::tests::fixture();
            let mut post = fixture.posts[0].clone();
            post.file_deleted = media["filedeleted"] == 1;
            post.since4pass = media["since4pass"].as_i64().unwrap() as i32;
            post.attachment = Some(board_store::post_media::PostAttachment {
                post_id: post.id,
                asset_id: "a".repeat(32),
                filename: "sample.jpg".into(),
                bytes: 100,
                width: 640,
                height: 480,
                spoiler: false,
                file_deleted: false,
                tim: 1704067200123,
                md5: Some("EjRWeJCrze8SNFZ4kKvN7w==".into()),
                thumbnail_width: Some(250),
                thumbnail_height: Some(187),
                output_format: board_store::media_assets::MediaFormat::Jpeg,
                has_replay: false,
                has_mobile: media["m_img"] == 1,
            });
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            for key in [
                "m_img",
                "since4pass",
                "filedeleted",
                "tim",
                "filename",
                "ext",
                "md5",
                "w",
                "h",
                "tn_w",
                "tn_h",
                "fsize",
            ] {
                assert_eq!(
                    value.get(key),
                    row["expected"]["full"].get(key),
                    "{key}: {row}"
                );
            }
            for key in ["host", "pwd", "4pass_id", "email", "asset_id"] {
                assert!(value.get(key).is_none(), "{key}");
            }
        }
    }
}

#[cfg(test)]
mod source_upload_subject_tests {
    use super::*;
    #[test]
    fn upload_board_tags_and_stripped_subjects_match_source() {
        let source: Value = serde_json::from_str(include_str!(
            "../../../fixtures/json-post-state-reference.json"
        ))
        .unwrap();
        for row in source["upload_cases"].as_array().unwrap() {
            let mut fixture = crate::native_updater_snapshot::tests::fixture();
            fixture.board.upload_board = true;
            let mut post = fixture.posts[0].clone();
            post.subject = row["media"]["sub"].as_str().unwrap().to_owned();
            let value = post_json(
                post,
                &fixture.thread,
                &fixture.board,
                0,
                0,
                None,
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            for key in ["tag", "sub"] {
                assert_eq!(
                    value.get(key),
                    row["expected"]["full"].get(key),
                    "{key}: {row}"
                );
            }
        }
    }
}

#[cfg(test)]
mod source_directory_builder_tests {
    use super::*;
    #[test]
    fn directory_titles_and_numeric_descriptions_match_source_aggregation() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/json-directory-build-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            if case["authorized"] == false {
                assert!(case["expected"].is_null());
                continue;
            }
            let actual: Vec<_> = case["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    let board = row["board"].as_str().unwrap();
                    let mut value = case["metadata"][board].clone();
                    value["board"] = json!(board);
                    value["title"] = row["title"].clone();
                    finalize_directory_metadata(&mut value);
                    value
                })
                .collect();
            assert_eq!(json!({"boards":actual}), case["expected"]);
        }
    }
}

#[cfg(test)]
mod source_unique_policy_tests {
    use super::*;
    #[test]
    fn unique_counts_follow_source_enablement_and_positive_count_rules() {
        let source: Value = serde_json::from_str(include_str!(
            "../../../fixtures/preview-policy-reference.json"
        ))
        .unwrap();
        for case in source["cases"].as_array().unwrap() {
            let mut fixture = crate::native_updater_snapshot::tests::fixture();
            fixture.board.show_thread_uniques =
                case["input"]["show_thread_uniques"].as_bool().unwrap();
            let value = post_json(
                fixture.posts[0].clone(),
                &fixture.thread,
                &fixture.board,
                0,
                0,
                Some(case["input"]["unique_ips"].as_i64().unwrap() as i32),
                QuoteContext {
                    targets: &fixture.quote_targets,
                    current_thread: Some(fixture.thread.id),
                },
            )
            .unwrap();
            assert_eq!(
                value.get("unique_ips"),
                case["expected"]["full_thread"]["op_extra"].get("unique_ips"),
                "{}",
                case["id"]
            );
        }
    }
}
