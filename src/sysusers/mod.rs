//! Helpers for working with `sysusers.d` configuration files.
//!
//! For the complete documentation see
//! <https://www.freedesktop.org/software/systemd/man/sysusers.d.html>.

use crate::errors::SdError;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::convert::TryFrom;
use std::path::PathBuf;

mod format;
mod serialization;

/// Single entry in `sysusers.d` configuration format.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SysusersEntry {
    AddRange(AddRange),
    AddUserToGroup(AddUserToGroup),
    CreateGroup(CreateGroup),
    CreateUserAndGroup(CreateUserAndGroup),
}

/// Sysusers entry of type `r`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "serialization::SysusersData")]
pub struct AddRange {
    pub(crate) from: u32,
    pub(crate) to: u32,
}

impl AddRange {
    /// Create a new `AddRange` entry.
    pub fn new(from: u32, to: u32) -> Result<Self, SdError> {
        Ok(Self { from, to })
    }
}

/// Sysusers entry of type `m`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "serialization::SysusersData")]
pub struct AddUserToGroup {
    pub(crate) username: String,
    pub(crate) groupname: String,
}

impl AddUserToGroup {
    /// Create a new `AddUserToGroup` entry.
    pub fn new(username: String, groupname: String) -> Result<Self, SdError> {
        validate_name_strict(&username)?;
        validate_name_strict(&groupname)?;
        Ok(Self {
            username,
            groupname,
        })
    }
}

/// Sysusers entry of type `g`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "serialization::SysusersData")]
pub struct CreateGroup {
    pub(crate) groupname: String,
    pub(crate) gid: GidOrPath,
}

impl CreateGroup {
    /// Create a new `CreateGroup` entry.
    pub fn new(groupname: String) -> Result<Self, SdError> {
        Self::impl_new(groupname, GidOrPath::Automatic)
    }

    /// Create a new `CreateGroup` entry, using a numeric ID.
    pub fn new_with_gid(groupname: String, gid: u32) -> Result<Self, SdError> {
        Self::impl_new(groupname, GidOrPath::Gid(gid))
    }

    /// Create a new `CreateGroup` entry, using a filepath reference.
    pub fn new_with_path(groupname: String, path: PathBuf) -> Result<Self, SdError> {
        Self::impl_new(groupname, GidOrPath::Path(path))
    }

    pub(crate) fn impl_new(groupname: String, gid: GidOrPath) -> Result<Self, SdError> {
        validate_name_strict(&groupname)?;
        Ok(Self { groupname, gid })
    }
}

/// Sysusers entry of type `u`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "serialization::SysusersData")]
pub struct CreateUserAndGroup {
    pub(crate) name: String,
    pub(crate) id: IdOrPath,
    pub(crate) gecos: String,
    pub(crate) home_dir: Option<PathBuf>,
    pub(crate) shell: Option<PathBuf>,
}

impl CreateUserAndGroup {
    /// Create a new `CreateUserAndGroup` entry, using a filepath reference.
    pub fn new(
        name: String,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
    ) -> Result<Self, SdError> {
        Self::impl_new(name, gecos, home_dir, shell, IdOrPath::Automatic)
    }

    /// Create a new `CreateUserAndrGroup` entry, using a numeric ID.
    pub fn new_with_id(
        name: String,
        id: u32,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
    ) -> Result<Self, SdError> {
        Self::impl_new(name, gecos, home_dir, shell, IdOrPath::Id(id))
    }

    /// Create a new `CreateUserAndGroup` entry, using a UID and a GID.
    pub fn new_with_uid_gid(
        name: String,
        uid: u32,
        gid: u32,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
    ) -> Result<Self, SdError> {
        Self::impl_new(name, gecos, home_dir, shell, IdOrPath::UidGid((uid, gid)))
    }

    /// Create a new `CreateUserAndGroup` entry, using a UID and a groupname.
    pub fn new_with_uid_groupname(
        name: String,
        uid: u32,
        groupname: String,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
    ) -> Result<Self, SdError> {
        validate_name_strict(&groupname)?;
        Self::impl_new(
            name,
            gecos,
            home_dir,
            shell,
            IdOrPath::UidGroupname((uid, groupname)),
        )
    }

    /// Create a new `CreateUserAndGroup` entry, using a filepath reference.
    pub fn new_with_path(
        name: String,
        path: PathBuf,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
    ) -> Result<Self, SdError> {
        Self::impl_new(name, gecos, home_dir, shell, IdOrPath::Path(path))
    }

    pub(crate) fn impl_new(
        name: String,
        gecos: String,
        home_dir: Option<PathBuf>,
        shell: Option<PathBuf>,
        id: IdOrPath,
    ) -> Result<Self, SdError> {
        validate_name_strict(&name)?;
        Ok(Self {
            name,
            id,
            gecos,
            home_dir,
            shell,
        })
    }
}

/// ID entity for `CreateUserAndGroup`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum IdOrPath {
    Id(u32),
    UidGid((u32, u32)),
    UidGroupname((u32, String)),
    Path(PathBuf),
    Automatic,
}

impl TryFrom<&str> for IdOrPath {
    type Error = SdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value == "-" {
            return Ok(IdOrPath::Automatic);
        }
        if value.starts_with('/') {
            return Ok(IdOrPath::Path(value.into()));
        }
        if let Ok(single_id) = value.parse() {
            return Ok(IdOrPath::Id(single_id));
        }
        let tokens: Vec<_> = value.split(':').filter(|s| !s.is_empty()).collect();
        if tokens.len() == 2 {
            let uid: u32 = tokens[0].parse().map_err(|_| "invalid user id")?;
            let id = match tokens[1].parse() {
                Ok(gid) => IdOrPath::UidGid((uid, gid)),
                _ => {
                    let groupname = tokens[1].to_string();
                    validate_name_strict(&groupname)?;
                    IdOrPath::UidGroupname((uid, groupname))
                }
            };
            return Ok(id);
        }

        Err(format!("unexpected user ID '{}'", value).into())
    }
}

/// ID entity for `CreateGroup`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GidOrPath {
    Gid(u32),
    Path(PathBuf),
    Automatic,
}

impl TryFrom<&str> for GidOrPath {
    type Error = SdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value == "-" {
            return Ok(GidOrPath::Automatic);
        }
        if value.starts_with('/') {
            return Ok(GidOrPath::Path(value.into()));
        }
        if let Ok(parsed_gid) = value.parse() {
            return Ok(GidOrPath::Gid(parsed_gid));
        }

        Err(format!("unexpected group ID '{}'", value).into())
    }
}

/// Validate a sysusers name in strict mode.
pub fn validate_name_strict(input: &str) -> Result<(), SdError> {
    if input.is_empty() {
        return Err(SdError::from("empty name"));
    }

    if input.len() > 31 {
        let err_msg = format!(
            "overlong sysusers name '{}' (more than 31 characters)",
            input
        );
        return Err(SdError::from(err_msg));
    }

    for (index, ch) in input.char_indices() {
        if index == 0 {
            if !(ch.is_ascii_alphabetic() || ch == '_') {
                let err_msg = format!(
                    "invalid starting character '{}' in sysusers name '{}'",
                    ch, input
                );
                return Err(SdError::from(err_msg));
            }
        } else if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            let err_msg = format!("invalid character '{}' in sysusers name '{}'", ch, input);
            return Err(SdError::from(err_msg));
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_validate_name_strict() {
        let err_cases = vec!["-foo", "10bar", "42"];
        for entry in err_cases {
            validate_name_strict(entry).unwrap_err();
        }

        let ok_cases = vec!["_authd", "httpd"];
        for entry in ok_cases {
            validate_name_strict(entry).unwrap();
        }
    }
}
