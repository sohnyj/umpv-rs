use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, FALSE, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};

use crate::encode_wide;

pub(crate) enum Error {
    ConnectFailed(u32),
    WriteFailed,
}

pub(crate) const PIPE_PATH: &str = r"\\.\pipe\umpv";

fn open_pipe(pipe_path_wide: &[u16]) -> HANDLE {
    unsafe {
        CreateFileW(
            pipe_path_wide.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            std::ptr::null_mut(),
        )
    }
}

const PIPE_BUSY_TIMEOUT_MS: u32 = 5_000;
const RETRY_INTERVAL_MS: u64 = 100;
const RETRY_MAX_ATTEMPTS: u32 = 50;

fn connect_pipe(retry: bool) -> Result<HANDLE, u32> {
    let pipe_path_wide = encode_wide(PIPE_PATH);
    let max_attempts = if retry { RETRY_MAX_ATTEMPTS } else { 1 };
    let mut last_error = ERROR_FILE_NOT_FOUND;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
        }

        let handle = open_pipe(&pipe_path_wide);
        if handle != INVALID_HANDLE_VALUE {
            return Ok(handle);
        }

        unsafe {
            last_error = GetLastError();
            match last_error {
                ERROR_PIPE_BUSY => {
                    if WaitNamedPipeW(pipe_path_wide.as_ptr(), PIPE_BUSY_TIMEOUT_MS) != FALSE {
                        let handle = open_pipe(&pipe_path_wide);
                        if handle != INVALID_HANDLE_VALUE {
                            return Ok(handle);
                        }
                        last_error = GetLastError();
                    }
                }
                ERROR_FILE_NOT_FOUND => {}
                error => return Err(error),
            }
        }
    }

    Err(last_error)
}

fn server_pid(handle: HANDLE) -> u32 {
    let mut pid: u32 = 0;
    unsafe { GetNamedPipeServerProcessId(handle, &raw mut pid) };
    pid
}

fn write_bytes(handle: HANDLE, data: &[u8]) -> bool {
    let mut offset = 0;
    while offset < data.len() {
        let mut bytes_written: u32 = 0;
        let succeeded = unsafe {
            WriteFile(
                handle,
                data[offset..].as_ptr(),
                (data.len() - offset) as u32,
                &raw mut bytes_written,
                std::ptr::null_mut(),
            )
        };
        if succeeded == FALSE || bytes_written == 0 {
            return false;
        }
        offset += bytes_written as usize;
    }
    true
}

fn write_command(handle: HANDLE, file: &str, loadfile: &str) -> bool {
    let mut buffer = String::from("raw loadfile \"");
    for ch in file.chars() {
        match ch {
            '\\' => buffer.push_str("\\\\"),
            '"' => buffer.push_str("\\\""),
            '\n' => buffer.push_str("\\n"),
            _ => buffer.push(ch),
        }
    }
    buffer.push_str("\" ");
    buffer.push_str(loadfile);
    buffer.push('\n');
    write_bytes(handle, buffer.as_bytes())
}

pub(crate) fn send_file(file: &str, loadfile: &str, retry: bool) -> Result<u32, Error> {
    let handle = connect_pipe(retry).map_err(Error::ConnectFailed)?;
    let pid = server_pid(handle);
    let succeeded = write_command(handle, file, loadfile);
    unsafe { CloseHandle(handle) };
    if succeeded {
        Ok(pid)
    } else {
        Err(Error::WriteFailed)
    }
}
