use super::*;
use std::time::Duration;

#[tokio::test]
async fn cancelled_blocking_reads_keep_capacity_until_their_work_finishes() {
    let reads = Arc::new(Semaphore::new(1));
    let (started, start) = tokio::sync::oneshot::channel();
    let (release, finish) = std::sync::mpsc::channel();
    let slots = reads.clone();
    let request = tokio::spawn(blocking(slots, move || {
        started.send(()).unwrap();
        finish.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(())
    }));
    tokio::time::timeout(Duration::from_secs(2), start)
        .await
        .unwrap()
        .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert_eq!(reads.available_permits(), 0);
    assert_eq!(
        blocking(reads.clone(), || panic!(
            "work admitted while a cancelled read still runs"
        ))
        .await,
        Err::<(), _>(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while reads.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(blocking(reads, || Ok(7)).await.unwrap(), 7);
}
