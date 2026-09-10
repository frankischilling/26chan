use board_media::{MAX_INPUT_BYTES, MediaError, ObjectId, Quarantine};
use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, ReadBuf};

#[tokio::test]
async fn claimed_input_opens_only_exact_regular_bounded_stored_bytes() {
    use std::io::Read;
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path()).unwrap();
    let id = ObjectId::generate().unwrap();
    quarantine.receive(id, b"abc".as_slice()).await.unwrap();
    let mut input = quarantine.open_input(id, 3).unwrap();
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"abc");
    for length in [0, 2, 4, MAX_INPUT_BYTES + 1] {
        assert!(quarantine.open_input(id, length).is_err());
    }
    let path = temp.path().join(format!("{id}.input"));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(quarantine.open_input(id, 3).is_err());
    std::fs::remove_dir(&path).unwrap();
    #[cfg(unix)]
    {
        let target = temp.path().join("other");
        std::fs::write(&target, b"abc").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(quarantine.open_input(id, 3).is_err());
    }
}

#[derive(Clone, Copy)]
enum End {
    Eof,
    Fail,
    Pending,
}

struct Generated {
    remaining: u64,
    end: End,
    consumed: Arc<AtomicUsize>,
}

impl AsyncRead for Generated {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        assert!(
            buf.remaining() <= 8192,
            "intake read buffer must be fixed and bounded"
        );
        if self.remaining == 0 {
            return match self.end {
                End::Eof => Poll::Ready(Ok(())),
                End::Fail => Poll::Ready(Err(io::Error::other("stream failed"))),
                End::Pending => Poll::Pending,
            };
        }
        let count = usize::try_from(self.remaining.min(buf.remaining() as u64)).unwrap();
        buf.put_slice(&[42; 8192][..count]);
        self.remaining -= count as u64;
        self.consumed.fetch_add(count, Ordering::Relaxed);
        Poll::Ready(Ok(()))
    }
}

fn source(remaining: u64, end: End) -> Generated {
    Generated {
        remaining,
        end,
        consumed: Arc::new(AtomicUsize::new(0)),
    }
}

#[tokio::test]
async fn receives_exact_limit_from_bounded_stream() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    let id = ObjectId::generate().unwrap();
    let bytes = quarantine
        .receive(id, source(MAX_INPUT_BYTES, End::Eof))
        .await
        .unwrap();
    assert_eq!(bytes, MAX_INPUT_BYTES);
    let stored = std::fs::read(root.path().join(format!("{id}.input"))).unwrap();
    assert_eq!(stored.len() as u64, MAX_INPUT_BYTES);
    assert!(stored.iter().all(|byte| *byte == 42));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    quarantine.remove(id).unwrap();
    quarantine.remove(id).unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn rejects_one_byte_over_and_stops_consuming_at_limit_plus_one() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    for size in [MAX_INPUT_BYTES + 1, MAX_INPUT_BYTES * 100] {
        let reader = source(size, End::Eof);
        let consumed = reader.consumed.clone();
        let result = quarantine
            .receive(ObjectId::generate().unwrap(), reader)
            .await;
        assert!(matches!(result, Err(MediaError::InputTooLarge)));
        assert_eq!(consumed.load(Ordering::Relaxed) as u64, MAX_INPUT_BYTES + 1);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn empty_failed_and_cancelled_streams_remove_partial_files() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    assert!(matches!(
        quarantine
            .receive(ObjectId::generate().unwrap(), source(0, End::Eof))
            .await,
        Err(MediaError::Empty)
    ));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(matches!(
        quarantine
            .receive(ObjectId::generate().unwrap(), source(123, End::Fail))
            .await,
        Err(MediaError::Io(_))
    ));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    let reader = source(123, End::Pending);
    let consumed = reader.consumed.clone();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            quarantine.receive(ObjectId::generate().unwrap(), reader)
        )
        .await
        .is_err()
    );
    assert_eq!(consumed.load(Ordering::Relaxed), 123);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn duplicate_intake_never_overwrites_existing_input() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    let id = ObjectId::generate().unwrap();
    quarantine.receive(id, &b"original"[..]).await.unwrap();
    assert!(matches!(
        quarantine.receive(id, &b"replacement"[..]).await,
        Err(MediaError::AlreadyExists)
    ));
    assert_eq!(
        std::fs::read(root.path().join(format!("{id}.input"))).unwrap(),
        b"original"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn concurrent_duplicate_cannot_delete_active_partial() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    let id = ObjectId::generate().unwrap();
    let mut first = Box::pin(quarantine.receive(id, source(123, End::Pending)));
    assert!(
        std::future::Future::poll(
            first.as_mut(),
            &mut Context::from_waker(std::task::Waker::noop())
        )
        .is_pending()
    );
    let partial = root.path().join(format!("{id}.part"));
    assert_eq!(std::fs::metadata(&partial).unwrap().len(), 123);
    assert!(matches!(
        quarantine.receive(id, &b"other"[..]).await,
        Err(MediaError::AlreadyExists)
    ));
    assert_eq!(std::fs::metadata(&partial).unwrap().len(), 123);
    drop(first);
    assert!(!partial.exists());
}

#[test]
fn cleanup_removes_only_the_requested_object_and_partial() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    let id = ObjectId::generate().unwrap();
    std::fs::write(root.path().join(format!("{id}.part")), b"partial").unwrap();
    std::fs::write(root.path().join("operator-file"), b"keep").unwrap();
    quarantine.remove(id).unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(root.path().join("operator-file")).unwrap(),
        b"keep"
    );
}

#[test]
fn strict_ids_cannot_select_paths() {
    for value in [
        "",
        "..",
        "../outside",
        "C:\\outside",
        "/tmp/outside",
        "ABCDEF0123456789abcdef0123456789",
        "0000000000000000000000000000000g",
        "00000000000000000000000000000000.png",
        "00000000000000000000000000000000\n",
    ] {
        assert!(value.parse::<ObjectId>().is_err(), "accepted {value:?}");
    }
    let value = "0123456789abcdef0123456789abcdef";
    assert_eq!(value.parse::<ObjectId>().unwrap().to_string(), value);
    let first = ObjectId::generate().unwrap();
    assert_eq!(first.to_string().parse::<ObjectId>().unwrap(), first);
    assert_ne!(first, ObjectId::generate().unwrap());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(128))]
    #[test]
    fn arbitrary_ids_are_accepted_only_in_the_exact_alphabet(value in ".{0,80}") {
        let expected = value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        proptest::prop_assert_eq!(value.parse::<ObjectId>().is_ok(), expected);
    }
}
