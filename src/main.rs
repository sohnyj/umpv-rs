#![windows_subsystem = "windows"]

use std::env;
use std::path::PathBuf;
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

fn show_message(heading: &str, text: &str) {
    let text_wide = encode_wide(&format!("{heading}\n{text}"));
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

fn parse_command(option: &str) -> Option<Command> {
    match option {
        "--register" => Some(Command::Register),
        "--unregister" => Some(Command::Unregister),
        _ => None,
    }
}

fn find_command(options: &[String]) -> Option<Command> {
    options.iter().find_map(|option| parse_command(option))
}

fn is_known_option(option: &str) -> bool {
    parse_command(option).is_some() || option.starts_with(LOADFILE_OPTION_PREFIX)
}

fn find_unknown_option(options: &[String]) -> Option<&str> {
    options
        .iter()
        .map(String::as_str)
        .find(|option| !is_known_option(option))
}

const OPTION_PREFIX: &str = "--";
const END_OF_OPTIONS: &str = "--";

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
        } else if argument == END_OF_OPTIONS {
            past_end_of_options = true;
        } else {
            options.push(argument);
        }
    }

    Arguments { options, files }
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

fn absolute_file_path(file: &str) -> String {
    match std::path::absolute(file) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => error_exit(&format!("Failed to make the file path absolute: {error}")),
    }
}

fn first_file(files: &[String]) -> Option<&str> {
    files
        .iter()
        .map(String::as_str)
        .find(|file| !file.is_empty())
}

const LOADFILE_OPTION_PREFIX: &str = "--loadfile=";
const DEFAULT_LOADFILE_FLAGS: &str = "replace";

fn find_loadfile_flags(options: &[String]) -> &str {
    options
        .iter()
        .find_map(|option| option.strip_prefix(LOADFILE_OPTION_PREFIX))
        .unwrap_or(DEFAULT_LOADFILE_FLAGS)
}

fn is_supported_loadfile_flags(loadfile_flags: &str) -> bool {
    matches!(
        loadfile_flags,
        "replace" | "append" | "append+play" | "insert-next" | "insert-next+play"
    )
}

fn umpv_path() -> PathBuf {
    let Ok(path) = env::current_exe() else {
        error_exit("Failed to locate umpv.exe.");
    };
    path
}

fn register(loadfile_flags: &str) {
    let command = format!(
        "\"{}\" {LOADFILE_OPTION_PREFIX}{loadfile_flags} -- \"%L\"",
        umpv_path().display()
    );

    match registry::register(&command) {
        Ok(count) => show_information(&format!(
            "Registered for {count} file extension(s).\nloadfile: {loadfile_flags}"
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

fn send_or_launch(file: &str, loadfile_flags: &str) -> Result<Option<u32>, PlayError> {
    let _lock_guard = lock::acquire().map_err(PlayError::Lock)?;

    match pipe::send_file(file, loadfile_flags) {
        Ok(pid) => Ok(pid),
        Err(pipe::Error::NoServer) => {
            let mpv_path = umpv_path().with_file_name("mpv.exe");
            mpv::launch(&mpv_path, file)
                .map(|()| None)
                .map_err(PlayError::Mpv)
        }
        Err(pipe::Error::ConnectFailed) => Err(PlayError::ConnectFailed),
        Err(pipe::Error::WriteFailed) => Err(PlayError::WriteFailed),
    }
}

fn play(files: &[String], loadfile_flags: &str) {
    let Some(file) = first_file(files) else {
        return;
    };
    if has_url_scheme(file) {
        error_exit("URLs are not supported.\nOnly local files can be opened.");
    }
    let file = absolute_file_path(file);

    match send_or_launch(&file, loadfile_flags) {
        Ok(Some(pid)) => mpv::activate_window(pid),
        Ok(None) => {}
        Err(PlayError::Lock(lock::Error::CreateFailed)) => {
            error_exit("Failed to create umpv lock.")
        }
        Err(PlayError::Lock(lock::Error::WaitFailed)) => {
            error_exit("Failed to wait for umpv lock.")
        }
        Err(PlayError::Lock(lock::Error::TimedOut)) => {
            error_exit("Timed out waiting for umpv lock.\nAnother umpv instance is holding it.")
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
    if let Some(option) = find_unknown_option(&arguments.options) {
        error_exit(&format!("Unknown option: {option}"));
    }
    let loadfile_flags = find_loadfile_flags(&arguments.options);
    if !is_supported_loadfile_flags(loadfile_flags) {
        error_exit(&format!("Unsupported loadfile flags: {loadfile_flags}"));
    }

    match find_command(&arguments.options) {
        Some(Command::Register) => register(loadfile_flags),
        Some(Command::Unregister) => unregister(),
        None => play(&arguments.files, loadfile_flags),
    }
}
