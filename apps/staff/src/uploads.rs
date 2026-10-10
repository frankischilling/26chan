//! Development-only receipt UI. Receipts are bearer capabilities, not session-owned drafts.
//! No producer is spawned: cancellation drops the joined producer and HTTP client together.
//! Cleanup is best effort and bounded. Disconnects, an uncertain reservation response, or a
//! failed cleanup leave only the existing intake expiry/worker cleanup as a fallback.
use crate::{AppError, AppState, access::Level, auth, handlers, views};
use askama::Template;
use axum::{
    Form,
    body::Body,
    extract::{FromRequest, Multipart, Request, State, rejection::FormRejection},
    http::HeaderMap,
    response::{Html, Redirect},
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(crate) const MAX_MULTIPART_BYTES: usize = 8_388_608 + 16_384;
const MAX_FILE_BYTES: usize = 8_388_608;

// Deliberately no Debug: these are bearer secrets and never belong in logs or URLs.
pub(crate) struct Receipt {
    pub upload_id: String,
    pub upload_capability: String,
    pub ready: bool,
    pub message: &'static str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiptForm {
    csrf: String,
    board: String,
    thread: i64,
    upload_id: String,
    upload_capability: String,
}
fn valid_receipt(id: &str, capability: &str) -> bool {
    fn hex(value: &str, length: usize) -> bool {
        value.len() == length
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
    hex(id, 32) && hex(capability, 64)
}
fn store_error(error: board_store::StoreError) -> AppError {
    match error {
        board_store::StoreError::NotFound | board_store::StoreError::Conflict(_) => {
            AppError::NotFound
        }
        board_store::StoreError::Invalid(_) => AppError::Invalid,
        _ => AppError::Internal,
    }
}
fn intake_error(error: board_intake_client::IntakeError) -> AppError {
    match error {
        board_intake_client::IntakeError::NotFound | board_intake_client::IntakeError::Conflict => {
            AppError::NotFound
        }
        _ => AppError::Internal,
    }
}
fn cancellation_error(error: sqlx::Error) -> AppError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("P0002" | "P0001") => AppError::NotFound,
        Some("22023") => AppError::Invalid,
        _ => AppError::Internal,
    }
}
fn media(state: &AppState) -> Result<&board_intake_client::IntakeClient, AppError> {
    if state.config.production {
        return Err(AppError::Forbidden);
    }
    state.config.media.as_ref().ok_or(AppError::NotFound)
}

fn ordinary_reply_limit_reached(board: &board_store::Board, parent: &board_store::Thread) -> bool {
    parent.reply_count >= board.reply_limit
        && !(parent.sticky && parent.undead && board.reply_limit > 1)
}
fn access(session: &auth::Session, board: &str, thread: i64, csrf: &str) -> Result<(), AppError> {
    if !session.at_least(Level::Janitor) || board == "j" || !session.permissions.allows(board) {
        return Err(AppError::Forbidden);
    }
    auth::csrf(session, csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    if thread < 0 || board_domain::BoardSlug::parse(board).is_err() {
        return Err(AppError::Invalid);
    }
    Ok(())
}
async fn initial(state: &AppState, headers: &HeaderMap) -> Result<auth::Session, AppError> {
    media(state)?;
    auth::origin(headers, &state.config.origin)?;
    let mut authority = auth::guard(state, headers).await?;
    if !authority.session.at_least(Level::Janitor) {
        return Err(AppError::Forbidden);
    }
    authority.ensure_current(true).await?;
    authority.finish().await
}
async fn current(
    state: &AppState,
    headers: &HeaderMap,
    board: &str,
    thread: i64,
    csrf: &str,
) -> Result<auth::Session, AppError> {
    let mut authority = auth::guard(state, headers).await?;
    access(&authority.session, board, thread, csrf)?;
    authority.ensure_current(true).await?;
    authority.finish().await
}
/// Preliminary policy only; the post transaction remains the final authority.
async fn settings(
    state: &AppState,
    session: &auth::Session,
    board: &str,
    thread: i64,
) -> Result<(), AppError> {
    let settings = board_store::board(&state.staff, board)
        .await
        .map_err(store_error)?;
    let moderator = session.at_least(Level::Moderator);
    if settings.staff_only
        || (thread > 0 && (settings.text_only || settings.upload_board))
        || settings.image_limit == 0
    {
        return Err(AppError::Forbidden);
    }
    if thread > 0 {
        let parent = board_store::thread(&state.staff, board, thread)
            .await
            .map_err(store_error)?;
        if parent.archived_at.is_some()
            || (!moderator && (parent.closed || ordinary_reply_limit_reached(&settings, &parent)))
        {
            return Err(AppError::Forbidden);
        }
        if !moderator && !parent.sticky && !parent.undead {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts p JOIN content.visible_post_media m ON m.post_id=p.id WHERE p.board=$1 AND p.thread_id=$2 AND p.id<>$2 AND NOT p.deleted AND NOT m.file_deleted")
                .bind(board).bind(thread).fetch_one(&state.staff).await?;
            if count >= i64::from(settings.image_limit) {
                return Err(AppError::Forbidden);
            }
        }
    }
    Ok(())
}
const MAX_PREAMBLE_BYTES: usize = 16_384;

fn multipart_boundary(headers: &HeaderMap) -> Result<String, AppError> {
    if headers.get_all("content-type").iter().count() != 1 {
        return Err(AppError::Invalid);
    }
    let value = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Invalid)?;
    if value.len() > 256 {
        return Err(AppError::Invalid);
    }
    let mut parts = value.split(';');
    if !parts
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("multipart/form-data"))
    {
        return Err(AppError::Invalid);
    }
    let parameter = parts.next().ok_or(AppError::Invalid)?.trim();
    if parts.next().is_some() {
        return Err(AppError::Invalid);
    }
    let (name, value) = parameter.split_once('=').ok_or(AppError::Invalid)?;
    if !name.trim().eq_ignore_ascii_case("boundary") {
        return Err(AppError::Invalid);
    }
    let value = value.trim();
    let value = if value.starts_with('"') {
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(AppError::Invalid)?
    } else {
        value
    };
    if value.is_empty()
        || value.len() > 70
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"'()+_,-./:=?".contains(&byte))
    {
        return Err(AppError::Invalid);
    }
    Ok(value.to_owned())
}

/// Multer eagerly drains ready frames before exposing a field. Stage only the
/// control envelope so it cannot request the next file frame before authorization.
/// A frame already containing both controls and file data is retained, not copied.
async fn control_preamble(request: Request) -> Result<(Request, String, String, i64), AppError> {
    let boundary = multipart_boundary(request.headers())?;
    let marker = format!("\r\n--{boundary}\r\n").into_bytes();
    let (parts, body) = request.into_parts();
    let mut stream = body.into_data_stream();
    let mut prefix = Vec::new();
    let mut search = 0usize;
    let mut boundaries = 0usize;
    let (synthetic_end, remainder) = loop {
        let chunk = stream
            .next()
            .await
            .ok_or(AppError::Invalid)?
            .map_err(|_| AppError::Invalid)?;
        if prefix.len().saturating_add(chunk.len()) > MAX_MULTIPART_BYTES {
            return Err(AppError::Invalid);
        }
        let before = prefix.len();
        let copied = chunk.len().min(MAX_PREAMBLE_BYTES - before);
        prefix.extend_from_slice(&chunk[..copied]);
        let mut complete = None;
        while let Some(relative) = prefix[search..]
            .windows(marker.len())
            .position(|window| window == marker.as_slice())
        {
            let position = search + relative;
            search = position + marker.len();
            boundaries += 1;
            if boundaries == 3 {
                complete = Some((position, search));
                break;
            }
        }
        if let Some((position, end)) = complete {
            prefix.truncate(end);
            break (position, chunk.slice(end - before..));
        }
        if prefix.len() == MAX_PREAMBLE_BYTES {
            return Err(AppError::Invalid);
        }
        search = search.max(prefix.len().saturating_sub(marker.len() - 1));
    };
    let mut synthetic = prefix[..synthetic_end].to_vec();
    synthetic.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut staged = Request::new(Body::from(synthetic));
    *staged.headers_mut() = parts.headers.clone();
    staged.headers_mut().remove("content-length");
    let mut multipart = Multipart::from_request(staged, &())
        .await
        .map_err(|_| AppError::Invalid)?;
    let csrf = field_text(&mut multipart, "csrf", 43).await?;
    let board = field_text(&mut multipart, "board", 32).await?;
    let thread = field_text(&mut multipart, "thread", 20)
        .await?
        .parse()
        .map_err(|_| AppError::Invalid)?;
    if multipart
        .next_field()
        .await
        .map_err(|_| AppError::Invalid)?
        .is_some()
    {
        return Err(AppError::Invalid);
    }
    let replay =
        futures_util::stream::iter([Ok::<Bytes, axum::Error>(Bytes::from(prefix)), Ok(remainder)])
            .chain(stream);
    Ok((
        Request::from_parts(parts, Body::from_stream(replay)),
        csrf,
        board,
        thread,
    ))
}

/// Bound parser prefetch as well as the outgoing channel. Multer's poll_stream
/// drains every immediately-ready input chunk, so each emitted 16 KiB piece must
/// yield before the next one. Keep only the current already-delivered frame.
fn bounded_multipart_body(body: Body, count: Arc<AtomicUsize>) -> Body {
    let stream = futures_util::stream::try_unfold(
        (body.into_data_stream(), Bytes::new(), 0usize),
        move |(mut stream, mut pending, mut total)| {
            let count = count.clone();
            async move {
                tokio::task::yield_now().await;
                while pending.is_empty() {
                    let Some(chunk) = stream.next().await else {
                        return Ok::<_, std::io::Error>(None);
                    };
                    pending = chunk.map_err(|_| std::io::Error::other("Invalid multipart body"))?;
                    if pending.len() > MAX_MULTIPART_BYTES.saturating_sub(total) {
                        return Err(std::io::Error::other("Multipart body limit exceeded"));
                    }
                }
                let size = pending.len().min(16_384);
                total = total
                    .checked_add(size)
                    .filter(|total| *total <= MAX_MULTIPART_BYTES)
                    .ok_or_else(|| std::io::Error::other("Multipart body limit exceeded"))?;
                let piece = Bytes::copy_from_slice(&pending.split_to(size));
                count.store(total, Ordering::Relaxed);
                Ok(Some((piece, (stream, pending, total))))
            }
        },
    );
    Body::from_stream(stream)
}

async fn field_text(
    multipart: &mut Multipart,
    name: &str,
    limit: usize,
) -> Result<String, AppError> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|_| AppError::Invalid)?
        .ok_or(AppError::Invalid)?;
    if field.name() != Some(name) || field.file_name().is_some() {
        return Err(AppError::Invalid);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| AppError::Invalid)? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(AppError::Invalid);
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| AppError::Invalid)
}
async fn cleanup(state: &AppState, receipt: &Receipt) {
    // Revoking a bearer receipt is the only cleanup authority. No owner consumer,
    // direct intake-table access or new database pool is used here.
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        board_store::post_media::cancel_upload(
            &state.staff,
            &receipt.upload_id,
            &receipt.upload_capability,
        ),
    )
    .await;
}
async fn page(
    state: &AppState,
    headers: &HeaderMap,
    session: &auth::Session,
    board: String,
    thread: i64,
    csrf: String,
    receipt: Receipt,
) -> Result<Html<String>, AppError> {
    let view: views::Posting = handlers::posting_view(
        state,
        session,
        handlers::PostingQuery {
            board,
            thread,
            posted: None,
        },
        csrf,
        Some(receipt),
    )
    .await?;
    let html = view.render().map_err(|_| AppError::Internal)?;
    // Rendering reads current board/account options and can wait on the DB.
    // Check authority after those waits, immediately before releasing bearer HTML.
    current(
        state,
        headers,
        &view.query.board,
        view.query.thread,
        &view.csrf,
    )
    .await?;
    Ok(Html(html))
}
pub(crate) async fn upload(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<Html<String>, AppError> {
    let mut reserved = None;
    let result = tokio::time::timeout(
        Duration::from_secs(32),
        receive(&state, request, &mut reserved),
    )
    .await
    .unwrap_or(Err(AppError::Internal));
    if result.is_err()
        && let Some(receipt) = &reserved
    {
        cleanup(&state, receipt).await;
    }
    result
}
async fn receive(
    state: &Arc<AppState>,
    request: Request,
    reserved: &mut Option<Receipt>,
) -> Result<Html<String>, AppError> {
    let headers = request.headers().clone();
    let session = initial(state, &headers).await?;
    let (request, csrf, board, thread) = control_preamble(request).await?;
    access(&session, &board, thread, &csrf)?;
    let session = current(state, &headers, &board, thread, &csrf).await?;
    settings(state, &session, &board, thread).await?;
    // Count the wire representation as well as file data, so a small file
    // cannot turn the file-size allowance into unlimited multipart metadata.
    let wire_bytes = Arc::new(AtomicUsize::new(0));
    let (parts, body) = request.into_parts();
    let body = bounded_multipart_body(body, wire_bytes.clone());
    let request = Request::from_parts(parts, body);
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|_| AppError::Invalid)?;
    // Replay the complete wire prefix through the normal parser as well.
    // Authorization has already passed before this parser can request file data.
    if field_text(&mut multipart, "csrf", 43).await? != csrf
        || field_text(&mut multipart, "board", 32).await? != board
        || field_text(&mut multipart, "thread", 20)
            .await?
            .parse::<i64>()
            .map_err(|_| AppError::Invalid)?
            != thread
    {
        return Err(AppError::Invalid);
    }
    let client = media(state)?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let (reservation_sender, reservation_receiver) =
        tokio::sync::oneshot::channel::<(String, String)>();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let send = async {
        let (id, capability) = reservation_receiver.await.map_err(|_| AppError::Internal)?;
        client
            .upload(&id, &capability, Body::from_stream(stream))
            .await
            .map_err(|_| AppError::Internal)
    };
    let pump = async {
        // Own the multipart field inside this future so its borrow ends before
        // the trailing-field check. Only the receipt crosses the one-shot gate;
        // neither future is spawned and both are cancelled together on failure.
        let mut field = multipart
            .next_field()
            .await
            .map_err(|_| AppError::Invalid)?
            .ok_or(AppError::Invalid)?;
        if field.name() != Some("upfile") {
            return Err(AppError::Invalid);
        }
        let filename = field
            .file_name()
            .filter(|name| {
                !name.is_empty() && name.len() <= 255 && !name.chars().any(char::is_control)
            })
            .ok_or(AppError::Invalid)?
            .to_owned();
        let reservation = tokio::time::timeout(Duration::from_secs(4), client.reserve(&filename))
            .await
            .map_err(|_| AppError::Internal)?
            .map_err(|_| AppError::Internal)?;
        *reserved = Some(Receipt {
            upload_id: reservation.id.clone(),
            upload_capability: reservation.capability.clone(),
            ready: false,
            message: "Your file is queued for isolated processing. Check its status before posting.",
        });
        reservation_sender
            .send((reservation.id, reservation.capability))
            .map_err(|_| AppError::Internal)?;
        let mut total = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|_| AppError::Invalid)? {
            total = total.checked_add(chunk.len()).ok_or(AppError::Invalid)?;
            if total > MAX_FILE_BYTES {
                return Err(AppError::Invalid);
            }
            for piece in chunk.chunks(16_384) {
                sender
                    .send(Ok(Bytes::copy_from_slice(piece)))
                    .await
                    .map_err(|_| AppError::Internal)?;
            }
        }
        drop(field);
        if total == 0 {
            return Err(AppError::Invalid);
        }
        // Verify exactly one file before signalling successful end-of-stream.
        if multipart
            .next_field()
            .await
            .map_err(|_| AppError::Invalid)?
            .is_some()
        {
            return Err(AppError::Invalid);
        }
        if wire_bytes.load(Ordering::Relaxed).saturating_sub(total) > 16_384 {
            return Err(AppError::Invalid);
        }
        drop(sender);
        Ok(())
    };
    tokio::try_join!(send, pump)?;
    let session = current(state, &headers, &board, thread, &csrf).await?;
    settings(state, &session, &board, thread).await?;
    // Retain a cleanup receipt until rendering succeeds, including query failures.
    let receipt = reserved.as_ref().ok_or(AppError::Internal)?;
    page(
        state,
        &headers,
        &session,
        board,
        thread,
        csrf,
        Receipt {
            upload_id: receipt.upload_id.clone(),
            upload_capability: receipt.upload_capability.clone(),
            ready: false,
            message: receipt.message,
        },
    )
    .await
}
fn parsed(form: Result<Form<ReceiptForm>, FormRejection>) -> Result<ReceiptForm, AppError> {
    let Form(form) = form.map_err(|_| AppError::Invalid)?;
    if !valid_receipt(&form.upload_id, &form.upload_capability) {
        return Err(AppError::Invalid);
    }
    Ok(form)
}
pub(crate) async fn status(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<Html<String>, AppError> {
    let headers = request.headers().clone();
    initial(&state, &headers).await?;
    let form = parsed(Form::<ReceiptForm>::from_request(request, &state).await)?;
    let session = current(&state, &headers, &form.board, form.thread, &form.csrf).await?;
    settings(&state, &session, &form.board, form.thread).await?;
    board_store::post_media::check_upload(&state.staff, &form.upload_id, &form.upload_capability)
        .await
        .map_err(store_error)?;
    let status = media(&state)?
        .status(&form.upload_id, &form.upload_capability)
        .await
        .map_err(intake_error)?;
    let ready = status.state == "published" && status.output_id.is_some();
    let message = if ready {
        "Your file is approved. Complete your post below."
    } else if status.state == "failed" {
        "Processing failed. Cancel this upload and choose another file."
    } else {
        "Your file is not ready. Check again shortly or cancel it."
    };
    let session = current(&state, &headers, &form.board, form.thread, &form.csrf).await?;
    page(
        &state,
        &headers,
        &session,
        form.board,
        form.thread,
        form.csrf,
        Receipt {
            upload_id: form.upload_id,
            upload_capability: form.upload_capability,
            ready,
            message,
        },
    )
    .await
}
pub(crate) async fn cancel(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<Redirect, AppError> {
    let headers = request.headers().clone();
    initial(&state, &headers).await?;
    let form = parsed(Form::<ReceiptForm>::from_request(request, &state).await)?;
    current(&state, &headers, &form.board, form.thread, &form.csrf).await?;
    // Match attachment posting's media -> account lock order. Cancellation is
    // tentative until the fresh authority check succeeds; any failure rolls the
    // staff transaction back. Use the same connection so we never wait on our
    // own job lock. No network exchange occurs while auth locks are held.
    let mut transaction = state.staff.begin().await.map_err(|_| AppError::Internal)?;
    sqlx::query("SELECT content.cancel_attachment_upload($1,$2)")
        .bind(&form.upload_id)
        .bind(&form.upload_capability)
        .execute(&mut *transaction)
        .await
        .map_err(cancellation_error)?;
    let mut authority = auth::guard(&state, &headers).await?;
    access(&authority.session, &form.board, form.thread, &form.csrf)?;
    authority.ensure_current(true).await?;
    // Cancellation remains possible when a thread closes or fills after upload.
    transaction.commit().await.map_err(|_| AppError::Internal)?;
    authority.finish().await?;
    Ok(Redirect::to(&format!(
        "/post?board={}&thread={}",
        form.board, form.thread
    )))
}
pub(crate) async fn attachment(
    state: &AppState,
    session: &auth::Session,
    input: &handlers::StaffMessage,
) -> Result<Option<board_store::post_media::NewAttachment>, AppError> {
    match (&input.upload_id, &input.upload_capability) {
        (None, None) if !input.spoiler => Ok(None),
        (Some(id), Some(capability)) if valid_receipt(id, capability) => {
            media(state)?;
            settings(state, session, &input.board, input.thread).await?;
            Ok(Some(board_store::post_media::NewAttachment {
                upload: board_store::media_intake::IntakeReservation {
                    id: id.clone(),
                    capability: capability.clone(),
                },
                spoiler: input.spoiler,
            }))
        }
        _ => Err(AppError::Invalid),
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::task::Poll;

    #[tokio::test]
    async fn parser_adapter_emits_bounded_pieces_with_pending_between_them() {
        let count = Arc::new(AtomicUsize::new(0));
        let body = bounded_multipart_body(Body::from(vec![7u8; 49_153]), count.clone());
        let mut stream = body.into_data_stream();
        for size in [16_384, 16_384, 16_384, 1] {
            assert!(matches!(futures_util::poll!(stream.next()), Poll::Pending));
            let piece = stream.next().await.unwrap().unwrap();
            assert_eq!(piece.len(), size);
            assert!(piece.iter().all(|byte| *byte == 7));
        }
        assert!(stream.next().await.is_none());
        assert_eq!(count.load(Ordering::Relaxed), 49_153);
    }

    #[tokio::test]
    async fn parser_adapter_rejects_excess_before_emitting_it() {
        let count = Arc::new(AtomicUsize::new(0));
        let bytes = Bytes::from(vec![3u8; 16_384]);
        let source = futures_util::stream::iter(
            (0..514).map(move |_| Ok::<_, std::io::Error>(bytes.clone())),
        );
        let mut stream =
            bounded_multipart_body(Body::from_stream(source), count.clone()).into_data_stream();
        let mut emitted = 0usize;
        while let Some(piece) = stream.next().await {
            match piece {
                Ok(piece) => {
                    assert!(piece.len() <= 16_384);
                    emitted += piece.len();
                }
                Err(_) => {
                    assert_eq!(emitted, MAX_MULTIPART_BYTES);
                    assert_eq!(count.load(Ordering::Relaxed), MAX_MULTIPART_BYTES);
                    return;
                }
            }
        }
        panic!("excess multipart frame was accepted");
    }

    #[tokio::test]
    async fn multipart_returns_file_data_before_draining_ready_source_frames() {
        let consumed = Arc::new(AtomicUsize::new(0));
        let observed = consumed.clone();
        let data = Bytes::from(vec![b'x'; 16_384]);
        let source = futures_util::stream::iter((0..66).map(move |index| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok::<_, std::io::Error>(match index {
                0 => Bytes::from_static(b"--unit\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"owned.png\"\r\n\r\n"),
                65 => Bytes::from_static(b"\r\n--unit--\r\n"),
                _ => data.clone(),
            })
        }));
        let body = bounded_multipart_body(Body::from_stream(source), Arc::new(AtomicUsize::new(0)));
        let request = Request::post("/post/upload")
            .header("content-type", "multipart/form-data; boundary=unit")
            .body(body)
            .unwrap();
        let mut multipart = Multipart::from_request(request, &()).await.unwrap();
        let mut field = multipart.next_field().await.unwrap().unwrap();
        let piece = field.chunk().await.unwrap().unwrap();
        assert!(!piece.is_empty() && piece.iter().all(|byte| *byte == b'x'));
        let before = consumed.load(Ordering::Relaxed);
        assert!(
            before < 8,
            "multipart greedily drained the ready file stream"
        );
        tokio::task::yield_now().await;
        assert_eq!(
            consumed.load(Ordering::Relaxed),
            before,
            "unpolled producer made background progress"
        );
    }

    fn preamble(padding: usize) -> String {
        format!(
            "--unit\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{}\r\n--unit\r\nContent-Disposition: form-data; name=\"board\"\r\n\r\ntest\r\n--unit\r\nContent-Disposition: form-data; name=\"thread\"\r\nX-Pad: {}\r\n\r\n0\r\n--unit\r\n",
            "a".repeat(43),
            "p".repeat(padding)
        )
    }
    fn multipart_request(bytes: Vec<u8>) -> Request {
        Request::post("/post/upload")
            .header("content-type", "multipart/form-data; boundary=unit")
            .body(Body::from(bytes))
            .unwrap()
    }

    #[tokio::test]
    async fn exact_control_bound_replays_coalesced_bytes_once() {
        let padding = MAX_PREAMBLE_BYTES - preamble(0).len();
        let prefix = preamble(padding);
        assert_eq!(prefix.len(), MAX_PREAMBLE_BYTES);
        let original = format!("{prefix}Content-Disposition: form-data; name=\"upfile\"; filename=\"owned.png\"\r\n\r\nowned\r\n--unit--\r\n").into_bytes();
        let (replay, csrf, board, thread) = control_preamble(multipart_request(original.clone()))
            .await
            .unwrap();
        assert_eq!((csrf, board, thread), ("a".repeat(43), "test".into(), 0));
        assert_eq!(
            axum::body::to_bytes(replay.into_body(), MAX_MULTIPART_BYTES)
                .await
                .unwrap()
                .as_ref(),
            original
        );
        assert!(
            control_preamble(multipart_request(preamble(padding + 1).into_bytes()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn oversized_first_frame_is_rejected_before_parser_replay() {
        let mut bytes = preamble(0).into_bytes();
        bytes.resize(MAX_MULTIPART_BYTES + 1, b'x');
        assert!(control_preamble(multipart_request(bytes)).await.is_err());
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod sticky_retention_database_tests {
    use super::ordinary_reply_limit_reached;
    use sqlx::PgPool;

    #[tokio::test]
    async fn staff_preflight_uses_current_persisted_sticky_undead_and_reply_capacity() {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let slug: String =
            sqlx::query_scalar("SELECT 'sr'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Staff sticky preflight','Owned synthetic',2000,3,3,100,10)")
            .bind(&slug).execute(&owner).await.unwrap();
        let op: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,sticky,undead,reply_count) VALUES($1,true,true,3) RETURNING id")
            .bind(&slug).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned sticky preflight','Fixture OP')")
            .bind(op).bind(&slug).execute(&owner).await.unwrap();
        let settings = board_store::board(&staff, &slug).await.unwrap();
        let parent = board_store::thread(&staff, &slug, op).await.unwrap();
        assert!(!ordinary_reply_limit_reached(&settings, &parent));
        for (sticky, undead, at_limit) in [
            (true, false, true),
            (false, true, true),
            (false, false, true),
            (true, true, false),
        ] {
            sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3 WHERE id=$1")
                .bind(op)
                .bind(sticky)
                .bind(undead)
                .execute(&owner)
                .await
                .unwrap();
            let refreshed = board_store::thread(&staff, &slug, op).await.unwrap();
            assert_eq!(
                ordinary_reply_limit_reached(&settings, &refreshed),
                at_limit
            );
        }
        // STICKY_CAP <= 1 disables the special source prune. No ordinary
        // staff reply may bypass the regular cap in that configuration.
        sqlx::query("UPDATE content.boards SET reply_limit=1,bump_limit=1 WHERE slug=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        let settings = board_store::board(&staff, &slug).await.unwrap();
        let parent = board_store::thread(&staff, &slug, op).await.unwrap();
        assert!(ordinary_reply_limit_reached(&settings, &parent));
        let old_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&slug)
                .fetch_one(&owner)
                .await
                .unwrap();
        assert_eq!(old_count, 1, "preflight must never mutate posts");
        sqlx::query("DELETE FROM content.posts WHERE board=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        staff.close().await;
        owner.close().await;
    }
}
