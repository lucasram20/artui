//! Windows sandbox via Job Object (Phase M5).
//!
//! Assigns spawned shell children to a job with `KILL_ON_JOB_CLOSE`. Restricted-token
//! hardening can be layered on later; the job object gives lifecycle isolation today.

use std::ffi::c_void;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
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

pub async fn run_command(
    command: &str,
    cwd: &Path,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let job = create_job().map_err(|e| format!("job object: {e}"))?;
    let mut job_handle = Some(job);

    let child = Command::new("cmd")
        .arg("/C")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to spawn shell: {e}"))?;

    if let Some(pid) = child.id() {
        let job = job_handle
            .take()
            .expect("job handle present while assigning child");
        if let Err(e) = assign_pid(job, pid) {
            tracing::warn!("sandbox: AssignProcessToJobObject failed: {e}");
            close_job(Some(job));
        } else {
            job_handle = Some(job);
        }
    } else {
        close_job(job_handle.take());
    }

    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("command timed out after {}ms", timeout.as_millis()))?
        .map_err(|e| format!("command execution failed: {e}"))?;

    // Job must stay open until the child exits; closing it early kills the process.
    close_job(job_handle.take());
    Ok(output)
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

fn close_job(job: Option<HANDLE>) {
    if let Some(h) = job {
        unsafe {
            let _ = CloseHandle(h);
        }
    }
}
