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

fn error_exit(error: &dyn fmt::Display) -> ! {
    show_message(&error.to_string());
    process::exit(1);
}

enum Mode {
    Register,
    Unregister,
}

enum CommandLineOption {
    Mode(Mode),
    Loadfile(&'static str),
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

fn supported_loadfile_flags(given: &str) -> Result<&'static str, ArgumentError> {
    SUPPORTED_LOADFILE_FLAGS
        .iter()
        .copied()
        .find(|supported| *supported == given)
        .ok_or_else(|| ArgumentError::UnsupportedLoadfileFlags(given.to_owned()))
}

fn parse_option(option: &str) -> Result<CommandLineOption, ArgumentError> {
    match option {
        "--register" => Ok(CommandLineOption::Mode(Mode::Register)),
        "--unregister" => Ok(CommandLineOption::Mode(Mode::Unregister)),
        _ => match option.strip_prefix(LOADFILE_OPTION_PREFIX) {
            Some(given) => supported_loadfile_flags(given).map(CommandLineOption::Loadfile),
            None => Err(ArgumentError::UnknownOption(option.to_owned())),
        },
    }
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
        /// The shell passes one file per invocation; extra files are ignored.
        file: Option<String>,
        loadfile_flags: &'static str,
    },
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Command, ArgumentError> {
    let mut mode = None;
    let mut loadfile_flags = None;
    let mut file = None;
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with("--") {
            file = file.or(Some(argument));
        } else if argument == "--" {
            past_end_of_options = true;
        } else {
            match parse_option(&argument)? {
                CommandLineOption::Mode(parsed) => mode = mode.or(Some(parsed)),
                CommandLineOption::Loadfile(parsed) => {
                    loadfile_flags = loadfile_flags.or(Some(parsed));
                }
            }
        }
    }

    let loadfile_flags = loadfile_flags.unwrap_or(DEFAULT_LOADFILE_FLAGS);

    Ok(match mode {
        Some(Mode::Register) => Command::Register { loadfile_flags },
        Some(Mode::Unregister) => Command::Unregister,
        None => Command::Open {
            file,
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

fn make_absolute(path: &str) -> String {
    std::path::absolute(path)
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

/// `%L` is the shell's placeholder for the selected file.
fn shell_open_command(loadfile_flags: &str) -> String {
    format!(
        "\"{}\" {LOADFILE_OPTION_PREFIX}{loadfile_flags} -- \"%L\"",
        umpv_path().display()
    )
}

fn register(loadfile_flags: &str) {
    match registry::register(&shell_open_command(loadfile_flags)) {
        Ok(extension_count) => show_message(&format!(
            "Registered for {extension_count} file extension(s).\nloadfile: {loadfile_flags}"
        )),
        Err(error) => error_exit(&error),
    }
}

fn unregister() {
    match registry::unregister() {
        registry::Unregistered::Nothing => show_message("Nothing to unregister."),
        registry::Unregistered::ProgIdOnly => {
            show_message("Removed the umpv ProgID.\nNo file extensions were pointing at umpv.");
        }
        registry::Unregistered::ExtensionsRestored(extension_count) => show_message(&format!(
            "Unregistered for {extension_count} file extension(s)."
        )),
    }
}

/// `None` when nothing needs raising: a new mpv raises its own window, and an
/// unidentified instance cannot be found.
fn open_in_mpv(pipe: &pipe::Pipe, file: &str, loadfile_flags: &str) -> Option<u32> {
    let _lock_guard = lock::acquire().unwrap_or_else(|error| error_exit(&error));

    match pipe.send_loadfile(file, loadfile_flags) {
        Ok(pipe::SendOutcome::Sent { server_pid }) => server_pid,
        Ok(pipe::SendOutcome::NoServer) => {
            if let Err(error) = mpv::launch(&mpv_path(), pipe, file) {
                error_exit(&error);
            }
            None
        }
        Err(error) => error_exit(&error),
    }
}

fn open(file: Option<&str>, loadfile_flags: &str) {
    let Some(file) = file else {
        return;
    };
    if has_url_scheme(file) {
        error_exit(&"URLs are not supported.\nOnly local files can be opened.");
    }
    let file = make_absolute(file);

    let pipe = pipe::Pipe::for_current_session().unwrap_or_else(|error| error_exit(&error));

    if let Some(mpv_pid) = open_in_mpv(&pipe, &file, loadfile_flags) {
        mpv::activate_window(mpv_pid);
    }
}

fn main() {
    match parse_arguments(env::args().skip(1)) {
        Ok(Command::Register { loadfile_flags }) => register(loadfile_flags),
        Ok(Command::Unregister) => unregister(),
        Ok(Command::Open {
            file,
            loadfile_flags,
        }) => open(file.as_deref(), loadfile_flags),
        Err(error) => error_exit(&error),
    }
}
