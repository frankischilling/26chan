use std::{
    future::pending,
    io::Write as _,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

use axum::{Router, body::Body, extract::Request, middleware, response::Response};
use http_body::Body as HttpBody;
use http_body_util::BodyExt;
use tokio::sync::Semaphore;
use tower::ServiceExt;

use super::*;
use crate::{hold_permit, retain_response_body};

#[test]
fn capacity_is_validated_and_writers_are_lazy() {
    assert!(matches!(
        ResponseBudget::new(BLOCK_BYTES - 1),
        Err(OutputError::InvalidCapacity)
    ));
    let maximum = maximum_capacity_bytes();
    assert!(ResponseBudget::new(maximum).is_ok());
    if let Some(invalid) = maximum.checked_add(1) {
        assert!(matches!(
            ResponseBudget::new(invalid),
            Err(OutputError::InvalidCapacity)
        ));
    }

    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let writer = budget.writer(usize::MAX);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    drop(writer);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
}

#[test]
fn capacity_uses_whole_blocks_and_accepts_the_portable_one_gibibyte_limit() {
    let budget = ResponseBudget::new(BLOCK_BYTES + 1).unwrap();
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);

    let one_gibibyte = 1_073_741_824_usize;
    let budget = ResponseBudget::new(one_gibibyte).unwrap();
    assert_eq!(budget.available_bytes(), one_gibibyte);
}

#[test]
fn aggregate_admission_happens_before_any_block_of_a_failed_write() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let mut writer = budget.writer(BLOCK_BYTES * 2);
    let error = writer.write_all(&vec![b'x'; BLOCK_BYTES + 1]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("aggregate response buffer budget")
    );
    assert!(writer.blocks.is_empty());
    assert_eq!(writer.len, 0);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    assert!(matches!(writer.finish(), Err(OutputError::BudgetExhausted)));
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
}

#[test]
fn writers_share_the_budget_and_release_blocks_on_drop() {
    let budget = ResponseBudget::new(BLOCK_BYTES * 2).unwrap();
    let mut first = budget.writer(BLOCK_BYTES * 2);
    let mut second = budget.writer(BLOCK_BYTES * 2);

    first.write_all(b"a").unwrap();
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    second.write_all(b"b").unwrap();
    assert_eq!(budget.available_bytes(), 0);
    assert!(second.write_all(&vec![b'c'; BLOCK_BYTES]).is_err());
    assert_eq!(budget.available_bytes(), 0);

    drop(first);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    drop(second);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES * 2);
}

#[test]
fn per_output_limit_and_writer_errors_are_sticky() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let mut writer = budget.writer(3);
    std::fmt::Write::write_str(&mut writer, "abc").unwrap();
    assert_eq!(budget.available_bytes(), 0);
    assert!(std::fmt::Write::write_str(&mut writer, "d").is_err());
    assert!(writer.write_all(b"").is_err());
    assert!(writer.flush().is_err());
    assert!(matches!(
        writer.finish(),
        Err(OutputError::OutputLimitExceeded)
    ));
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
}

#[test]
fn finish_moves_blocks_without_changing_the_charge_or_chunking() {
    let budget = ResponseBudget::new(BLOCK_BYTES * 3).unwrap();
    let payload = vec![b'z'; BLOCK_BYTES * 2 + 17];
    let mut writer = budget.writer(payload.len());
    writer.write_all(&payload).unwrap();
    assert_eq!(budget.available_bytes(), 0);

    let encoded = writer.finish().unwrap();
    assert_eq!(encoded.len(), payload.len());
    assert!(!encoded.is_empty());
    let chunks: Vec<&[u8]> = encoded.chunks().collect();
    assert_eq!(
        chunks.iter().map(|chunk| chunk.len()).collect::<Vec<_>>(),
        [BLOCK_BYTES, BLOCK_BYTES, 17]
    );
    assert!(
        chunks
            .iter()
            .flat_map(|chunk| chunk.iter())
            .copied()
            .eq(payload.iter().copied())
    );
    assert_eq!(budget.available_bytes(), 0);
    drop(encoded);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES * 3);
}

#[tokio::test]
async fn body_has_exact_size_and_emitted_slices_retain_only_their_blocks() {
    let budget = ResponseBudget::new(BLOCK_BYTES * 2).unwrap();
    let mut writer = budget.writer(BLOCK_BYTES + 11);
    writer.write_all(&vec![b'q'; BLOCK_BYTES + 11]).unwrap();
    let encoded = writer.finish().unwrap();
    let first_pointer = encoded.chunks().next().unwrap().as_ptr();
    let mut body = encoded.into_body();

    assert_eq!(body.size_hint().exact(), Some((BLOCK_BYTES + 11) as u64));
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(first.len(), BLOCK_BYTES);
    assert_eq!(first.as_ptr(), first_pointer, "payload block was copied");
    assert_eq!(body.size_hint().exact(), Some(11));
    let slice = first.slice(1..2);
    drop(first);
    assert_eq!(budget.available_bytes(), 0);

    let last = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(last.len(), 11);
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
    drop(body);
    drop(last);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    drop(slice);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES * 2);
}

#[tokio::test]
async fn request_permit_and_response_block_survive_the_same_emitted_slice() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let mut writer = budget.writer(7);
    writer.write_all(b"payload").unwrap();
    let encoded = writer.finish().unwrap();

    let admission = Arc::new(Semaphore::new(1));
    let permit = admission.clone().try_acquire_owned().unwrap();
    let response = hold_permit(Response::new(encoded.into_body()), permit);
    let response = Arc::new(Mutex::new(Some(response)));
    let app = Router::new()
        .fallback({
            let response = Arc::clone(&response);
            move || {
                let response = response.lock().unwrap().take().unwrap();
                async move { response }
            }
        })
        .layer(middleware::from_fn(retain_response_body));

    let mut body = app
        .oneshot(Request::new(Body::empty()))
        .await
        .unwrap()
        .into_body();
    let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = data.slice(1..2);
    drop(body);
    drop(data);
    assert_eq!(budget.available_bytes(), 0);
    assert_eq!(admission.available_permits(), 0);
    drop(slice);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    assert_eq!(admission.available_permits(), 1);
}

#[test]
fn dropping_an_unpolled_body_or_replacing_it_with_empty_releases_blocks() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let encode = || {
        let mut writer = budget.writer(7);
        writer.write_all(b"payload").unwrap();
        writer.finish().unwrap()
    };

    let body = encode().into_body();
    assert_eq!(budget.available_bytes(), 0);
    drop(body);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);

    let mut response = Response::new(encode().into_body());
    assert_eq!(budget.available_bytes(), 0);
    *response.body_mut() = Body::empty();
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    drop(response);
}

#[tokio::test]
async fn cancelling_a_future_drops_its_unpolled_encoded_body() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let mut writer = budget.writer(7);
    writer.write_all(b"payload").unwrap();
    let body = writer.finish().unwrap().into_body();
    assert_eq!(budget.available_bytes(), 0);

    let task = tokio::spawn(async move {
        let _body = body;
        pending::<()>().await;
    });
    tokio::task::yield_now().await;
    assert_eq!(budget.available_bytes(), 0);
    task.abort();
    let _ = task.await;
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
}

#[test]
fn unwinding_drops_charged_blocks() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let worker_budget = budget.clone();
    let result = catch_unwind(AssertUnwindSafe(move || {
        let mut writer = worker_budget.writer(7);
        writer.write_all(b"payload").unwrap();
        assert_eq!(worker_budget.available_bytes(), 0);
        panic!("owned test panic");
    }));
    assert!(result.is_err());
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
}

#[test]
fn empty_output_needs_no_block() {
    let budget = ResponseBudget::new(BLOCK_BYTES).unwrap();
    let encoded = budget.writer(0).finish().unwrap();
    assert!(encoded.is_empty());
    assert_eq!(encoded.len(), 0);
    assert_eq!(encoded.chunks().count(), 0);
    assert_eq!(budget.available_bytes(), BLOCK_BYTES);
    let body = encoded.into_body();
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
}

#[tokio::test]
async fn utf8_limits_count_bytes_and_block_boundaries_preserve_the_complete_stream() {
    let budget = ResponseBudget::new(BLOCK_BYTES * 2).unwrap();
    let mut too_small = budget.writer(3);
    assert!(std::fmt::Write::write_str(&mut too_small, "🙂").is_err());
    assert_eq!(budget.available_bytes(), BLOCK_BYTES * 2);
    assert!(matches!(
        too_small.finish(),
        Err(OutputError::OutputLimitExceeded)
    ));

    let mut exact = budget.writer(4);
    std::fmt::Write::write_str(&mut exact, "🙂").unwrap();
    let encoded = exact.finish().unwrap();
    assert_eq!(encoded.len(), 4);
    assert_eq!(encoded.chunks().next().unwrap(), "🙂".as_bytes());
    drop(encoded);

    let prefix = "x".repeat(BLOCK_BYTES - 2);
    let mut writer = budget.writer(BLOCK_BYTES + 2);
    std::fmt::Write::write_str(&mut writer, &prefix).unwrap();
    std::fmt::Write::write_str(&mut writer, "🙂").unwrap();
    let mut body = writer.finish().unwrap().into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(first.len(), BLOCK_BYTES);
    assert_eq!(&first[BLOCK_BYTES - 2..], &"🙂".as_bytes()[..2]);
    assert_eq!(&second[..], &"🙂".as_bytes()[2..]);
    let mut complete = first.to_vec();
    complete.extend_from_slice(&second);
    assert_eq!(String::from_utf8(complete).unwrap(), format!("{prefix}🙂"));
    drop((first, second, body));
    assert_eq!(budget.available_bytes(), BLOCK_BYTES * 2);
}
