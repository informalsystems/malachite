use core::{fmt, str};

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use malachitebft_proto;

pub type MessageHash = Hash;
pub type BlockHash = Hash;

impl malachitebft_core_types::Value for BlockHash {
    type Id = BlockHash;

    fn id(&self) -> Self::Id {
        *self
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hash([u8; 32]);

impl Hash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }
}

impl PartialOrd for Hash {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl malachitebft_proto::Protobuf for Hash {
    type Proto = crate::proto::Hash;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_proto(proto: Self::Proto) -> Result<Self, malachitebft_proto::Error> {
        Ok(Self::new(proto.elements.as_ref().try_into().map_err(
            |_| malachitebft_proto::Error::Other("Invalid hash length".to_string()),
        )?))
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn to_proto(&self) -> Result<Self::Proto, malachitebft_proto::Error> {
        Ok(crate::proto::Hash {
            elements: Bytes::copy_from_slice(self.as_bytes().as_ref()),
        })
    }
}

impl fmt::Display for Hash {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Debug for Hash {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl str::FromStr for Hash {
    type Err = Box<dyn core::error::Error>;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hash: [u8; 32] = hex::decode(s)?[0..32].try_into()?;
        Ok(Self(hash))
    }
}
