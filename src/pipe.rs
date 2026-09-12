use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SEM_TIMEOUT, FALSE,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeServerProcessId, NMPWAIT_NOWAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::encode_wide;

pub(crate) enum Error {
    SessionIdUnavailable,
    ConnectFailed,
    WriteFailed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::SessionIdUnavailable => "Failed to determine the session id.",
            Self::ConnectFailed => "Failed to connect to mpv.",
            Self::WriteFailed => "Failed to send the file to mpv.",
        })
    }
}

/// Outcome of handing a file to the mpv IPC server.
pub(crate) enum Sent {
    /// A running mpv instance accepted the file. Its process id is absent when
    /// the pipe would not report it.
    Delivered { server_pid: Option<u32> },
    /// No mpv instance is listening on the pipe.
    NoServer,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const INSTANCE_WAIT_MILLISECONDS: u32 = 5;

fn session_id() -> Result<u32, Error> {
    let mut session_id: u32 = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session_id) } == FALSE {
        return Err(Error::SessionIdUnavailable);
    }
    Ok(session_id)
}

fn error_code(error: &std::io::Error) -> Option<u32> {
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

/// The mpv IPC pipe of the current session, resolved once at start-up.
pub(crate) struct Pipe {
    path: String,
    path_wide: Vec<u16>,
}

impl Pipe {
    pub(crate) fn for_current_session() -> Result<Self, Error> {
        let path = format!(r"\\.\pipe\umpv-{}", session_id()?);
        let path_wide = encode_wide(&path);
        Ok(Self { path, path_wide })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn server_exists(&self) -> bool {
        if unsafe { WaitNamedPipeW(self.path_wide.as_ptr(), NMPWAIT_NOWAIT) } != FALSE {
            return true;
        }
        error_code(&std::io::Error::last_os_error()) == Some(ERROR_SEM_TIMEOUT)
    }

    fn open(&self) -> std::io::Result<File> {
        OpenOptions::new()
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(&self.path)
    }

    /// Connects to the server, or returns `None` when no server is listening.
    fn connect(&self) -> Result<Option<File>, Error> {
        let timeout_at = Instant::now() + CONNECT_TIMEOUT;

        loop {
            match self.open() {
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
            unsafe { WaitNamedPipeW(self.path_wide.as_ptr(), INSTANCE_WAIT_MILLISECONDS) };
        }
    }

    pub(crate) fn send_loadfile(&self, file: &str, loadfile_flags: &str) -> Result<Sent, Error> {
        let Some(mut stream) = self.connect()? else {
            return Ok(Sent::NoServer);
        };
        let server_pid = server_pid(&stream);
        stream
            .write_all(loadfile_command(file, loadfile_flags).as_bytes())
            .map_err(|_| Error::WriteFailed)?;
        Ok(Sent::Delivered { server_pid })
    }
}
