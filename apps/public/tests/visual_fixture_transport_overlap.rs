//! Diagnostic-only bounded streaming body and real fixture listener ownership.
#[path = "../examples/visual/listeners.rs"]
mod listeners;
#[path = "../examples/visual/transport_overlap.rs"]
mod transport_overlap;

// Exercise the public wiring too; the gate is off on non-Windows, and is never
// enabled by these tests through process-global environment mutation.
#[test]
fn environment_wiring_constructs_router_without_binding_or_spawning() {
    let _ = transport_overlap::from_env(axum::Router::new());
}

#[test]
fn diagnostic_listener_rejects_invalid_profile_with_configuration_name() {
    let error =
        listeners::MediaProfile::parse(Some(std::ffi::OsStr::new("unsupported"))).unwrap_err();
    assert!(error.to_string().contains(listeners::PROFILE_ENV));
}
