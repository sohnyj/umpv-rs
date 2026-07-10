use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::Foundation::{FALSE, HWND, LPARAM, TRUE};
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsIconic, SW_RESTORE,
    SetForegroundWindow, ShowWindow,
};
use windows_sys::core::BOOL;

use crate::pipe;

fn resolve_mpv_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("mpv.exe")))
}

pub(crate) fn launch_mpv() -> std::io::Result<()> {
    let mpv_path = resolve_mpv_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "mpv.exe not found."))?;
    Command::new(&mpv_path)
        .arg(format!("--input-ipc-server={}", pipe::PIPE_PATH))
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

const MPV_WINDOW_CLASS_NAME: [u16; 3] = [b'm' as u16, b'p' as u16, b'v' as u16];

unsafe extern "system" fn activate_window_if_mpv(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let target_pid = lparam as u32;
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid != target_pid {
            return TRUE;
        }
        let mut class_name = [0u16; 16];
        let length = GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32);
        if class_name.get(..length as usize) == Some(&MPV_WINDOW_CLASS_NAME[..]) {
            if IsIconic(hwnd) != FALSE {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
            return FALSE;
        }
        TRUE
    }
}

pub(crate) fn activate_mpv_window(pid: u32) {
    if pid == 0 {
        return;
    }
    unsafe { EnumWindows(Some(activate_window_if_mpv), pid as LPARAM) };
}
