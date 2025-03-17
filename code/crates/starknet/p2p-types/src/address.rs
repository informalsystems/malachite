use bytes::Bytes;
use core::fmt;
use serde::{Deserialize, Serialize};
use starknet_api::core::{ContractAddress, PatriciaKey};

use malachitebft_proto::{Error as ProtoError, Protobuf};
use malachitebft_starknet_p2p_proto as p2p_proto;

use crate::Felt;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Address(ContractAddress);

impl Address {
    pub fn new(address: ContractAddress) -> Self {
        Self(address)
    }
}

impl From<u64> for Address {
    fn from(address: u64) -> Self {
        Self(ContractAddress::from(address))
    }
}

impl fmt::Display for Address {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for Address {
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.0)
    }
}

impl malachitebft_core_types::Address for Address {}

impl Protobuf for Address {
    type Proto = p2p_proto::Address;

    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        let mut felt = [0; 32];
        if proto.elements.len() != 32 {
            return Err(ProtoError::invalid_data::<Self::Proto>("elements"));
        }

        felt.copy_from_slice(&proto.elements);

        let hash = Felt::from_bytes_be(&felt);
        if let Ok(stark_felt) = PatriciaKey::try_from(hash) {
            Ok(Self(ContractAddress(stark_felt)))
        } else {
            Err(ProtoError::invalid_data::<Self::Proto>("elements"))
        }
    }

    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        Ok(p2p_proto::Address {
            elements: Bytes::from(self.0.key().to_bytes_be().to_vec()),
        })
    }
}
