use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, FALSE};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::encode_wide;

pub(crate) enum Error {
    NotRunning,
    ConnectFailed,
    WriteFailed,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

fn session_id() -> u32 {
    let mut session_id: u32 = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session_id) } == FALSE {
        return 0;
    }
    session_id
}

pub(crate) fn path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| format!(r"\\.\pipe\umpv-{}", session_id()))
}

fn open_pipe() -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(path())
}

fn error_code(error: &std::io::Error) -> u32 {
    error.raw_os_error().unwrap_or_default().cast_unsigned()
}

pub(crate) fn server_exists() -> bool {
    match open_pipe() {
        Ok(_) => true,
        Err(error) => error_code(&error) == ERROR_PIPE_BUSY,
    }
}

fn connect() -> Result<File, Error> {
    let timeout_at = Instant::now() + CONNECT_TIMEOUT;

    loop {
        match open_pipe() {
            Ok(pipe) => return Ok(pipe),
            Err(error) => match error_code(&error) {
                ERROR_FILE_NOT_FOUND => return Err(Error::NotRunning),
                ERROR_PIPE_BUSY => {}
                _ => return Err(Error::ConnectFailed),
            },
        }

        let remaining = timeout_at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::ConnectFailed);
        }
        let timeout_milliseconds = u32::try_from(remaining.as_millis()).unwrap_or(u32::MAX);
        let pipe_path_wide = encode_wide(path());
        unsafe { WaitNamedPipeW(pipe_path_wide.as_ptr(), timeout_milliseconds) };
    }
}

fn server_pid(pipe: &File) -> u32 {
    let mut pid: u32 = 0;
    unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &raw mut pid) };
    pid
}

fn loadfile_command(file: &str, loadfile: &str) -> String {
    let escaped = file
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("raw loadfile \"{escaped}\" {loadfile}\n")
}

pub(crate) fn send_file(file: &str, loadfile: &str) -> Result<u32, Error> {
    let mut pipe = connect()?;
    let pid = server_pid(&pipe);
    pipe.write_all(loadfile_command(file, loadfile).as_bytes())
        .map_err(|_| Error::WriteFailed)?;
    Ok(pid)
}
