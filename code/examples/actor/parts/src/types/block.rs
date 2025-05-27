use malachitebft_proto::{Error as ProtoError, Protobuf};
use prost::Message;

use super::proto;
use crate::types::{hash::BlockHash, height::Height, transaction::TransactionBatch};

#[derive(Clone, Debug)]
pub struct Block {
    pub height: Height,
    pub transactions: TransactionBatch,
    pub block_hash: BlockHash,
}

impl Block {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtoError> {
        let proto = proto::Block::decode(bytes)?;
        Self::from_proto(proto)
    }
}

impl Protobuf for Block {
    type Proto = proto::Block;

    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        let transactions = proto
            .transactions
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("transactions"))?;

        let block_hash = proto
            .block_hash
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("block_hash"))?;

        Ok(Self {
            height: Height::new(proto.height),
            transactions: TransactionBatch::from_proto(transactions)?,
            block_hash: BlockHash::from_bytes(&block_hash.elements)
                .map_err(ProtoError::invalid_data::<Self::Proto>)?,
        })
    }

    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        Ok(Self::Proto {
            height: self.height.to_proto()?,
            transactions: Some(self.transactions.to_proto()?),
            block_hash: Some(proto::Hash {
                elements: self.block_hash.to_vec().into(),
            }),
        })
    }
}
