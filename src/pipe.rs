use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, FALSE, GENERIC_WRITE, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::encode_wide;

pub(crate) enum MutexError {
    Create,
    Timeout,
}

pub(crate) enum SendError {
    Connect(u32),
    Write,
}

const MUTEX_NAME: &str = "umpv_mutex";
const MUTEX_TIMEOUT_MS: u32 = 10_000;
const PIPE_BUSY_TIMEOUT_MS: u32 = 5_000;
pub(crate) const PIPE_PATH: &str = r"\\.\pipe\umpv";
const RETRY_INTERVAL_MS: u64 = 100;
const RETRY_MAX_ATTEMPTS: u32 = 50;

pub(crate) struct MutexGuard(HANDLE);

impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn acquire_mutex() -> Result<MutexGuard, MutexError> {
    let mutex_name_wide = encode_wide(MUTEX_NAME);
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, mutex_name_wide.as_ptr());
        if handle.is_null() {
            return Err(MutexError::Create);
        }
        let wait_result = WaitForSingleObject(handle, MUTEX_TIMEOUT_MS);
        if wait_result != WAIT_OBJECT_0 && wait_result != WAIT_ABANDONED {
            CloseHandle(handle);
            return Err(MutexError::Timeout);
        }
        Ok(MutexGuard(handle))
    }
}

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

fn connect(retry: bool) -> Result<HANDLE, u32> {
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
    unsafe { GetNamedPipeServerProcessId(handle, &mut pid) };
    pid
}

fn write_bytes(handle: HANDLE, data: &[u8]) -> bool {
    let mut offset = 0;
    while offset < data.len() {
        let mut bytes_written: u32 = 0;
        let ok = unsafe {
            WriteFile(
                handle,
                data[offset..].as_ptr(),
                (data.len() - offset) as u32,
                &mut bytes_written,
                std::ptr::null_mut(),
            )
        };
        if ok == FALSE || bytes_written == 0 {
            return false;
        }
        offset += bytes_written as usize;
    }
    true
}

fn write_commands(handle: HANDLE, files: &[String], loadfile: &str) -> bool {
    let mut buffer = String::new();
    for file in files {
        buffer.push_str("raw loadfile \"");
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
    }
    write_bytes(handle, buffer.as_bytes())
}

pub(crate) fn send_files(files: &[String], loadfile: &str, retry: bool) -> Result<u32, SendError> {
    let handle = connect(retry).map_err(SendError::Connect)?;
    let pid = server_pid(handle);
    let ok = write_commands(handle, files, loadfile);
    unsafe { CloseHandle(handle) };
    if ok { Ok(pid) } else { Err(SendError::Write) }
}
