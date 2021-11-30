use thiserror::Error;

/// Library errors.
#[derive(Error, Debug)]
#[error("libsystemd error: {msg}")]
pub struct SdError {
    pub(crate) kind: ErrorKind,
    pub(crate) msg: String,
}

impl From<&str> for SdError {
    fn from(arg: &str) -> Self {
        Self {
            kind: ErrorKind::Generic,
            msg: arg.to_string(),
        }
    }
}

impl From<String> for SdError {
    fn from(arg: String) -> Self {
        Self {
            kind: ErrorKind::Generic,
            msg: arg,
        }
    }
}

/// Markers for recoverable error kinds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Generic,
    SysusersUnknownType,
}
