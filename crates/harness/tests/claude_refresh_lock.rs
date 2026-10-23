#![cfg(unix)]
//! A Claude child is never torn down while Claude Code is saving a refreshed
//! login: neither signalled nor sent EOF until the refresh lock is released.
//! Its own test binary — it points the process at a temp config dir.
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use zeron_harness::claude::refresh_lock;
use zeron_harness::{ClaudeHarness, Harness};

const FAKE_CLAUDE: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
case "$*" in
  *--input-format*) ;;
  *) echo "2.1.309 (Claude Code)"; exit 0 ;;
esac
trap 'echo term >> "$dir/events"; exit 0' TERM
read line
echo '{"type":"control_response","response":{"subtype":"success","request_id":"zeron-command-probe","response":{"commands":[{"name":"review","description":"Review"}]}}}'
while read line; do :; done
echo eof >> "$dir/events"
"#;

#[tokio::test]
async fn a_probe_is_retired_only_once_no_login_refresh_is_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude-config");
    std::fs::create_dir_all(&config).unwrap();
    // SAFETY: the only test in this binary; set before any thread reads it.
    unsafe { std::env::set_var("CLAUDE_SECURESTORAGE_CONFIG_DIR", &config) };
    let script = dir.path().join("claude");
    std::fs::write(&script, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let events = dir.path().join("events");

    // Another Claude process is mid-refresh.
    let [held, _] = refresh_lock::lock_paths(&config);
    std::fs::create_dir(&held).unwrap();

    let harness = ClaudeHarness::new().with_executable(&script);
    let commands = tokio::time::timeout(Duration::from_secs(10), harness.commands())
        .await
        .expect("the probe answers without waiting for the refresh")
        .unwrap();
    assert_eq!(commands[0].name, "review");

    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !events.exists(),
        "probe torn down mid-refresh: {:?}",
        std::fs::read_to_string(&events)
    );

    std::fs::remove_dir(&held).unwrap();
    for _ in 0..100 {
        if events.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("probe never retired once the refresh was saved");
}
