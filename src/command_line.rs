use std::fmt;
use std::path::Path;
use std::str::FromStr;

enum Mode {
    Register,
    Unregister,
}

const LOADFILE_OPTION_PREFIX: &str = "--loadfile=";

#[derive(Clone, Copy, Default)]
pub(crate) enum LoadfileFlags {
    #[default]
    Replace,
    Append,
    AppendPlay,
    InsertNext,
    InsertNextPlay,
}

impl LoadfileFlags {
    const ALL: &[Self] = &[
        Self::Replace,
        Self::Append,
        Self::AppendPlay,
        Self::InsertNext,
        Self::InsertNextPlay,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
            Self::AppendPlay => "append+play",
            Self::InsertNext => "insert-next",
            Self::InsertNextPlay => "insert-next+play",
        }
    }
}

impl fmt::Display for LoadfileFlags {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for LoadfileFlags {
    type Err = ArgumentError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|flags| flags.as_str() == text)
            .ok_or_else(|| ArgumentError::UnsupportedLoadfileFlags(text.to_owned()))
    }
}

pub(crate) enum ArgumentError {
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

pub(crate) enum Command {
    Register {
        loadfile_flags: LoadfileFlags,
    },
    Unregister,
    Open {
        /// The shell passes one file per invocation; extra files are ignored.
        file: Option<String>,
        loadfile_flags: LoadfileFlags,
    },
}

pub(crate) fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Command, ArgumentError> {
    let mut mode = None;
    let mut loadfile_flags = None;
    let mut file = None;
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with("--") {
            file = file.or(Some(argument));
            continue;
        }
        match argument.as_str() {
            "--" => past_end_of_options = true,
            "--register" => mode = mode.or(Some(Mode::Register)),
            "--unregister" => mode = mode.or(Some(Mode::Unregister)),
            option => match option.strip_prefix(LOADFILE_OPTION_PREFIX) {
                Some(text) => loadfile_flags = loadfile_flags.or(Some(text.parse()?)),
                None => return Err(ArgumentError::UnknownOption(argument)),
            },
        }
    }

    let loadfile_flags = loadfile_flags.unwrap_or_default();

    Ok(match mode {
        Some(Mode::Register) => Command::Register { loadfile_flags },
        Some(Mode::Unregister) => Command::Unregister,
        None => Command::Open {
            file,
            loadfile_flags,
        },
    })
}

/// `%L` is the shell's placeholder for the selected file.
pub(crate) fn shell_open_command(umpv_path: &Path, loadfile_flags: LoadfileFlags) -> String {
    format!(
        "\"{}\" {LOADFILE_OPTION_PREFIX}{loadfile_flags} -- \"%L\"",
        umpv_path.display()
    )
}
