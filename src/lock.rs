use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{FALSE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::core::w;

pub(crate) enum Error {
    CreateFailed,
    WaitFailed,
    TimedOut,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        unsafe { ReleaseMutex(self.0.as_raw_handle()) };
    }
}

const MUTEX_NAME: *const u16 = w!(r"Local\umpv_lock");
// Bounds the longest run a lock holder can make: waiting for a free pipe
// instance (`pipe::CONNECT_TIMEOUT`) and then, if no server turned up, waiting
// for a freshly launched mpv to listen (`mpv::STARTUP_TIMEOUT`). Keep this at
// or above the sum of those two, or a waiter can give up on a healthy holder.
const ACQUIRE_TIMEOUT_MILLISECONDS: u32 = 10_000;

pub(crate) fn acquire() -> Result<Guard, Error> {
    let handle = unsafe { CreateMutexW(std::ptr::null(), FALSE, MUTEX_NAME) };
    if handle.is_null() {
        return Err(Error::CreateFailed);
    }
    let mutex = unsafe { OwnedHandle::from_raw_handle(handle) };

    match unsafe { WaitForSingleObject(mutex.as_raw_handle(), ACQUIRE_TIMEOUT_MILLISECONDS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Guard(mutex)),
        WAIT_TIMEOUT => Err(Error::TimedOut),
        _ => Err(Error::WaitFailed),
    }
}
