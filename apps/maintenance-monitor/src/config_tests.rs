use super::*;
use serde_json::json;

fn document(path: &Path) -> Vec<u8> {
    serde_json::to_vec(&json!({"targets":[{"target":"application", "path":path,
        "max_age_seconds":604800,"run_timeout_seconds":1800}]}))
    .unwrap()
}

#[test]
fn missing_journals_remain_configured_and_have_fixed_independent_slots() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let bytes = serde_json::to_vec(&json!({"targets":[
        {"target":"monitoring","path":root.join("absent/monitoring.json"),"max_age_seconds":1,"run_timeout_seconds":1},
        {"target":"application","path":root.join("application.json"),"max_age_seconds":2592000,"run_timeout_seconds":3600}
    ]})).unwrap();
    let targets = parse_config(&bytes, true).unwrap();
    assert!(targets.production);
    assert_eq!(
        targets.targets[0].as_ref().unwrap().max_age_seconds,
        2592000
    );
    assert!(targets.targets[1].is_none());
    assert!(targets.targets[2].is_none());
    assert_eq!(targets.targets[3].as_ref().unwrap().run_timeout_seconds, 1);
}

#[test]
fn config_rejects_unknown_duplicate_fields_targets_bad_limits_and_path_spellings() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&document(&root.join("application.json"))).unwrap();
    for replacement in [json!([]), json!([value["targets"][0], value["targets"][0]])] {
        assert!(
            parse_config(
                &serde_json::to_vec(&json!({"targets":replacement})).unwrap(),
                false
            )
            .is_err()
        );
    }
    for (name, bad) in [
        ("target", json!("private-label")),
        ("max_age_seconds", json!(0)),
        ("max_age_seconds", json!(2592001)),
        ("run_timeout_seconds", json!(0)),
        ("run_timeout_seconds", json!(3601)),
        ("run_timeout_seconds", json!(1.0)),
        ("path", json!("relative/path")),
        ("unexpected", json!(true)),
    ] {
        let mut wrong = value.clone();
        wrong["targets"][0][name] = bad;
        assert!(parse_config(&serde_json::to_vec(&wrong).unwrap(), false).is_err());
    }
    let encoded = serde_json::to_string(&root.join("application.json")).unwrap();
    for bytes in [
        format!(
            r#"{{"targets":[],"targets":[{{"target":"application","path":{encoded},"max_age_seconds":1,"run_timeout_seconds":1}}]}}"#
        ),
        format!(
            r#"{{"targets":[{{"target":"application","target":"host","path":{encoded},"max_age_seconds":1,"run_timeout_seconds":1}}]}}"#
        ),
    ] {
        assert!(parse_config(bytes.as_bytes(), false).is_err());
    }
    for middle in ["..", ".", ""] {
        let separator = std::path::MAIN_SEPARATOR;
        let mut path = root.as_os_str().to_os_string();
        path.push(format!("{separator}{middle}{separator}application.json"));
        assert!(parse_config(&document(Path::new(&path)), false).is_err());
    }
    assert!(parse_config(&[b' '; 16385], false).is_err());
}

#[test]
fn reads_bounded_regular_config_and_errors_never_contain_input() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("config.json");
    let mut bytes = document(&root.join("application.json"));
    bytes.resize(16384, b' ');
    std::fs::write(&file, &bytes).unwrap();
    assert!(read_config(&file, false).is_ok());
    bytes.push(b' ');
    std::fs::write(&file, bytes).unwrap();
    assert!(read_config(&file, false).is_err());
    assert!(read_config(&root, false).is_err());
    let error = read_config(&root.join("private-secret"), false).unwrap_err();
    assert!(!format!("{error} {error:?}").contains("private-secret"));
}

#[test]
fn root_authority_checks_reject_foreign_owners_and_shared_write_access() {
    assert!(trusted(0, 0o100644, true));
    assert!(trusted(0, 0o40755, true));
    assert!(!trusted(1000, 0o100644, true));
    assert!(!trusted(0, 0o100664, true));
    assert!(!trusted(0, 0o41777, true));
    assert!(trusted(1000, 0o100600, false));
}

#[test]
fn environment_child() {
    let Some(expectation) = std::env::var_os("MAINTENANCE_CONFIG_TEST_CHILD") else {
        return;
    };
    assert_eq!(from_env().is_ok(), expectation == "ok");
}

#[test]
fn required_mode_and_unrelated_env_credentials_are_checked_in_isolated_children() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("config.json");
    std::fs::write(&file, document(&root.join("application.json"))).unwrap();
    let run = |key: &str, value: &std::ffi::OsStr, expectation: &str| {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.env_clear();
        for key in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                child.env(key, value);
            }
        }
        let output = child
            .args(["--exact", "config::tests::environment_child"])
            .env("APP_ENV", "development")
            .env("MAINTENANCE_CONFIG_FILE", &file)
            .env("MAINTENANCE_CONFIG_TEST_CHILD", expectation)
            .env(key, value)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    };
    run("APP_ENV", "development".as_ref(), "ok");
    run("APP_ENV", "test".as_ref(), "error");
    run("APP_ENV", "".as_ref(), "error");
    run("MAINTENANCE_CONFIG_FILE", "".as_ref(), "error");
    for key in [
        "DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "MONITOR_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "PGHOST",
        "PGPASSWORD",
        "PGFUTURE",
    ] {
        run(key, "private-secret".as_ref(), "error");
        run(key, "".as_ref(), "error");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        run("PGHOST", std::ffi::OsStr::from_bytes(b"\xff"), "error");
        run(
            "DATABASE_URL",
            std::ffi::OsStr::from_bytes(b"\xff"),
            "error",
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_wide(&[0xd800]);
        run("PGHOST", &invalid, "error");
        run("DATABASE_URL", &invalid, "error");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn nofollow_reads_reject_symlink_components_fifo_and_untrusted_production_parent() {
    use rustix::fs::{CWD, Mode, mkfifoat};
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("config.json");
    std::fs::write(&file, document(&root.join("application.json"))).unwrap();
    symlink(&file, root.join("alias.json")).unwrap();
    assert!(read_config(&root.join("alias.json"), false).is_err());
    symlink(&root, root.join("alias-dir")).unwrap();
    assert!(read_config(&root.join("alias-dir/config.json"), false).is_err());
    let fifo = root.join("fifo");
    mkfifoat(CWD, &fifo, Mode::RUSR | Mode::WUSR).unwrap();
    assert!(read_config(&fifo, false).is_err());
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(read_config(&file, true).is_err());
}
