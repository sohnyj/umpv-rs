use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::encode_wide;

pub(crate) enum Error {
    CreateFailed,
    Timeout,
}

pub(crate) struct Guard(HANDLE);

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

const MUTEX_NAME: &str = "umpv_mutex";
const ACQUIRE_TIMEOUT_MS: u32 = 10_000;

pub(crate) fn acquire() -> Result<Guard, Error> {
    let mutex_name_wide = encode_wide(MUTEX_NAME);
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), FALSE, mutex_name_wide.as_ptr());
        if handle.is_null() {
            return Err(Error::CreateFailed);
        }
        let wait_result = WaitForSingleObject(handle, ACQUIRE_TIMEOUT_MS);
        if wait_result != WAIT_OBJECT_0 && wait_result != WAIT_ABANDONED {
            CloseHandle(handle);
            return Err(Error::Timeout);
        }
        Ok(Guard(handle))
    }
}
