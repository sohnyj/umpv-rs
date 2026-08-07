use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{FALSE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::encode_wide;

pub(crate) enum Error {
    CreateFailed,
    Timeout,
}

pub(crate) struct Guard(OwnedHandle);

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0.as_raw_handle()) };
    }
}

const MUTEX_NAME: &str = "umpv_lock";
const ACQUIRE_TIMEOUT_MS: u32 = 10_000;

pub(crate) fn acquire() -> Result<Guard, Error> {
    let mutex_name_wide = encode_wide(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(std::ptr::null(), FALSE, mutex_name_wide.as_ptr()) };
    if handle.is_null() {
        return Err(Error::CreateFailed);
    }
    let mutex = unsafe { OwnedHandle::from_raw_handle(handle) };

    let wait_result = unsafe { WaitForSingleObject(mutex.as_raw_handle(), ACQUIRE_TIMEOUT_MS) };
    if wait_result != WAIT_OBJECT_0 && wait_result != WAIT_ABANDONED {
        return Err(Error::Timeout);
    }
    Ok(Guard(mutex))
}
