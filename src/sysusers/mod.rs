//! Helpers for working with `sysusers.d` configuration files.
//!
//! For the complete documentation see
//! <https://www.freedesktop.org/software/systemd/man/sysusers.d.html>.

use crate::errors::SdError;
use std::borrow::Cow;
use std::path::PathBuf;

mod format;

/// Single entry in `sysusers.d` configuration format.
#[derive(Clone, Debug, PartialEq)]
pub enum SysusersEntry {
    AddRange(AddRange),
    AddUserToGroup(AddUserToGroup),
    CreateGroup(CreateGroup),
    CreateUserAndGroup(CreateUserAndGroup),
}

/// Sysusers entry of type `r`.
#[derive(Clone, Debug, PartialEq)]
pub struct AddRange {
    from: u32,
    to: u32,
}

impl AddRange {
    /// Create a new `AddRange` entry.
    pub fn new(from: u32, to: u32) -> Result<Self, SdError> {
        Ok(Self { from, to })
    }
}

/// Sysusers entry of type `m`.
#[derive(Clone, Debug, PartialEq)]
pub struct AddUserToGroup {
    username: String,
    groupname: String,
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
#[derive(Clone, Debug, PartialEq)]
pub struct CreateGroup {
    groupname: String,
    gid: GidOrPath,
}

impl CreateGroup {
    /// Create a new `CreateGroup` entry.
    pub fn new(groupname: String) -> Result<Self, SdError> {
        validate_name_strict(&groupname)?;
        Ok(Self {
            groupname,
            gid: GidOrPath::Automatic,
        })
    }

    /// Create a new `CreateGroup` entry, using a numeric ID.
    pub fn new_with_gid(groupname: String, gid: u32) -> Result<Self, SdError> {
        validate_name_strict(&groupname)?;
        Ok(Self {
            groupname,
            gid: GidOrPath::Gid(gid),
        })
    }

    /// Create a new `CreateGroup` entry, using a filepath reference.
    pub fn new_with_path(groupname: String, path: PathBuf) -> Result<Self, SdError> {
        validate_name_strict(&groupname)?;
        Ok(Self {
            groupname,
            gid: GidOrPath::Path(path),
        })
    }
}

/// Sysusers entry of type `u`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateUserAndGroup {
    name: String,
    id: IdOrPath,
    gecos: String,
    home_dir: Option<PathBuf>,
    shell: Option<PathBuf>,
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

    fn impl_new(
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

/// ID entity for `CreateGroup`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GidOrPath {
    Gid(u32),
    Path(PathBuf),
    Automatic,
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
