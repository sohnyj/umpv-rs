#![windows_subsystem = "windows"]

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
use windows_sys::core::w;

mod lock;
mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    string.encode_utf16().chain(std::iter::once(0)).collect()
}

fn show_message(prefix: &str, text: &str) {
    let text_wide = encode_wide(&format!("{prefix}\n{text}"));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text_wide.as_ptr(), w!("umpv"), 0);
    }
}

fn show_information(text: &str) {
    show_message("Info", text);
}

fn error_exit(text: &str) -> ! {
    show_message("Error", text);
    process::exit(1);
}

enum Command {
    Register,
    Unregister,
}

fn find_command(options: &[String]) -> Option<Command> {
    options.iter().find_map(|option| match option.as_str() {
        "--register" => Some(Command::Register),
        "--unregister" => Some(Command::Unregister),
        _ => None,
    })
}

const OPTION_PREFIX: &str = "--";

struct Arguments {
    options: Vec<String>,
    files: Vec<String>,
}

fn split_arguments(arguments: impl IntoIterator<Item = String>) -> Arguments {
    let mut options = Vec::new();
    let mut files = Vec::new();
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with(OPTION_PREFIX) {
            files.push(argument);
        } else if argument == OPTION_PREFIX {
            past_end_of_options = true;
        } else {
            options.push(argument);
        }
    }

    Arguments { options, files }
}

fn has_url_scheme(file: &str) -> bool {
    let Some((scheme, _)) = file.split_once("://") else {
        return false;
    };
    scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn resolve_file_path(file: &str) -> String {
    match std::path::absolute(file) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => file.to_string(),
    }
}

fn find_file(files: &[String]) -> Option<&String> {
    files.iter().find(|file| !file.is_empty())
}

const DEFAULT_LOADFILE: &str = "replace";

fn find_loadfile(options: &[String]) -> &str {
    options
        .iter()
        .find_map(|option| option.strip_prefix("--loadfile="))
        .unwrap_or(DEFAULT_LOADFILE)
}

fn is_supported_loadfile(loadfile: &str) -> bool {
    matches!(
        loadfile,
        "replace" | "append" | "append+play" | "insert-next" | "insert-next+play"
    )
}

fn umpv_path() -> PathBuf {
    let Ok(path) = env::current_exe() else {
        error_exit("Failed to locate umpv.exe.");
    };
    path
}

fn register(loadfile: &str) {
    let command = format!(
        "\"{}\" --loadfile={loadfile} -- \"%L\"",
        umpv_path().display()
    );

    match registry::register(&command) {
        Ok(count) => show_information(&format!(
            "Registered for {count} file extension(s).\nloadfile: {loadfile}"
        )),
        Err(registry::Error::NoAssociations) => {
            error_exit("No mpv file associations found.\nRun 'mpv.exe --register' first.")
        }
        Err(registry::Error::ProgIdWriteFailed) => {
            error_exit("Failed to write umpv ProgID to registry.")
        }
        Err(registry::Error::NoExtensionsRegistered) => {
            error_exit("Failed to register any file associations.")
        }
    }
}

fn unregister() {
    match registry::unregister() {
        0 => show_information("Nothing to unregister."),
        count => show_information(&format!("Unregistered for {count} file extension(s).")),
    }
}

enum PlayError {
    Lock(lock::Error),
    Mpv(mpv::Error),
    ConnectFailed,
    WriteFailed,
}

fn send_or_launch(mpv_path: &Path, file: &str, loadfile: &str) -> Result<Option<u32>, PlayError> {
    let _lock_guard = lock::acquire().map_err(PlayError::Lock)?;

    match pipe::send_file(file, loadfile) {
        Ok(pid) => Ok(pid),
        Err(pipe::Error::NotRunning) => mpv::launch(mpv_path, file)
            .map(|()| None)
            .map_err(PlayError::Mpv),
        Err(pipe::Error::ConnectFailed) => Err(PlayError::ConnectFailed),
        Err(pipe::Error::WriteFailed) => Err(PlayError::WriteFailed),
    }
}

fn play(files: &[String], loadfile: &str) {
    let Some(file) = find_file(files) else {
        return;
    };
    if has_url_scheme(file) {
        error_exit("URLs are not supported.\nOnly local files can be opened.");
    }
    let file = resolve_file_path(file);
    let mpv_path = umpv_path().with_file_name("mpv.exe");

    match send_or_launch(&mpv_path, &file, loadfile) {
        Ok(Some(pid)) => mpv::activate_window(pid),
        Ok(None) => {}
        Err(PlayError::Lock(lock::Error::CreateFailed)) => {
            error_exit("Failed to create umpv lock.")
        }
        Err(PlayError::Lock(lock::Error::WaitFailed)) => {
            error_exit("Failed to wait for the umpv lock.")
        }
        Err(PlayError::Lock(lock::Error::TimedOut)) => {
            error_exit("Failed to acquire lock: an mpv instance is not responding.")
        }
        Err(PlayError::Mpv(mpv::Error::SpawnFailed(error))) => {
            error_exit(&format!("Failed to launch mpv.exe: {error}"))
        }
        Err(PlayError::Mpv(mpv::Error::Exited)) => {
            error_exit("mpv.exe exited before it opened the file.")
        }
        Err(PlayError::Mpv(mpv::Error::StartupTimedOut)) => {
            error_exit("Timed out waiting for mpv.exe to start.")
        }
        Err(PlayError::ConnectFailed) => error_exit("Failed to connect to mpv."),
        Err(PlayError::WriteFailed) => error_exit("Failed to send the file to mpv."),
    }
}

fn main() {
    let arguments = split_arguments(env::args().skip(1));
    let loadfile = find_loadfile(&arguments.options);
    if !is_supported_loadfile(loadfile) {
        error_exit(&format!("Unsupported loadfile flag: {loadfile}"));
    }

    match find_command(&arguments.options) {
        Some(Command::Register) => register(loadfile),
        Some(Command::Unregister) => unregister(),
        None => play(&arguments.files, loadfile),
    }
}
