use board_config::{MediaAdminSettings, MediaReaderSettings, MonitorSettings, Settings};
use std::{ffi::OsString, process::Command};

#[test]
fn unrelated_runtimes_reject_intake_credentials_before_connecting() {
    for (role, key, login) in [
        ("public", "DATABASE_URL", "board_public"),
        ("writer", "MEDIA_DATABASE_URL", "board_media"),
        ("reader", "MEDIA_READ_DATABASE_URL", "board_media_read"),
        ("observer", "MONITOR_DATABASE_URL", "board_monitor"),
    ] {
        for credential in [
            None,
            Some(OsString::from("synthetic-intake-secret")),
            Some(non_unicode()),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .env_clear()
                .args(["--exact", "intake_credential_child"])
                .env("INTAKE_CONFIG_TEST_ROLE", role)
                .env("APP_ENV", "development")
                .env(
                    key,
                    format!("postgres://{login}:synthetic@127.0.0.1:55432/imageboard"),
                );
            if let Some(root) = std::env::var_os("SystemRoot") {
                child.env("SystemRoot", root);
            }
            if let Some(credential) = credential {
                child.env("INTAKE_DATABASE_URL", credential);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{role}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }
}

#[test]
fn intake_credential_child() {
    let Ok(role) = std::env::var("INTAKE_CONFIG_TEST_ROLE") else {
        return;
    };
    let result = match role.as_str() {
        "public" => Settings::from_env().map(|_| ()),
        "writer" => MediaAdminSettings::from_env().map(|_| ()),
        "reader" => MediaReaderSettings::from_env().map(|_| ()),
        "observer" => MonitorSettings::from_env().map(|_| ()),
        _ => panic!("unexpected runtime fixture"),
    };
    if std::env::var_os("INTAKE_DATABASE_URL").is_some() {
        let error = result.expect_err("unrelated intake credential accepted");
        assert!(!error.to_string().contains("synthetic"));
    } else {
        result.expect("healthy runtime configuration rejected");
    }
}

fn non_unicode() -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff])
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800])
    }
}
