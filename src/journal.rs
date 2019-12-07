use std::fs::File;
use std::path::Path;

use sdjournal::journal::*;
use sdjournal::iter::EntryIter;

use crate::errors::*;

#[derive(Debug)]
pub struct SdJournal {
    inner: Journal<File>,
}

impl SdJournal {
    pub fn open_journal<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;

        let journal = Journal::new(file)?;

        let sdjournal = SdJournal {
            inner: journal,
        };

        Ok(sdjournal)
    }

    pub fn iter(&self) -> EntryIter<File> {
        self.inner.iter_entries()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdjournal_read_simple() {
        let sd = SdJournal::open_journal("./tests/user-1000.journal");
        assert!(sd.is_ok());
    }

    #[test]
    fn test_sdjournal_iter_simple() {
        let sd = SdJournal::open_journal("./tests/user-1000.journal").unwrap();

        let mut counter = 0;

        let iter = sd.iter();
        for _obj in iter {
            counter += 1;
            //eprintln!("obj: {}", obj.realtime);
        }

        // journalctl --header --file tests/user-1000.journal | grep "Entry Objects" == 645
        assert_eq!(counter, 645);
    }
}
