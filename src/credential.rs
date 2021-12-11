use std::{env, ffi::OsStr, fs, io, path::PathBuf};

/// Credential loader for units.
///
/// Credentials are read by systemd on unit startup and exported by their ID.
///
/// **Note**: only the user associated with the unit and the superuser may access credentials.
///
/// More documentation: <https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Credentials>
#[derive(Clone, Debug)]
pub struct CredentialLoader {
    dir: PathBuf,
}

impl CredentialLoader {
    /// Attempt to initiate a loader, returning [`None`] if no credentials are available.
    pub fn new() -> Option<Self> {
        let dir: PathBuf = env::var_os("CREDENTIALS_DIRECTORY")?.into();

        if dir.is_dir() {
            Some(Self { dir })
        } else {
            None
        }
    }

    /// Get a credential by its ID.
    ///
    /// # Examples
    /// ```no_run
    /// use libsystemd::CredentialLoader;
    ///
    /// if let Some(loader) = CredentialLoader::new() {
    ///     let key = "token";
    ///     match loader.get("token") {
    ///         Ok(val) => println!("{}: {}", key, String::from_utf8_lossy(&val)),
    ///         Err(e) => println!("couldn't retreive {}: {}", key, e),
    ///     }
    /// }
    /// ```
    pub fn get<K: AsRef<OsStr>>(&self, id: K) -> io::Result<Vec<u8>> {
        self._get(id.as_ref())
    }

    fn _get(&self, id: &OsStr) -> io::Result<Vec<u8>> {
        let path: PathBuf = [self.dir.as_ref(), id].iter().collect();

        fs::read(path)
    }

    /// An iterator over this units credentials.
    ///
    /// # Examples
    /// ```no_run
    /// use libsystemd::CredentialLoader;
    ///
    /// if let Some(loader) = CredentialLoader::new() {
    ///     for entry in loader.iter() {
    ///         match entry {
    ///             Ok(entry) => {
    ///                 let key = entry.file_name();
    ///                 println!("{:?}: {}", key, String::from_utf8_lossy(&loader.get(&key)?))
    ///             }
    ///             Err(e) => println!("couldn't list some credential: {}", e),
    ///         }
    ///     }
    /// }
    /// # Ok::<(), std::io::Error>(())
    pub fn iter(&self) -> fs::ReadDir {
        self.dir.read_dir().expect("path exists")
    }
}
