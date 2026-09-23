#![windows_subsystem = "windows"]

mod command_line;
mod lock;
mod mpv;
mod pipe;
mod registry;

use std::env;
use std::fmt;
use std::iter;
use std::path::{self, Path, PathBuf};
use std::process;
use std::ptr;

use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
use windows_sys::core::w;

use crate::command_line::{Command, LoadfileFlags};

fn encode_wide(string: &str) -> Vec<u16> {
    string.encode_utf16().chain(iter::once(0)).collect()
}

fn show_message(text: &str) {
    let text_wide = encode_wide(text);
    unsafe {
        MessageBoxW(ptr::null_mut(), text_wide.as_ptr(), w!("umpv"), 0);
    }
}

fn error_exit(error: &dyn fmt::Display) -> ! {
    show_message(&error.to_string());
    process::exit(1);
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

fn make_absolute(path: &str) -> String {
    path::absolute(path)
        .unwrap_or_else(|error| {
            error_exit(&format!("Failed to make the file path absolute: {error}"))
        })
        .to_string_lossy()
        .into_owned()
}

fn umpv_path() -> PathBuf {
    env::current_exe().unwrap_or_else(|_| error_exit(&"Failed to locate umpv.exe."))
}

fn mpv_path() -> PathBuf {
    umpv_path().with_file_name("mpv.exe")
}

fn register(loadfile_flags: LoadfileFlags) {
    match registry::register(&command_line::shell_open_command(
        &umpv_path(),
        loadfile_flags,
    )) {
        Ok(extension_count) => show_message(&format!(
            "Registered for {extension_count} file extension(s).\nloadfile: {loadfile_flags}"
        )),
        Err(error) => error_exit(&error),
    }
}

fn unregister() {
    match registry::unregister() {
        Ok(registry::Unregistered::Nothing) => show_message("Nothing to unregister."),
        Ok(registry::Unregistered::ProgIdOnly) => {
            show_message("Removed the umpv ProgID.\nNo file extensions were pointing at umpv.");
        }
        Ok(registry::Unregistered::Extensions(extension_count)) => show_message(&format!(
            "Unregistered for {extension_count} file extension(s)."
        )),
        Err(error) => error_exit(&error),
    }
}

enum OpenError {
    Lock(lock::Error),
    Pipe(pipe::Error),
    Mpv(mpv::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lock(error) => fmt::Display::fmt(error, formatter),
            Self::Pipe(error) => fmt::Display::fmt(error, formatter),
            Self::Mpv(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

/// `None` when nothing needs raising: a new mpv raises its own window, and an
/// unidentified instance cannot be found.
fn open_in_mpv(
    pipe: &pipe::Pipe,
    mpv_path: &Path,
    file: &str,
    loadfile_flags: LoadfileFlags,
) -> Result<Option<u32>, OpenError> {
    // Showing an error here would hold the lock until its message box is closed.
    let _lock_guard = lock::acquire().map_err(OpenError::Lock)?;

    match pipe.send_loadfile(file, loadfile_flags) {
        Ok(pipe::SendOutcome::Sent { server_pid }) => Ok(server_pid),
        Ok(pipe::SendOutcome::NoServer) => {
            mpv::launch(mpv_path, pipe, file).map_err(OpenError::Mpv)?;
            Ok(None)
        }
        Err(error) => Err(OpenError::Pipe(error)),
    }
}

fn open(file: Option<&str>, loadfile_flags: LoadfileFlags) {
    let Some(file) = file else {
        return;
    };
    if has_url_scheme(file) {
        error_exit(&"URLs are not supported.\nOnly local files can be opened.");
    }
    let file = make_absolute(file);

    let pipe = pipe::Pipe::for_current_session().unwrap_or_else(|error| error_exit(&error));

    match open_in_mpv(&pipe, &mpv_path(), &file, loadfile_flags) {
        Ok(Some(mpv_pid)) => mpv::activate_window(mpv_pid),
        Ok(None) => {}
        Err(error) => error_exit(&error),
    }
}

fn main() {
    match command_line::parse_arguments(env::args().skip(1)) {
        Ok(Command::Register { loadfile_flags }) => register(loadfile_flags),
        Ok(Command::Unregister) => unregister(),
        Ok(Command::Open {
            file,
            loadfile_flags,
        }) => open(file.as_deref(), loadfile_flags),
        Err(error) => error_exit(&error),
    }
}
