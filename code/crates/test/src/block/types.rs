use crate::{hash::BlockHash, Height, transaction::Transactions};

use malachitebft_proto::{Error as ProtoError, Protobuf};

#[derive(Clone, Debug)]
pub struct Block {
    pub height: Height,
    pub transactions: Transactions,
    pub block_hash: BlockHash,
}

impl Protobuf for Block {
    type Proto = crate::proto::Block;

    fn from_proto(proto: Self::Proto) -> Result<Self, ProtoError> {
        let transactions = proto
            .transactions
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("transactions"))?;

        let block_hash = proto
            .block_hash
            .ok_or_else(|| ProtoError::missing_field::<Self::Proto>("block_hash"))?;

        Ok(Self {
            height: Height::new(proto.block_number),
            transactions: Transactions::from_proto(transactions)?,
            block_hash: BlockHash::from_proto(block_hash)?,
        })
    }

    fn to_proto(&self) -> Result<Self::Proto, ProtoError> {
        Ok(Self::Proto {
            block_number: self.height.as_u64(),
            transactions: Some(self.transactions.to_proto()?),
            block_hash: Some(self.block_hash.to_proto()?),
        })
    }
}
