use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};

use crate::encode_wide;

pub(crate) enum Error {
    NotRunning,
    ConnectFailed,
    WriteFailed,
}

pub(crate) const PIPE_PATH: &str = r"\\.\pipe\umpv";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_INTERVAL: Duration = Duration::from_millis(25);

fn open_pipe() -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(PIPE_PATH)
}

fn connect() -> Result<File, Error> {
    let pipe_path_wide = encode_wide(PIPE_PATH);
    let timeout_at = Instant::now() + CONNECT_TIMEOUT;

    loop {
        match open_pipe() {
            Ok(pipe) => return Ok(pipe),
            Err(error) => match error.raw_os_error().unwrap_or_default().cast_unsigned() {
                ERROR_FILE_NOT_FOUND => return Err(Error::NotRunning),
                ERROR_PIPE_BUSY => {}
                _ => return Err(Error::ConnectFailed),
            },
        }

        let remaining = timeout_at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::ConnectFailed);
        }
        let timeout_ms = u32::try_from(remaining.as_millis()).unwrap_or(u32::MAX);
        unsafe { WaitNamedPipeW(pipe_path_wide.as_ptr(), timeout_ms) };
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

pub(crate) fn wait_for_server() {
    let timeout_at = Instant::now() + CONNECT_TIMEOUT;
    while open_pipe().is_err() && Instant::now() < timeout_at {
        std::thread::sleep(RETRY_INTERVAL);
    }
}
