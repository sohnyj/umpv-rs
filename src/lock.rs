use std::fmt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{FALSE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading;
use windows_sys::core::{PCWSTR, w};

pub(crate) enum Error {
    CreateFailed,
    WaitFailed,
    TimedOut,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CreateFailed => "Failed to create umpv lock.",
            Self::WaitFailed => "Failed to wait for umpv lock.",
            Self::TimedOut => {
                "Timed out waiting for umpv lock.\nAnother umpv instance is holding it."
            }
        })
    }
}

pub(crate) struct Guard(OwnedHandle);

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { Threading::ReleaseMutex(self.0.as_raw_handle()) };
    }
}

const MUTEX_NAME: PCWSTR = w!(r"Local\umpv_lock");
// Must stay at or above pipe::CONNECT_TIMEOUT + mpv::STARTUP_TIMEOUT, the
// longest the holder can take, or another instance times out while waiting.
const ACQUIRE_TIMEOUT_MILLISECONDS: u32 = 10_000;

pub(crate) fn acquire() -> Result<Guard, Error> {
    let handle = unsafe { Threading::CreateMutexW(ptr::null(), FALSE, MUTEX_NAME) };
    if handle.is_null() {
        return Err(Error::CreateFailed);
    }
    let mutex = unsafe { OwnedHandle::from_raw_handle(handle) };

    match unsafe {
        Threading::WaitForSingleObject(mutex.as_raw_handle(), ACQUIRE_TIMEOUT_MILLISECONDS)
    } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Guard(mutex)),
        WAIT_TIMEOUT => Err(Error::TimedOut),
        _ => Err(Error::WaitFailed),
    }
}
