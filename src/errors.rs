use std::ffi::OsString;
use std::num::ParseIntError;
use thiserror::Error;

/// Library errors.
#[derive(Error, Debug)]
#[error("libsystemd error: {0}")]
pub struct SdError(pub(crate) String);

impl From<&str> for SdError {
    fn from(arg: &str) -> Self {
        Self(arg.to_string())
    }
}

impl From<String> for SdError {
    fn from(arg: String) -> Self {
        Self(arg)
    }
}

impl From<JournalStreamEnvError> for SdError {
    fn from(error: JournalStreamEnvError) -> Self {
        Self(format!("JournalStreamEnvError: {}", error))
    }
}

impl From<ParseJournalStreamError> for SdError {
    fn from(error: ParseJournalStreamError) -> Self {
        Self(format!("ParseJournalStreamError: {}", error))
    }
}

/// An error while parsing a journal stream
#[derive(Error, Debug)]
pub enum ParseJournalStreamError {
    #[error("Value was not UTF-8 encoded: {0:?}")]
    ValueNotUtf8Encoded(OsString),
    #[error("Missing separator : between inode and device number in value: {0}")]
    MissingSeparator(String),
    #[error("Failed to parse device number: {0}")]
    FailedToParseDeviceNumber(ParseIntError),
    #[error("Failed to parse inode number: {0}")]
    FailedToParseInodeNumber(ParseIntError),
}

#[derive(Error, Debug)]
pub enum JournalStreamEnvError {
    #[error("Failed to parse contents of environment variable {0:?}: {1}")]
    ParseError(OsString, ParseJournalStreamError),
    #[error("Variable {0:?} was not set")]
    EnvironmentVariableUnset(OsString),
}
