use crate::errors::SdError;
use std::{convert::TryFrom, fs, str::FromStr};
use uuid::Uuid;

/// A 128-bits ID.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Id128(Uuid);

impl Id128 {
    /// Returns the ID as a byte array.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    #[deprecated(since = "0.3.2", note = "use TryFrom")]
    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, SdError> {
        Id128::try_from(bytes)
    }

    #[deprecated(since = "0.3.2", note = "use parse")]
    pub fn parse_str<S>(input: S) -> Result<Self, SdError>
    where
        S: AsRef<str>,
    {
        Id128::from_str(input.as_ref())
    }

    /// Hash this ID with an application-specific ID.
    pub fn app_specific(&self, app: &Self) -> Result<Self, SdError> {
        use hmac::{Hmac, Mac, NewMac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(self.as_bytes())
            .map_err(|_| "failed to prepare HMAC")?;
        mac.update(app.as_bytes());
        let mut hashed = mac.finalize().into_bytes();

        if hashed.len() != 32 {
            return Err("short hash".into());
        };

        // Set version to 4.
        hashed[6] = (hashed[6] & 0x0F) | 0x40;
        // Set variant to DCE.
        hashed[8] = (hashed[8] & 0x3F) | 0x80;

        Self::try_from(&hashed[..16])
    }

    /// Return the unique boot ID.
    pub fn from_boot() -> Result<Self, SdError> {
        let buf = fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(|e| format!("failed to open boot_id: {}", e))?;
        Id128::from_str(buf.trim_end())
    }

    /// Return this machine unique ID.
    pub fn from_machine() -> Result<Self, SdError> {
        let buf = fs::read_to_string("/etc/machine-id")
            .map_err(|e| format!("failed to open machine-id: {}", e))?;
        Id128::from_str(buf.trim_end())
    }

    /// Return this ID as a lowercase hexadecimal string, without dashes.
    pub fn lower_hex(&self) -> String {
        self.0.to_simple_ref().to_string()
    }

    /// Return this ID as a lowercase hexadecimal string, with dashes.
    pub fn dashed_hex(&self) -> String {
        self.0.to_hyphenated_ref().to_string()
    }
}

impl AsRef<[u8]> for Id128 {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<[u8; 16]> for Id128 {
    fn from(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }
}

impl FromStr for Id128 {
    type Err = SdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(Self(input.parse().map_err(|e| {
            format!("failed to parse ID from string: {}", e)
        })?))
    }
}

impl TryFrom<&[u8]> for Id128 {
    type Error = SdError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::from_slice(bytes).map_err(|e| {
            format!("failed to parse ID from bytes slice: {}", e)
        })?))
    }
}

// TODO: (in 0.4): make optional behind serde feature
impl serde_crate::Serialize for Id128 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde_crate::Serializer,
    {
        if serializer.is_human_readable() {
            self.lower_hex().serialize(serializer)
        } else {
            self.as_bytes().serialize(serializer)
        }
    }
}

impl<'de> serde_crate::Deserialize<'de> for Id128 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde_crate::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            Ok(Self(Uuid::deserialize(deserializer)?))
        } else {
            Ok(Self(uuid::adapter::compact::deserialize(deserializer)?))
        }
    }
}

#[deprecated(since = "0.3.2", note = "use Id128::from_machine()")]
pub fn get_machine() -> Result<Id128, SdError> {
    Id128::from_machine()
}

/// Return this machine unique ID, hashed with an application-specific ID.
#[deprecated(since = "0.3.2", note = "use Id128::from_machine()?.app_specific()")]
pub fn get_machine_app_specific(app_id: &Id128) -> Result<Id128, SdError> {
    Id128::from_machine()?.app_specific(app_id)
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_test::{assert_tokens, Configure, Token};

    #[test]
    fn basic_parse_str() {
        let input = "2e074e9b299c41a59923c51ae16f279b";
        let id = input.parse::<Id128>().unwrap();
        assert_eq!(id.lower_hex(), input);

        assert!("".parse::<Id128>().is_err());
    }

    #[test]
    fn basic_keyed_hash() {
        let machine_id = "2e074e9b299c41a59923c51ae16f279b".parse::<Id128>().unwrap();
        let app_id = "033b1b9b264441fcaa173e9e5bf35c5a".parse().unwrap();

        let hashed_id = "4d4a86c9c6644a479560ded5d19a30c5".parse().unwrap();

        let output = machine_id.app_specific(&app_id).unwrap();
        assert_eq!(output, hashed_id);
    }

    #[test]
    fn basic_from_slice() {
        let input_str = "d86a4e9e4dca45c5bcd9846409bfa1ae";
        let array = [
            0xd8, 0x6a, 0x4e, 0x9e, 0x4d, 0xca, 0x45, 0xc5, 0xbc, 0xd9, 0x84, 0x64, 0x09, 0xbf,
            0xa1, 0xae,
        ];
        let slice: &[u8] = &[
            0xd8, 0x6a, 0x4e, 0x9e, 0x4d, 0xca, 0x45, 0xc5, 0xbc, 0xd9, 0x84, 0x64, 0x09, 0xbf,
            0xa1, 0xae,
        ];

        let id = Id128::from(array);
        assert_eq!(input_str, id.lower_hex());
        let id = Id128::try_from(slice).unwrap();
        assert_eq!(input_str, id.lower_hex());

        Id128::try_from([].as_ref()).unwrap_err();
    }

    #[test]
    fn basic_debug() {
        let input = "0b37f793-aeb9-4d67-99e1-6e678d86781f";
        let id = input.parse::<Id128>().unwrap();
        assert_eq!(id.dashed_hex(), input);
    }

    #[test]
    fn test_ser_de() {
        let id: Id128 = "1071334a-9324-4511-adcc-b8d8b70eb1c2".parse().unwrap();

        assert_tokens(
            &id.readable(),
            &[Token::Str("1071334a93244511adccb8d8b70eb1c2")],
        );

        assert_tokens(
            &id.compact(),
            &[
                Token::Tuple { len: 16 },
                Token::U8(16),
                Token::U8(113),
                Token::U8(51),
                Token::U8(74),
                Token::U8(147),
                Token::U8(36),
                Token::U8(69),
                Token::U8(17),
                Token::U8(173),
                Token::U8(204),
                Token::U8(184),
                Token::U8(216),
                Token::U8(183),
                Token::U8(14),
                Token::U8(177),
                Token::U8(194),
                Token::TupleEnd,
            ],
        );
    }
}
