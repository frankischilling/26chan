use super::*;
use serde_json::json;
use std::fs;

fn fixture(path: &Path) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "storages": [{"target":"database", "path":path}],
        "services": [{"target":"public", "path":path}]
    }))
    .unwrap()
}

#[test]
fn maps_closed_targets_to_fixed_slots_and_permits_shared_paths() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    let bytes = serde_json::to_vec(&json!({
        "storages":[{"target":"monitoring","path":path},{"target":"database","path":path}],
        "services":[{"target":"alertmanager","path":path},{"target":"public","path":path}]
    }))
    .unwrap();
    let targets = parse_config(&bytes).unwrap();
    assert_eq!(targets.storages[0].as_ref(), Some(&path));
    assert!(targets.storages[1].is_none());
    assert_eq!(targets.storages[3].as_ref(), Some(&path));
    assert_eq!(targets.services[0].as_ref(), Some(&path));
    assert!(targets.services[1].is_none());
    assert_eq!(targets.services[9].as_ref(), Some(&path));
}

#[test]
fn rejects_unknown_duplicate_missing_empty_and_wrongly_typed_config() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    let good: serde_json::Value = serde_json::from_slice(&fixture(&path)).unwrap();
    let mut cases = vec![json!({}), json!({"storages":[],"services":[]})];
    let mut value = good.clone();
    value["unexpected"] = json!(true);
    cases.push(value);
    let mut value = good.clone();
    value["storages"][0]["target"] = json!("secret-path-label");
    cases.push(value);
    let mut value = good.clone();
    value["storages"][0]["extra"] = json!(true);
    cases.push(value);
    let mut value = good.clone();
    value["services"][0]["path"] = json!(42);
    cases.push(value);
    let mut value = good.clone();
    value["services"] = json!([]);
    cases.push(value);
    let mut value = good.clone();
    value["storages"] = json!([good["storages"][0], good["storages"][0]]);
    cases.push(value);
    let mut value = good.clone();
    value["services"] = json!([good["services"][0], good["services"][0]]);
    cases.push(value);
    for value in cases {
        assert!(parse_config(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let encoded = serde_json::to_string(&path).unwrap();
    for bytes in [
        format!(
            r#"{{"storages":[],"storages":[{{"target":"database","path":{encoded}}}],"services":[{{"target":"public","path":{encoded}}}]}}"#
        ),
        format!(
            r#"{{"storages":[{{"target":"database","target":"database","path":{encoded}}}],"services":[{{"target":"public","path":{encoded}}}]}}"#
        ),
    ] {
        assert!(parse_config(bytes.as_bytes()).is_err());
    }
}

#[test]
fn rejects_noncanonical_missing_relative_and_nondirectory_sources_without_disclosure() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::write(root.join("private-payload"), "private-content").unwrap();
    // Windows verbatim PathBuf::join normalizes dots before the parser sees them.
    let mut noncanonical = root.as_os_str().to_os_string();
    noncanonical.push(format!("{}.", std::path::MAIN_SEPARATOR));
    for path in [
        root.join("missing"),
        root.join("private-payload"),
        PathBuf::from("relative-private-path"),
        PathBuf::from(noncanonical),
    ] {
        let error = parse_config(&fixture(&path)).unwrap_err();
        let display = format!("{error} {error:?}");
        assert!(!display.contains("private"));
        assert!(!display.contains(&root.to_string_lossy().to_string()));
    }
}

#[test]
fn reads_only_bounded_regular_canonical_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("config.json");
    fs::write(&file, fixture(&root)).unwrap();
    assert!(read_config(&file).is_ok());
    assert!(read_config(&root).is_err());
    assert!(read_config(&root.join("missing")).is_err());
    let mut exact = fixture(&root);
    exact.resize(16 * 1024, b' ');
    fs::write(&file, &exact).unwrap();
    assert!(read_config(&file).is_ok());
    exact.push(b' ');
    fs::write(&file, &exact).unwrap();
    assert!(read_config(&file).is_err());
    assert!(parse_config(&exact).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_source_and_config_components() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::create_dir(root.join("real")).unwrap();
    symlink(root.join("real"), root.join("alias")).unwrap();
    assert!(parse_config(&fixture(&root.join("alias"))).is_err());
    fs::write(root.join("real/config.json"), fixture(&root)).unwrap();
    assert!(read_config(&root.join("alias/config.json")).is_err());
    symlink(root.join("real/config.json"), root.join("config-link")).unwrap();
    assert!(read_config(&root.join("config-link")).is_err());
}

// Environment changes belong to fresh child processes, never unsafe set_var.
#[test]
fn environment_child() {
    let Some(expectation) = std::env::var_os("RESOURCE_CONFIG_TEST_CHILD") else {
        return;
    };
    assert_eq!(from_env().is_ok(), expectation == "ok");
}

#[test]
fn requires_explicit_environment_and_rejects_unrelated_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let file = root.join("config.json");
    fs::write(&file, fixture(&root)).unwrap();
    let run = |key: &str, value: &std::ffi::OsStr, expected: &str| {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.env_clear();
        for key in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                child.env(key, value);
            }
        }
        child
            .args(["--exact", "config::tests::environment_child"])
            .env("APP_ENV", "development")
            .env("RESOURCE_CONFIG_FILE", &file)
            .env("RESOURCE_CONFIG_TEST_CHILD", expected)
            .env(key, value);
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child test did not run"
        );
    };
    run("APP_ENV", "production".as_ref(), "ok");
    run("APP_ENV", "".as_ref(), "error");
    run("APP_ENV", "test".as_ref(), "error");
    run("RESOURCE_CONFIG_FILE", "".as_ref(), "error");
    for key in [
        "DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "MONITOR_DATABASE_URL",
        "PGHOST",
        "PGPASSWORD",
        "PGFUTUREOPTION",
    ] {
        run(key, "private-value".as_ref(), "error");
        run(key, "".as_ref(), "error");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        run(
            "MONITOR_DATABASE_URL",
            std::ffi::OsStr::from_bytes(b"\xff"),
            "error",
        );
        run("PGHOST", std::ffi::OsStr::from_bytes(b"\xff"), "error");
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_wide(&[0xd800]);
        run("MONITOR_DATABASE_URL", &invalid, "error");
        run("PGHOST", &invalid, "error");
    }
}
