//! OS-gated sandbox smoke tests (Phase J / M4).

#[cfg(target_os = "linux")]
#[test]
fn bwrap_echo_succeeds_when_installed() {
    if !artui::sandbox::bwrap::is_available() {
        eprintln!("skip: bwrap not installed");
        return;
    }
    let ws = std::env::temp_dir().join("artui_bwrap_test");
    std::fs::create_dir_all(&ws).expect("mkdir");
    let args = artui::sandbox::bwrap::wrap_command("echo ok", &ws, &ws, false);
    let status = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status()
        .expect("spawn bwrap");
    assert!(status.success(), "bwrap echo failed");
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_echo_succeeds_when_installed() {
    use artui::sandbox::seatbelt::{is_available, wrap_command, SANDBOX_EXEC};
    if !is_available() {
        eprintln!("skip: {SANDBOX_EXEC} missing");
        return;
    }
    let ws = std::env::temp_dir().join("artui_seatbelt_test");
    std::fs::create_dir_all(&ws).expect("mkdir");
    let args = wrap_command("echo ok", &ws, &ws, false, false);
    let status = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status()
        .expect("spawn sandbox-exec");
    assert!(status.success(), "seatbelt echo failed");
}
