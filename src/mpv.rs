use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::Foundation::{FALSE, HWND};
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

use crate::{encode_wide, pipe};

pub(crate) enum Error {
    NotFound,
    SpawnFailed(std::io::Error),
}

fn resolve_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("mpv.exe")))
}

pub(crate) fn launch(file: &str) -> Result<(), Error> {
    let mpv_path = resolve_path().ok_or(Error::NotFound)?;
    Command::new(&mpv_path)
        .arg(format!("--input-ipc-server={}", pipe::PIPE_PATH))
        .arg("--")
        .arg(file)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(Error::SpawnFailed)?;
    Ok(())
}

const MPV_WINDOW_CLASS_NAME: &str = "mpv";

fn find_window(pid: u32) -> Option<HWND> {
    let class_name_wide = encode_wide(MPV_WINDOW_CLASS_NAME);
    let mut hwnd: HWND = std::ptr::null_mut();
    loop {
        hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                hwnd,
                class_name_wide.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        let mut window_pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &raw mut window_pid) };
        if window_pid == pid {
            return Some(hwnd);
        }
    }
}

pub(crate) fn activate_window(pid: u32) {
    if pid == 0 {
        return;
    }
    let Some(hwnd) = find_window(pid) else {
        return;
    };
    if unsafe { IsIconic(hwnd) } != FALSE {
        unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    unsafe { SetForegroundWindow(hwnd) };
}
