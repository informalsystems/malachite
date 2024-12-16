use core::fmt;
use malachite_proto::{Error as ProtoError, Protobuf};
use serde::{Deserialize, Serialize};

use crate::proto;

/// A blockchain height
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Height(u64);

impl Height {
    pub const fn new(height: u64) -> Self {
        Self(height)
    }

    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    pub fn increment(&self) -> Self {
        Self(self.0 + 1)
    }

    pub fn decrement(&self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }
}

impl Default for Height {
    fn default() -> Self {
        Height(1)
    }
}

impl fmt::Display for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Height({})", self.0)
    }
}

impl malachite_common::Height for Height {
    fn increment_by(&self, n: u64) -> Self {
        Self(self.0 + n)
    }

    fn decrement_by(&self, n: u64) -> Option<Self> {
        Some(Self(self.0.saturating_sub(n)))
    }

    fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Protobuf for Height {
    type Proto = proto::Height;

    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        Ok(Self(proto.value))
    }

    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        Ok(proto::Height { value: self.0 })
    }
}
