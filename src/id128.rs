use errors::*;
use std::fs;
use std::io::Read;
use uuid::Uuid;

/// A 128-bits ID.
#[derive(Debug, Eq, PartialEq)]
pub struct Id128 {
    uuid_v4: Uuid,
}

impl Id128 {
    /// Parse an `Id128` from string.
    pub fn parse_str<S>(input: S) -> Result<Self>
    where
        S: AsRef<str>,
    {
        let uuid_v4 = Uuid::parse_str(input.as_ref()).map_err(|_| "failed to parse from string")?;
        Ok(Self { uuid_v4 })
    }

    /// Hash this ID with an application-specific ID.
    pub fn app_specific(&self, app: &Self) -> Result<Self> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_varkey(self.uuid_v4.as_bytes())
            .map_err(|_| "failed to prepare HMAC")?;
        mac.input(app.uuid_v4.as_bytes());
        let mut hashed = mac.result().code();

        ensure!(hashed.len() == 32, "short hash");

        // Set version to 4.
        hashed[6] = (hashed[6] & 0x0F) | 0x40;
        // Set variant to DCE.
        hashed[8] = (hashed[8] & 0x3F) | 0x80;

        let uuid_v4 = Uuid::from_slice(&hashed[..16]).map_err(|_| "failed to parse bytes")?;

        Ok(Id128 { uuid_v4 })
    }
}

/// Return this machine unique ID.
pub fn get_machine() -> Result<Id128> {
    let mut buf = String::new();
    let mut fd = fs::File::open("/etc/machine-id")?;
    fd.read_to_string(&mut buf)?;
    Id128::parse_str(buf.trim_end())
}

/// Return this machine unique ID, hashed with an application-specific ID.
pub fn get_machine_app_specific(app_id: &Id128) -> Result<Id128> {
    let machine_id = get_machine()?;
    machine_id.app_specific(app_id)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic_parse_str() {
        let input = "2e074e9b299c41a59923c51ae16f279b";
        Id128::parse_str(input).unwrap();
    }

    #[test]
    fn basic_keyed_hash() {
        let input = "2e074e9b299c41a59923c51ae16f279b";
        let machine_id = Id128::parse_str(input).unwrap();

        let key = "033b1b9b264441fcaa173e9e5bf35c5a";
        let app_id = Id128::parse_str(key).unwrap();

        let expected = "4d4a86c9c6644a479560ded5d19a30c5";
        let hashed_id = Id128::parse_str(expected).unwrap();

        let output = machine_id.app_specific(&app_id).unwrap();
        assert_eq!(output, hashed_id);
    }
}
