use super::*;
use crate::config::Target;
#[cfg(target_os = "linux")]
use crate::config::Targets;
use serde_json::json;

fn completed() -> serde_json::Value {
    json!({"schema":1,"target":"application","started_ms":1000,"finished_ms":2000,
        "outcome":"success","last_success_ms":2000,"failure_pending":false})
}

fn target() -> Target {
    Target {
        path: "unused".into(),
        max_age_seconds: 60,
        run_timeout_seconds: 10,
    }
}

#[test]
fn completed_running_retry_failure_and_recovery_preserve_valid_meanings() {
    let good = completed();
    let result = parse(
        &serde_json::to_vec(&good).unwrap(),
        "application",
        &target(),
        2000,
    )
    .unwrap();
    assert!(result.available);
    assert!(!result.running);
    assert!(!result.failure_pending);
    assert_eq!(result.started_ms, 1000);
    assert_eq!(result.last_success_ms, 2000);
    for (outcome, finished, pending, success) in [
        ("running", json!(null), false, json!(null)),
        ("running", json!(null), true, json!(999)),
        ("failure", json!(2000), true, json!(999)),
    ] {
        let mut value = good.clone();
        value["outcome"] = json!(outcome);
        value["finished_ms"] = finished;
        value["failure_pending"] = json!(pending);
        value["last_success_ms"] = success;
        let sample = parse(
            &serde_json::to_vec(&value).unwrap(),
            "application",
            &target(),
            2000,
        )
        .unwrap();
        assert_eq!(sample.failure_pending, pending);
        assert_eq!(sample.running, outcome == "running");
    }
}

#[test]
fn rejects_missing_null_fields_duplicates_unknown_fields_future_and_inconsistent_state() {
    let good = completed();
    for key in [
        "schema",
        "target",
        "started_ms",
        "finished_ms",
        "outcome",
        "last_success_ms",
        "failure_pending",
    ] {
        let mut value = good.clone();
        value.as_object_mut().unwrap().remove(key);
        assert!(
            parse(
                &serde_json::to_vec(&value).unwrap(),
                "application",
                &target(),
                2000
            )
            .is_err()
        );
    }
    for (key, bad) in [
        ("schema", json!(2)),
        ("target", json!("host")),
        ("started_ms", json!(0)),
        ("started_ms", json!(2001)),
        ("started_ms", json!(9007199254740992_u64)),
        ("started_ms", json!(1.0)),
        ("finished_ms", json!(null)),
        ("finished_ms", json!(999)),
        ("finished_ms", json!(2001)),
        ("last_success_ms", json!(null)),
        ("last_success_ms", json!(1999)),
        ("last_success_ms", json!(2001)),
        ("failure_pending", json!(true)),
        ("extra", json!("private-secret")),
        ("outcome", json!("unknown")),
    ] {
        let mut value = good.clone();
        value[key] = bad;
        assert!(
            parse(
                &serde_json::to_vec(&value).unwrap(),
                "application",
                &target(),
                2000
            )
            .is_err(),
            "accepted {key}"
        );
    }
    let mut running = good.clone();
    running["outcome"] = json!("running");
    running["finished_ms"] = json!(null);
    // Prior success cannot be later than this attempt's start.
    assert!(
        parse(
            &serde_json::to_vec(&running).unwrap(),
            "application",
            &target(),
            2000
        )
        .is_err()
    );
    running["last_success_ms"] = json!(0);
    assert!(
        parse(
            &serde_json::to_vec(&running).unwrap(),
            "application",
            &target(),
            2000
        )
        .is_err()
    );
    let mut failure = good.clone();
    failure["outcome"] = json!("failure");
    failure["last_success_ms"] = json!(999);
    assert!(
        parse(
            &serde_json::to_vec(&failure).unwrap(),
            "application",
            &target(),
            2000
        )
        .is_err()
    );
    for (outcome, finished, pending, last_success) in [
        ("running", json!(2000), false, json!(999)),
        ("failure", json!(null), true, json!(999)),
        ("failure", json!(2000), true, json!(1500)),
        ("failure", json!(2000), true, json!(0)),
    ] {
        let mut value = good.clone();
        value["outcome"] = json!(outcome);
        value["finished_ms"] = finished;
        value["failure_pending"] = json!(pending);
        value["last_success_ms"] = last_success;
        assert!(
            parse(
                &serde_json::to_vec(&value).unwrap(),
                "application",
                &target(),
                2000
            )
            .is_err()
        );
    }
    let duplicate = serde_json::to_string(&good)
        .unwrap()
        .replacen("{", "{\"schema\":1,", 1);
    assert!(parse(duplicate.as_bytes(), "application", &target(), 2000).is_err());
    assert!(parse(&[b' '; 4097], "application", &target(), 2000).is_err());
}

proptest::proptest! {
    #[test]
    fn arbitrary_journal_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..5000)) {
        let _=parse(&bytes,"application",&target(),2000);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn actual_journals_fail_and_recover_independently_without_payload_or_path_disclosure() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let app = root.join("application.json");
    let host = root.join("host.json");
    let mut value = completed();
    value["target"] = json!("host");
    std::fs::write(&host, serde_json::to_vec(&value).unwrap()).unwrap();
    let targets = Targets {
        production: false,
        targets: [
            Some(Target {
                path: app.clone(),
                ..target()
            }),
            Some(Target {
                path: host.clone(),
                ..target()
            }),
            None,
            None,
        ],
    };
    let sample = collect_at(&targets, 2000);
    assert!(!sample.targets[0].unwrap().available);
    assert!(sample.targets[1].unwrap().available);
    std::fs::write(&app, serde_json::to_vec(&completed()).unwrap()).unwrap();
    assert!(collect_at(&targets, 2000).targets[0].unwrap().available);
    let mut padded = serde_json::to_vec(&completed()).unwrap();
    padded.resize(4096, b' ');
    std::fs::write(&app, &padded).unwrap();
    assert!(collect_at(&targets, 2000).targets[0].unwrap().available);
    padded.push(b' ');
    std::fs::write(&app, padded).unwrap();
    assert!(!collect_at(&targets, 2000).targets[0].unwrap().available);
    assert!(collect_at(&targets, 2000).targets[1].unwrap().available);
    std::fs::remove_file(&app).unwrap();
    symlink(&host, &app).unwrap();
    assert!(!collect_at(&targets, 2000).targets[0].unwrap().available);
    assert!(collect_at(&targets, 2000).targets[1].unwrap().available);
    assert!(sample.targets[2].is_none());
}
