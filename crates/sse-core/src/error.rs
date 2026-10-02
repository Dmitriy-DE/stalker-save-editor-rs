use std::fmt;

/// What went wrong, in the terms the command line and the interface report it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input is not what the caller said it is, or it is damaged.
    Damaged(String),
    /// The operation is understood but refused: unsupported format, unproven write, unsafe request.
    Refused(String),
    /// The file system or the operating system failed.
    System(String),
}

/// Result of every fallible operation in the workspace.
pub type Result<T> = std::result::Result<T, Error>;

/// Process exit codes; the same numbers the C# command line uses, so scripts keep working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    /// Done.
    Done = 0,
    /// Wrong arguments.
    Usage = 2,
    /// Refused: unsupported or unsafe.
    Refused = 3,
    /// Unreadable or damaged input.
    Damaged = 4,
    /// File or system error.
    System = 5,
}

impl Error {
    /// A damaged-input error with the place it was found at.
    pub fn damaged(what: impl Into<String>) -> Self {
        Self::Damaged(what.into())
    }

    /// The exit code a command line returns for this error.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Damaged(_) => ExitCode::Damaged,
            Self::Refused(_) => ExitCode::Refused,
            Self::System(_) => ExitCode::System,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged(text) | Self::Refused(text) | Self::System(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::System(error.to_string())
    }
}
