//! Error types for various errors that may occur in spurs.

/// An error type representing things that could possibly go wrong when using an SshShell.
#[derive(Debug)]
pub enum SshError {
    /// Unable to find the private key at the given path.
    KeyNotFound { file: String },

    /// SSH authentication failed.
    AuthFailed { key: std::path::PathBuf },

    /// The comand run over SSH returned with a non-zero exit code.
    NonZeroExit { cmd: String, exit: i32 },

    /// An SSH error occurred.
    SshError { error: ssh2::Error },

    /// An I/O error occurred.
    IoError { error: std::io::Error },
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            SshError::KeyNotFound { file } => write!(f, "no such key: {}", file),
            SshError::AuthFailed { key } => {
                write!(f, "authentication failed with private key: {:?}", key)
            }
            SshError::NonZeroExit { cmd, exit } => {
                write!(f, "non-zero exit ({}) for command: {}", exit, cmd)
            }
            SshError::SshError { error } => write!(f, "{}", error),
            SshError::IoError { error } => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for SshError {}

impl std::convert::From<ssh2::Error> for SshError {
    fn from(error: ssh2::Error) -> Self {
        SshError::SshError { error }
    }
}

impl std::convert::From<std::io::Error> for SshError {
    fn from(error: std::io::Error) -> Self {
        SshError::IoError { error }
    }
}
