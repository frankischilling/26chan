#![cfg(all(feature = "database-tests", target_os = "linux"))]

//! Explicit, test-only SQL coordinator qualification against the owned mTLS
//! gateway, root broker and real Firecracker guest. No fake result producer,
//! guest-library linkage, approval, encoder or public route is used here.
//!
//! The Python qualification harness starts the services and invokes this native
//! test binary as the nonroot coordinator with:
//! `--ignored --exact real_guest_candidates_remain_nonpublishable --nocapture`.
//! An idle disposable database and MEDIA_PAIRED_VM_QUALIFY=1 are required.

use board_media::{
    ObjectId, Quarantine,
    paired::{self, InputKind},
    source_digest::{PngSourceDigestLimits, png_source_processed_digest},
};
use board_media_admin::paired::dispatch_candidate;
use board_media_dispatch::{ClientSettings, DispatchClient};
use board_store::{
    media::MediaQueue,
    media_assets::SourceProfile,
    media_intake::{IntakeStore, PairedInputDescriptor},
};
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const EMPTY_REPLAY: &[u8] = include_bytes!("../../media-guest/tests/fixtures/replay/empty.tgkr");
const COMMANDS_REPLAY: &[u8] =
    include_bytes!("../../media-guest/tests/fixtures/replay/commands.tgkr");
const EMPTY_WIRE: &[u8] = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr");
const COMMANDS_WIRE: &[u8] =
    include_bytes!("../../../tests/media/fixtures/replay-wire/commands.ibr");

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}

fn digest(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_png() -> Vec<u8> {
    // Independent frozen red pixel, removable text and a post-IEND trailer.
    // The source SHA-256 and retained-byte MD5 below are golden constants,
    // rather than values derived through the implementation under test.
    let red = hex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c48900000010494441547801010500faff00ff0000ff050001fffa5c88d10000000049454e44ae426082",
    );
    [
        red[..33].to_vec(),
        hex("00000003744558746e0076cdcf317b"),
        red[33..].to_vec(),
        b"ignored trailer".to_vec(),
    ]
    .concat()
}

fn required_path(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("qualification path is required"));
    assert!(path.is_absolute(), "{name} must be absolute");
    path
}

async fn assert_idle(admin: &sqlx::PgPool) {
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing')",
    )
    .fetch_one(admin)
    .await
    .unwrap();
    assert_eq!(pending, 0, "use an idle owned disposable queue");
}

#[tokio::test]
#[ignore = "requires explicit owned root harness, real Firecracker and an idle disposable database"]
async fn real_guest_candidates_remain_nonpublishable() {
    assert_eq!(
        std::env::var("APP_ENV").as_deref(),
        Ok("development"),
        "explicit development qualification mode is required"
    );
    assert_eq!(
        std::env::var("MEDIA_PAIRED_VM_QUALIFY").as_deref(),
        Ok("1"),
        "explicit paired VM qualification opt-in is required"
    );
    // The surrounding harness owns root-only provisioning; the coordinator
    // must not acquire that authority. Avoid a new production dependency just
    // to inspect the Linux test process's real/effective/saved/filesystem UIDs.
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let uids: Vec<u32> = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .expect("Linux process UID record")
        .split_ascii_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    assert_eq!(uids.len(), 4);
    assert!(
        !uids.contains(&0),
        "run the test as the nonroot coordinator"
    );

    let client = DispatchClient::new(
        &ClientSettings::read(&required_path("MEDIA_PAIRED_CLIENT_CONFIG")).unwrap(),
    )
    .unwrap();
    let quarantine = Arc::new(Quarantine::new(required_path("MEDIA_QUARANTINE_DIR")).unwrap());
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert_idle(&admin).await;

    // Keep exact owned IDs outside the assertion task so an ordinary test
    // failure can still clean its SQL rows and private quarantine files.
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let task_ids = ids.clone();
    let task_admin = admin.clone();
    let task_quarantine = quarantine.clone();
    let result = tokio::spawn(async move {
        exercise(
            &intake,
            &queue,
            &task_admin,
            &task_quarantine,
            &client,
            &task_ids,
        )
        .await;
        intake.close().await.unwrap();
    })
    .await;

    let ids = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    for id in &ids {
        quarantine.remove(id.parse().unwrap()).unwrap();
    }
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(remaining, 0, "owned qualification rows were not removed");
    admin.close().await;
    result.unwrap();
    println!(
        "PASS paired SQL coordinator real-guest candidates remain nonpublishable; owned rows cleaned"
    );
}

async fn exercise(
    intake: &IntakeStore,
    queue: &MediaQueue,
    admin: &sqlx::PgPool,
    quarantine: &Quarantine,
    client: &DispatchClient,
    ids: &Mutex<Vec<String>>,
) {
    let mut bindings = Vec::new();
    for (case, replay, expected_wire, succeeds) in [
        ("png-only", None, None, true),
        ("empty-replay", Some(EMPTY_REPLAY), Some(EMPTY_WIRE), true),
        (
            "commands-replay",
            Some(COMMANDS_REPLAY),
            Some(COMMANDS_WIRE),
            true,
        ),
        ("invalid-png-crc", Some(EMPTY_REPLAY), None, false),
        (
            "invalid-tgkr",
            Some(b"invalid TGKR".as_slice()),
            None,
            false,
        ),
    ] {
        assert_idle(admin).await;
        let reservation = intake
            .reserve_pair("paired-vm-qualification.png")
            .await
            .unwrap();
        ids.lock().unwrap().push(reservation.id.clone());
        // Public opaque job IDs, never capabilities or connection strings. The
        // harness can capture these for cleanup if it must kill the process.
        println!("\nPAIRED_VM_OWNED_JOB={}", reservation.id);
        std::io::stdout().flush().unwrap();
        let id: ObjectId = reservation.id.parse().unwrap();
        let mut png = source_png();
        if case == "invalid-png-crc" {
            png[29] ^= 1;
        }
        // CRC is deliberately outside this host framing scanner's remit. All
        // cases reach the real dispatch path rather than being rejected here.
        let cap = paired::MAX_PNG_INPUT_BYTES as usize;
        assert!(
            png_source_processed_digest(&png, PngSourceDigestLimits::new(cap, cap, cap).unwrap())
                .is_ok(),
            "case {case} must pass host source framing"
        );
        let input = paired::encode_input(InputKind::PairedV2, id.bytes(), &png, replay).unwrap();
        intake
            .begin_pair_upload(&reservation.id, &reservation.capability)
            .await
            .unwrap();
        let receipt = quarantine.receive_pair(id, input.as_slice()).await.unwrap();
        let descriptor = PairedInputDescriptor {
            bytes: receipt.bytes,
            sha256: digest(&receipt.sha256),
            image_bytes: receipt.image_bytes,
            image_sha256: digest(&receipt.image_sha256),
            replay_bytes: receipt.replay_bytes,
            replay_sha256: receipt.replay_sha256.as_ref().map(|value| digest(value)),
        };
        intake
            .finish_pair_upload(&reservation.id, &reservation.capability, &descriptor)
            .await
            .unwrap();
        let queued = queue.get(&reservation.id).await.unwrap();
        assert_eq!(queued.state, "queued", "case {case}");
        assert_eq!(queued.input_kind, "paired-v2");
        assert_eq!(queued.attempts, 0);
        assert!(queued.lease_token.is_none());

        let result = dispatch_candidate(queue, quarantine, client).await;
        assert_eq!(result.is_ok(), succeeds, "case {case}");
        if let Ok(checked) = result {
            assert_eq!(checked.job_id(), id, "case {case}");
            assert_eq!(checked.bundle_sha256(), &receipt.sha256);
            let source = checked.png_source();
            assert_eq!(
                source.input_sha256,
                "f482035298dcdf31dca1ce576adea047626ddb9deff6c5bfce3cf1ea68b15357"
            );
            assert_eq!(source.input_sha256, descriptor.image_sha256);
            assert_ne!(source.input_sha256, descriptor.sha256);
            assert_eq!(source.input_bytes, 103);
            assert_eq!(source.profile, SourceProfile::PngV1);
            assert_eq!(source.retained_bytes, 73);
            assert_eq!(
                source.md5.as_slice(),
                hex("b4e7464f29bcc44451c570504d61030b")
            );

            let candidate = checked.candidate().unwrap();
            assert_eq!(candidate.dimensions(), (1, 1));
            assert_eq!(candidate.rgba_bytes(), &[255, 0, 0, 255]);
            assert_eq!(candidate.replay_wire_bytes(), expected_wire, "case {case}");
            assert_eq!(candidate.untrusted_replay().is_some(), replay.is_some());
            if let Some(replay) = candidate.untrusted_replay() {
                assert_eq!((replay.metadata.width, replay.metadata.height), (640, 480));
            }
            assert_ne!(candidate.binding(), checked.bundle_sha256());
            assert_ne!(candidate.binding(), &receipt.image_sha256);
            assert!(
                !bindings.contains(candidate.binding()),
                "fresh attempt binding"
            );
            bindings.push(*candidate.binding());
        }

        let finished = queue.get(&reservation.id).await.unwrap();
        assert_eq!(finished.state, "failed", "case {case} must not publish");
        assert_eq!(finished.input_kind, "paired-v2");
        assert_eq!(finished.attempts, 1);
        assert_eq!(
            finished.failure.as_deref(),
            Some(if succeeds {
                "candidate_checked"
            } else {
                "processing_failed"
            }),
            "case {case}"
        );
        assert!(finished.lease_token.is_none());
        assert!(finished.expires_at.is_none());
        assert!(finished.output_sha256.is_none());
        assert!(finished.output_bytes.is_none());
        assert_eq!(
            finished.input_sha256.as_deref(),
            Some(descriptor.sha256.as_str())
        );
        assert_eq!(
            finished.input_image_sha256.as_deref(),
            Some(descriptor.image_sha256.as_str())
        );
        assert_eq!(finished.input_replay_sha256, descriptor.replay_sha256);
        let status = intake
            .status(&reservation.id, &reservation.capability)
            .await
            .unwrap();
        assert_eq!(status.state, "failed");
        assert!(status.output_id.is_none());
        let assets: i64 = sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=$1")
            .bind(&reservation.id)
            .fetch_one(admin)
            .await
            .unwrap();
        assert_eq!(assets, 0, "case {case} must create no assets");
        assert_eq!(quarantine.inspect_pair(id).await.unwrap(), receipt);
        assert_idle(admin).await;
        println!("PASS paired coordinator {case}: failed, no output metadata or assets");
    }
}
