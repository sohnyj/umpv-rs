#![windows_subsystem = "windows"]

use std::env;
use std::fmt;
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

fn show_message(text: &str) {
    let text_wide = encode_wide(text);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text_wide.as_ptr(), w!("umpv"), 0);
    }
}

fn show_information(text: &dyn fmt::Display) {
    show_message(&format!("Info\n{text}"));
}

fn error_exit(error: &dyn fmt::Display) -> ! {
    show_message(&format!("Error\n{error}"));
    process::exit(1);
}

enum Mode {
    Register,
    Unregister,
}

enum CommandLineOption {
    Mode(Mode),
    Loadfile(String),
}

const LOADFILE_OPTION_PREFIX: &str = "--loadfile=";
const SUPPORTED_LOADFILE_FLAGS: &[&str] = &[
    "replace",
    "append",
    "append+play",
    "insert-next",
    "insert-next+play",
];
const DEFAULT_LOADFILE_FLAGS: &str = SUPPORTED_LOADFILE_FLAGS[0];

fn parse_option(option: &str) -> Option<CommandLineOption> {
    match option {
        "--register" => Some(CommandLineOption::Mode(Mode::Register)),
        "--unregister" => Some(CommandLineOption::Mode(Mode::Unregister)),
        _ => option
            .strip_prefix(LOADFILE_OPTION_PREFIX)
            .map(|loadfile_flags| CommandLineOption::Loadfile(loadfile_flags.to_owned())),
    }
}

fn supported_loadfile_flags(given: &str) -> Option<&'static str> {
    SUPPORTED_LOADFILE_FLAGS
        .iter()
        .copied()
        .find(|supported| *supported == given)
}

enum ArgumentError {
    UnknownOption(String),
    UnsupportedLoadfileFlags(String),
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOption(option) => write!(formatter, "Unknown option: {option}"),
            Self::UnsupportedLoadfileFlags(loadfile_flags) => {
                write!(formatter, "Unsupported loadfile flags: {loadfile_flags}")
            }
        }
    }
}

enum Command {
    Register {
        loadfile_flags: &'static str,
    },
    Unregister,
    Open {
        files: Vec<String>,
        loadfile_flags: &'static str,
    },
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, ArgumentError> {
    let mut mode = None;
    let mut given_loadfile_flags = None;
    let mut files = Vec::new();
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with("--") {
            files.push(argument);
        } else if argument == "--" {
            past_end_of_options = true;
        } else {
            match parse_option(&argument) {
                Some(CommandLineOption::Mode(parsed)) => mode = mode.or(Some(parsed)),
                Some(CommandLineOption::Loadfile(parsed)) => {
                    given_loadfile_flags = given_loadfile_flags.or(Some(parsed));
                }
                None => return Err(ArgumentError::UnknownOption(argument)),
            }
        }
    }

    let loadfile_flags = match given_loadfile_flags {
        Some(given) => match supported_loadfile_flags(&given) {
            Some(supported) => supported,
            None => return Err(ArgumentError::UnsupportedLoadfileFlags(given)),
        },
        None => DEFAULT_LOADFILE_FLAGS,
    };

    Ok(match mode {
        Some(Mode::Register) => Command::Register { loadfile_flags },
        Some(Mode::Unregister) => Command::Unregister,
        None => Command::Open {
            files,
            loadfile_flags,
        },
    })
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

fn umpv_path() -> PathBuf {
    let Ok(path) = env::current_exe() else {
        error_exit(&"Failed to locate umpv.exe.");
    };
    path
}

fn mpv_path() -> PathBuf {
    umpv_path().with_file_name("mpv.exe")
}

/// `%L` is the shell's placeholder for the selected file.
fn shell_open_command(loadfile_flags: &str) -> String {
    format!(
        "\"{}\" {LOADFILE_OPTION_PREFIX}{loadfile_flags} -- \"%L\"",
        umpv_path().display()
    )
}

fn register(loadfile_flags: &str) {
    match registry::register(&shell_open_command(loadfile_flags)) {
        Ok(extension_count) => show_information(&format!(
            "Registered for {extension_count} file extension(s).\nloadfile: {loadfile_flags}"
        )),
        Err(error) => error_exit(&error),
    }
}

fn unregister() {
    match registry::unregister() {
        registry::Unregistered::Nothing => show_information(&"Nothing to unregister."),
        registry::Unregistered::ProgIdOnly => {
            show_information(&"Removed the umpv ProgID.\nNo file extensions were pointing at umpv.")
        }
        registry::Unregistered::ExtensionsRestored(extension_count) => show_information(&format!(
            "Unregistered for {extension_count} file extension(s)."
        )),
    }
}

fn launch_mpv(pipe: &pipe::Pipe, file: &str) {
    if let Err(error) = mpv::launch(&mpv_path(), pipe, file) {
        error_exit(&error);
    }
}

/// `None` when there is no window to activate: a newly launched mpv raises its
/// own, and an unidentified running instance cannot be located.
fn open_in_mpv(pipe: &pipe::Pipe, file: &str, loadfile_flags: &str) -> Option<u32> {
    let _lock_guard = match lock::acquire() {
        Ok(guard) => guard,
        Err(error) => error_exit(&error),
    };

    match pipe.send_loadfile(file, loadfile_flags) {
        Ok(pipe::SendOutcome::Sent { server_pid }) => server_pid,
        Ok(pipe::SendOutcome::NoServer) => {
            launch_mpv(pipe, file);
            None
        }
        Err(error) => error_exit(&error),
    }
}

fn open(files: &[String], loadfile_flags: &str) {
    let Some(file) = files.first() else {
        return;
    };
    if has_url_scheme(file) {
        error_exit(&"URLs are not supported.\nOnly local files can be opened.");
    }
    let file = absolute_file_path(file);

    let pipe = match pipe::Pipe::for_current_session() {
        Ok(pipe) => pipe,
        Err(error) => error_exit(&error),
    };

    if let Some(mpv_pid) = open_in_mpv(&pipe, &file, loadfile_flags) {
        mpv::activate_window(mpv_pid);
    }
}

fn main() {
    match parse_arguments(env::args().skip(1)) {
        Ok(Command::Register { loadfile_flags }) => register(loadfile_flags),
        Ok(Command::Unregister) => unregister(),
        Ok(Command::Open {
            files,
            loadfile_flags,
        }) => open(&files, loadfile_flags),
        Err(error) => error_exit(&error),
    }
}
