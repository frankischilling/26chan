use board_config::{MediaAdminSettings, MediaReaderSettings, MonitorSettings, Settings};
use std::{ffi::OsString, process::Command};

#[test]
fn application_roles_reject_staff_identity_sources_before_connecting() {
    for role in ["public", "reader", "writer", "observer"] {
        for source in [
            "",
            "STAFF_POSTER_ID_KEY",
            "STAFF_COUNTRY_DATABASE",
            "STAFF_PROXY_SOCKET",
            "STAFF_PROXY_UID",
        ] {
            for value in [
                OsString::from("owned-private-staff-source"),
                invalid_unicode(),
            ] {
                let mut child = Command::new(std::env::current_exe().unwrap());
                child
                    .env_clear()
                    .args(["--exact", "staff_source_config_child", "--nocapture"]);
                #[cfg(windows)]
                for key in ["PATH", "SystemRoot"] {
                    if let Some(value) = std::env::var_os(key) {
                        child.env(key, value);
                    }
                }
                child
                    .env("STAFF_SOURCE_CONFIG_ROLE", role)
                    .env(
                        "STAFF_SOURCE_CONFIG_PRESENT",
                        if source.is_empty() { "false" } else { "true" },
                    )
                    .env("APP_ENV", "development");
                let (name, user) = match role {
                    "public" => ("DATABASE_URL", "board_public"),
                    "reader" => ("MEDIA_READ_DATABASE_URL", "board_media_read"),
                    "writer" => ("MEDIA_DATABASE_URL", "board_media"),
                    "observer" => ("MONITOR_DATABASE_URL", "board_monitor"),
                    _ => unreachable!(),
                };
                child.env(
                    name,
                    format!("postgres://{user}:unused@127.0.0.1:55432/imageboard"),
                );
                if !source.is_empty() {
                    child.env(source, value);
                }
                let output = child.output().unwrap();
                assert!(
                    output.status.success(),
                    "{role} {source}: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
                assert!(
                    !String::from_utf8_lossy(&output.stdout).contains("owned-private-staff-source")
                );
                assert!(
                    !String::from_utf8_lossy(&output.stderr).contains("owned-private-staff-source")
                );
            }
        }
    }
}

#[test]
fn staff_source_config_child() {
    let Ok(role) = std::env::var("STAFF_SOURCE_CONFIG_ROLE") else {
        return;
    };
    let present = std::env::var("STAFF_SOURCE_CONFIG_PRESENT").unwrap() == "true";
    let result = match role.as_str() {
        "public" => Settings::from_env().map(|_| ()),
        "reader" => MediaReaderSettings::from_env().map(|_| ()),
        "writer" => MediaAdminSettings::from_env().map(|_| ()),
        "observer" => MonitorSettings::from_env().map(|_| ()),
        _ => unreachable!(),
    };
    match (present, result) {
        (false, Ok(())) => (),
        (true, Err(error)) => {
            let text = error.to_string();
            assert!(!text.contains("owned-private-staff-source"));
            let expected = match role.as_str() {
                "public" => "Operator or staff credentials",
                "reader" => "Media readers must not inherit",
                "writer" => "The media operator command must not inherit",
                "observer" => "Observer received an unrelated",
                _ => unreachable!(),
            };
            assert!(text.contains(expected));
        }
        _ => panic!("Staff identity source isolation failed."),
    }
}

fn invalid_unicode() -> OsString {
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
