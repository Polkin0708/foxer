use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "forex-service-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_forex-bot"))
}
#[test]
fn empty_env_fails_without_exposing_contents_or_starting_service() {
    let p = path();
    std::fs::write(&p, "").unwrap();
    let out = exe()
        .args(["check-service", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("missing setting"));
}
#[test]
fn smoke_binary_restores_queue_and_refuses_existing_database() {
    let p = path();
    let out = exe()
        .args([
            "smoke-test",
            p.to_str().unwrap(),
            "../strategy/fvg-bot-rules.json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Smoke test OK"));
    let before = std::fs::read(&p).unwrap();
    let out = exe()
        .args([
            "smoke-test",
            p.to_str().unwrap(),
            "../strategy/fvg-bot-rules.json",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(std::fs::read(&p).unwrap(), before);
}

#[test]
fn upscale_probe_checks_env_without_starting_service() {
    let p = path();
    let out = exe()
        .args(["probe-upscale", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read service settings"));
    assert!(out.stdout.is_empty());
}
