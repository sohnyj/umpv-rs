use std::fmt;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{FALSE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading;
use windows_sys::Win32::UI::WindowsAndMessaging::{self, SW_RESTORE};
use windows_sys::core::{PCWSTR, w};

use crate::pipe::Pipe;

pub(crate) enum Error {
    SpawnFailed(io::Error),
    Exited,
    WaitFailed,
    StartupTimedOut,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpawnFailed(error) => write!(formatter, "Failed to launch mpv.exe: {error}"),
            Self::Exited => formatter.write_str("mpv.exe exited before it opened the file."),
            Self::WaitFailed => formatter.write_str("Failed to wait for mpv.exe."),
            Self::StartupTimedOut => formatter.write_str("Timed out waiting for mpv.exe to start."),
        }
    }
}

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL_MILLISECONDS: u32 = 5;

pub(crate) fn launch(mpv_path: &Path, pipe: &Pipe, file: &str) -> Result<(), Error> {
    let mpv_process = Command::new(mpv_path)
        .arg(format!("--input-ipc-server={}", pipe.path()))
        .arg("--")
        .arg(file)
        .spawn()
        .map_err(Error::SpawnFailed)?;
    unsafe { WindowsAndMessaging::AllowSetForegroundWindow(mpv_process.id()) };
    wait_for_ipc_server(pipe, &mpv_process)
}

fn wait_for_ipc_server(pipe: &Pipe, mpv_process: &Child) -> Result<(), Error> {
    let timeout_at = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if pipe.server_exists() {
            return Ok(());
        }
        if Instant::now() >= timeout_at {
            return Err(Error::StartupTimedOut);
        }
        match unsafe {
            Threading::WaitForSingleObject(mpv_process.as_raw_handle(), POLL_INTERVAL_MILLISECONDS)
        } {
            WAIT_TIMEOUT => {}
            WAIT_OBJECT_0 => return Err(Error::Exited),
            _ => return Err(Error::WaitFailed),
        }
    }
}

const MPV_WINDOW_CLASS_NAME: PCWSTR = w!("mpv");

fn find_window(pid: u32) -> Option<HWND> {
    let mut hwnd: HWND = ptr::null_mut();
    loop {
        hwnd = unsafe {
            WindowsAndMessaging::FindWindowExW(
                ptr::null_mut(),
                hwnd,
                MPV_WINDOW_CLASS_NAME,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        let mut window_pid: u32 = 0;
        unsafe { WindowsAndMessaging::GetWindowThreadProcessId(hwnd, &raw mut window_pid) };
        if window_pid == pid {
            return Some(hwnd);
        }
    }
}

pub(crate) fn activate_window(pid: u32) {
    let Some(hwnd) = find_window(pid) else {
        return;
    };
    if unsafe { WindowsAndMessaging::IsIconic(hwnd) } != FALSE {
        unsafe { WindowsAndMessaging::ShowWindow(hwnd, SW_RESTORE) };
    }
    unsafe { WindowsAndMessaging::SetForegroundWindow(hwnd) };
}
