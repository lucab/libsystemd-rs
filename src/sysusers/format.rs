use crate::sysusers::{
    AddRange, AddUserToGroup, CreateGroup, CreateUserAndGroup, GidOrPath, IdOrPath, SysusersEntry,
};
use std::borrow::Cow;
use std::fmt::{self, Display};
use std::path::Path;

impl Display for SysusersEntry {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::AddRange(ref v) => write!(f, "{v}"),
            Self::AddUserToGroup(ref v) => write!(f, "{v}"),
            Self::CreateGroup(ref v) => write!(f, "{v}"),
            Self::CreateUserAndGroup(ref v) => write!(f, "{v}"),
        }
    }
}

impl Display for AddRange {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r - {}-{} - - -", self.from, self.to)
    }
}

impl Display for AddUserToGroup {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "m {} {} - - -", self.username, self.groupname,)
    }
}

impl Display for CreateGroup {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "g {} {} - - -", self.groupname, self.gid,)
    }
}

impl Display for CreateUserAndGroup {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "u {} {} \"{}\" {} {}",
            self.name,
            self.id,
            self.gecos,
            self.home_dir
                .as_deref()
                .map_or(Cow::Borrowed("-"), Path::to_string_lossy),
            self.shell
                .as_deref()
                .map_or(Cow::Borrowed("-"), Path::to_string_lossy),
        )
    }
}

impl Display for IdOrPath {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Id(i) => write!(f, "{i}"),
            Self::UidGid((u, g)) => write!(f, "{u}:{g}"),
            Self::UidGroupname((u, ref g)) => write!(f, "{u}:{g}"),
            Self::Path(ref p) => write!(f, "{}", p.display()),
            Self::Automatic => write!(f, "-",),
        }
    }
}

impl Display for GidOrPath {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Gid(g) => write!(f, "{g}"),
            Self::Path(ref p) => write!(f, "{}", p.display()),
            Self::Automatic => write!(f, "-",),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::sysusers::{AddRange, AddUserToGroup, CreateGroup, CreateUserAndGroup};

    #[test]
    fn test_formatters() {
        {
            let type_u =
                CreateUserAndGroup::new("foo0".to_string(), "test".to_string(), None, None)
                    .unwrap();
            let expected = r#"u foo0 - "test" - -"#;
            assert_eq!(type_u.to_string(), expected);
        }
        {
            let type_g = CreateGroup::new("foo1".to_string()).unwrap();
            let expected = r#"g foo1 - - - -"#;
            assert_eq!(type_g.to_string(), expected);
        }
        {
            let type_r = AddRange::new(10, 20).unwrap();
            let expected = r#"r - 10-20 - - -"#;
            assert_eq!(type_r.to_string(), expected);
        }
        {
            let type_m = AddUserToGroup::new("foo3".to_string(), "bar".to_string()).unwrap();
            let expected = r#"m foo3 bar - - -"#;
            assert_eq!(type_m.to_string(), expected);
        }
    }
}
