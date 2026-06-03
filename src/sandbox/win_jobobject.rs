//! Windows sandbox via Job Object (Phase M5).
//!
//! Assigns spawned shell children to a job with `KILL_ON_JOB_CLOSE`. Restricted-token
//! hardening can be layered on later; the job object gives lifecycle isolation today.

use std::ffi::c_void;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};

pub fn is_available() -> bool {
    true
}

pub fn wrap_command(command: &str, cwd: &Path, _workspace: &Path, _network: bool) -> Vec<String> {
    vec![
        "cmd.exe".to_owned(),
        "/C".to_owned(),
        command.to_owned(),
        cwd.to_string_lossy().into_owned(),
    ]
}

/// Closes the job handle on drop so error paths cannot leak Job Objects.
struct JobHandleGuard(HANDLE);

impl JobHandleGuard {
    fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for JobHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
            self.0 = HANDLE::default();
        }
    }
}

pub async fn run_command(
    command: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let command = command.to_owned();
    let cwd = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || run_command_blocking(&command, &cwd, timeout))
        .await
        .map_err(|e| format!("sandbox worker panicked: {e}"))?
}

fn run_command_blocking(
    command: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let job = create_job().map_err(|e| format!("job object: {e}"))?;
    let _job_guard = JobHandleGuard::new(job);

    let mut child = StdCommand::new("cmd")
        .arg("/C")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn shell: {e}"))?;

    if let Err(e) = assign_pid(_job_guard.raw(), child.id()) {
        tracing::warn!("sandbox: AssignProcessToJobObject failed: {e}");
    }

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| format!("command execution failed: {e}"));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("command execution failed: {e}")),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(format!("command timed out after {}ms", timeout.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn create_job() -> std::io::Result<HANDLE> {
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|_| std::io::Error::last_os_error())?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|_| std::io::Error::last_os_error())?;
    }
    Ok(job)
}

/// Assign `pid` to `job` without closing the job handle (caller owns lifecycle).
fn assign_pid(job: HANDLE, pid: u32) -> std::io::Result<()> {
    let process = unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION, false, pid)
            .map_err(|_| std::io::Error::last_os_error())?
    };
    unsafe {
        AssignProcessToJobObject(job, process).map_err(|_| std::io::Error::last_os_error())?;
        let _ = CloseHandle(process);
    }
    Ok(())
}
