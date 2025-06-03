use bytes::Bytes;
use serde::{Deserialize, Serialize};

use malachitebft_proto::{Error as ProtoError, Protobuf};

use crate::types::{block::Block, context::MockContext};

use super::proto;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalData {
    pub block: Block,
}

impl ProposalData {
    pub fn new(block: Block) -> Self {
        Self { block }
    }

    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<u64>()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PartType {
    Data,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalPart {
    Data(ProposalData),
}

impl ProposalPart {
    pub fn part_type(&self) -> PartType {
        match self {
            Self::Data(_) => PartType::Data,
        }
    }

    pub fn get_type(&self) -> &'static str {
        match self {
            Self::Data(_) => "data",
        }
    }

    pub fn as_data(&self) -> Option<&ProposalData> {
        match self {
            Self::Data(data) => Some(data),
        }
    }

    pub fn tx_count(&self) -> usize {
        match self {
            Self::Data(data) => data.block.transactions.len(),
        }
    }

    pub fn to_sign_bytes(&self) -> Bytes {
        Protobuf::to_bytes(self).unwrap()
    }

    pub fn size_bytes(&self) -> usize {
        self.to_sign_bytes().len() // TODO: Do this more efficiently
    }
}

impl Protobuf for ProposalPart {
    type Proto = proto::ProposalPart;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        use proto::proposal_part::Part;

        let part = proto
            .part
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("part"))?;

        match part {
            Part::Data(data) => Ok(Self::Data(ProposalData {
                block: Block::from_proto(
                    data.block
                        .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("block"))?,
                )?,
            })),
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        use proto::proposal_part::Part;

        match self {
            Self::Data(data) => Ok(Self::Proto {
                part: Some(Part::Data(proto::ProposalData {
                    block: Some(data.block.to_proto()?),
                })),
            }),
        }
    }
}

impl malachitebft_core_types::ProposalPart<MockContext> for ProposalPart {
    fn is_first(&self) -> bool {
        true
    }

    fn is_last(&self) -> bool {
        true
    }
}
