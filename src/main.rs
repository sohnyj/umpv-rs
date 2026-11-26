#![windows_subsystem = "windows"]

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::iter;
use std::path::{self, Path, PathBuf};
use std::process;
use std::ptr;

use windows_sys::Win32::UI::WindowsAndMessaging::{self, MB_OK};
use windows_sys::core::w;

use crate::command_line::{Command, LoadfileFlags};
use crate::pipe::{Pipe, SendOutcome};
use crate::registry::Unregistered;

mod command_line;
mod lock;
mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    string.encode_utf16().chain(iter::once(0)).collect()
}

fn show_message(text: &str) {
    let text_wide = encode_wide(text);
    // SAFETY: Null owner is allowed. `text_wide` is NUL-terminated by `encode_wide`
    // and outlives call. Caption is `w!` literal.
    unsafe {
        WindowsAndMessaging::MessageBoxW(ptr::null_mut(), text_wide.as_ptr(), w!("umpv"), MB_OK);
    }
}

fn exit_with_error(error: &dyn fmt::Display) -> ! {
    show_message(&error.to_string());
    process::exit(1);
}

enum Error {
    UnsupportedUrl,
    AbsolutePathFailed(io::Error),
    NonUnicodePath(OsString),
    Registry(registry::Error),
    Lock(lock::Error),
    Pipe(pipe::Error),
    Mpv(mpv::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedUrl => {
                formatter.write_str("URLs are not supported.\nOnly local files can be opened.")
            }
            Self::AbsolutePathFailed(error) => {
                write!(formatter, "Failed to make the file path absolute: {error}")
            }
            Self::NonUnicodePath(path) => {
                write!(
                    formatter,
                    "File path is not valid Unicode: {}",
                    path.display()
                )
            }
            Self::Registry(error) => fmt::Display::fmt(error, formatter),
            Self::Lock(error) => fmt::Display::fmt(error, formatter),
            Self::Pipe(error) => fmt::Display::fmt(error, formatter),
            Self::Mpv(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

fn has_url_scheme(argument: &str) -> bool {
    let Some((scheme, _)) = argument.split_once("://") else {
        return false;
    };
    scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn make_absolute(path: &str) -> Result<String, Error> {
    path::absolute(path)
        .map_err(Error::AbsolutePathFailed)?
        .into_os_string()
        .into_string()
        .map_err(Error::NonUnicodePath)
}

fn umpv_path() -> PathBuf {
    env::current_exe().expect("running process should know its own executable path")
}

fn mpv_path() -> PathBuf {
    umpv_path().with_file_name("mpv.exe")
}

fn register(loadfile_flags: LoadfileFlags) -> Result<(), Error> {
    let shell_open_command = command_line::shell_open_command(&umpv_path(), loadfile_flags);
    let extension_count = registry::register(&shell_open_command).map_err(Error::Registry)?;
    show_message(&format!(
        "Registered extensions: {extension_count}\nloadfile: {loadfile_flags}"
    ));
    Ok(())
}

fn unregister() -> Result<(), Error> {
    match registry::unregister().map_err(Error::Registry)? {
        Unregistered::Nothing => show_message("Nothing to unregister."),
        Unregistered::ProgIdOnly => {
            show_message("Removed the umpv ProgID.\nNo file extensions were pointing at umpv.");
        }
        Unregistered::Extensions(extension_count) => {
            show_message(&format!("Unregistered extensions: {extension_count}"));
        }
    }
    Ok(())
}

/// `None` when nothing needs raising: a new mpv raises its own window, and an
/// unidentified instance cannot be found.
fn open_in_mpv(
    pipe: &Pipe,
    mpv_path: &Path,
    file: &str,
    loadfile_flags: LoadfileFlags,
) -> Result<Option<u32>, Error> {
    // Showing an error here would hold the lock until its message box is closed.
    let _lock_guard = lock::acquire().map_err(Error::Lock)?;

    match pipe
        .send_loadfile(file, loadfile_flags)
        .map_err(Error::Pipe)?
    {
        SendOutcome::Sent { server_pid } => Ok(server_pid),
        SendOutcome::NoServer => {
            mpv::launch(mpv_path, pipe, file).map_err(Error::Mpv)?;
            Ok(None)
        }
    }
}

fn open(file: Option<&str>, loadfile_flags: LoadfileFlags) -> Result<(), Error> {
    let Some(file) = file else {
        return Ok(());
    };
    if has_url_scheme(file) {
        return Err(Error::UnsupportedUrl);
    }
    let file = make_absolute(file)?;

    let pipe = Pipe::for_current_session().map_err(Error::Pipe)?;

    if let Some(mpv_pid) = open_in_mpv(&pipe, &mpv_path(), &file, loadfile_flags)? {
        mpv::activate_window(mpv_pid);
    }
    Ok(())
}

fn run(command: Command) -> Result<(), Error> {
    match command {
        Command::Register { loadfile_flags } => register(loadfile_flags),
        Command::Unregister => unregister(),
        Command::Open {
            file,
            loadfile_flags,
        } => open(file.as_deref(), loadfile_flags),
    }
}

fn main() {
    let command = command_line::parse_arguments(env::args_os().skip(1))
        .unwrap_or_else(|error| exit_with_error(&error));
    if let Err(error) = run(command) {
        exit_with_error(&error);
    }
}
