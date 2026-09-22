use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::process;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT, FALSE, GetLastError,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeServerProcessId, NMPWAIT_NOWAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;

use crate::encode_wide;

pub(crate) enum Error {
    SessionIdUnavailable,
    ConnectFailed,
    WriteFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SessionIdUnavailable => "Failed to determine the session id.",
            Self::ConnectFailed => "Failed to connect to mpv.",
            Self::WriteFailed => "Failed to send the file to mpv.",
        })
    }
}

pub(crate) enum SendOutcome {
    /// `server_pid` is absent when querying it failed.
    Sent {
        server_pid: Option<u32>,
    },
    NoServer,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const INSTANCE_WAIT_MILLISECONDS: u32 = 5;

fn session_id() -> Result<u32, Error> {
    let mut session_id: u32 = 0;
    if unsafe { ProcessIdToSessionId(process::id(), &raw mut session_id) } == FALSE {
        return Err(Error::SessionIdUnavailable);
    }
    Ok(session_id)
}

fn error_code(error: &io::Error) -> Option<u32> {
    error.raw_os_error().map(i32::cast_unsigned)
}

fn server_pid(stream: &File) -> Option<u32> {
    let mut pid: u32 = 0;
    if unsafe { GetNamedPipeServerProcessId(stream.as_raw_handle(), &raw mut pid) } == FALSE {
        return None;
    }
    Some(pid)
}

fn loadfile_command(file: &str, loadfile_flags: &str) -> String {
    let escaped = file
        .replace('\\', r"\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("raw loadfile \"{escaped}\" {loadfile_flags}\n")
}

pub(crate) struct Pipe(String);

impl Pipe {
    pub(crate) fn for_current_session() -> Result<Self, Error> {
        Ok(Self(format!(r"\\.\pipe\umpv-{}", session_id()?)))
    }

    pub(crate) fn path(&self) -> &str {
        &self.0
    }

    /// Freeing the encoded path can overwrite the last error, so it is read first.
    fn wait_for_instance(&self, timeout_milliseconds: u32) -> Result<(), u32> {
        let path_wide = encode_wide(self.path());
        if unsafe { WaitNamedPipeW(path_wide.as_ptr(), timeout_milliseconds) } != FALSE {
            return Ok(());
        }
        Err(unsafe { GetLastError() })
    }

    pub(crate) fn server_exists(&self) -> bool {
        match self.wait_for_instance(NMPWAIT_NOWAIT) {
            Ok(()) => true,
            Err(last_error) => last_error == ERROR_SEM_TIMEOUT,
        }
    }

    fn open_stream(&self) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(self.path())
    }

    /// `Ok(None)` when no server is listening.
    fn connect(&self) -> Result<Option<File>, Error> {
        let timeout_at = Instant::now() + CONNECT_TIMEOUT;

        loop {
            match self.open_stream() {
                Ok(stream) => return Ok(Some(stream)),
                Err(error) => match error_code(&error) {
                    Some(ERROR_FILE_NOT_FOUND) => return Ok(None),
                    Some(ERROR_PIPE_BUSY) => {}
                    _ => return Err(Error::ConnectFailed),
                },
            }
            if Instant::now() >= timeout_at {
                return Err(Error::ConnectFailed);
            }
            let _ = self.wait_for_instance(INSTANCE_WAIT_MILLISECONDS);
        }
    }

    pub(crate) fn send_loadfile(
        &self,
        file: &str,
        loadfile_flags: &str,
    ) -> Result<SendOutcome, Error> {
        let Some(mut stream) = self.connect()? else {
            return Ok(SendOutcome::NoServer);
        };
        let server_pid = server_pid(&stream);
        stream
            .write_all(loadfile_command(file, loadfile_flags).as_bytes())
            .map_err(|_| Error::WriteFailed)?;
        Ok(SendOutcome::Sent { server_pid })
    }
}
