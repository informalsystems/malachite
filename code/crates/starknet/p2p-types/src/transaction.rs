use bytes::Bytes;
use malachitebft_proto::{self as proto};
use malachitebft_starknet_p2p_proto as p2p_proto;

use crate::Hash;

/// Transaction
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction(Box<p2p_proto::ConsensusTransaction>);

impl Transaction {
    /// Create a new transaction from a protobuf message
    pub fn new(tx: p2p_proto::ConsensusTransaction) -> Self {
        Self(Box::new(tx))
    }

    /// Crate a new transaction from a bytes
    pub fn dummy(bytes: impl Into<Bytes>) -> Self {
        Self::new(p2p_proto::ConsensusTransaction {
            txn: Some(p2p_proto::consensus_transaction::Txn::Dummy(bytes.into())),
            transaction_hash: None,
        })
    }

    /// Compute the size of this transaction in bytes
    pub fn size_bytes(&self) -> usize {
        prost::Message::encoded_len(&self.0)
    }

    /// Compute the hash of this transaction
    pub fn hash(&self) -> Hash {
        Self::compute_hash(&prost::Message::encode_to_vec(&self.0))
    }

    /// Compute the hash of a transaction
    ///
    /// TODO: Use hash function from Context
    pub fn compute_hash(bytes: &[u8]) -> Hash {
        use sha3::Digest;
        let mut hasher = sha3::Keccak256::new();
        hasher.update(bytes);
        Hash::new(hasher.finalize().into())
    }
}

impl proto::Protobuf for Transaction {
    type Proto = p2p_proto::ConsensusTransaction;

    fn from_proto(proto: Self::Proto) -> Result<Self, proto::Error> {
        Ok(Self(Box::new(proto)))
    }

    fn to_proto(&self) -> Result<Self::Proto, proto::Error> {
        Ok(*self.0.clone())
    }
}

impl Eq for Transaction {}

/// Transaction batch (used by mempool and proposal part)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionBatch(Vec<Transaction>);

impl TransactionBatch {
    /// Create a new transaction batch
    pub fn new(txes: Vec<Transaction>) -> Self {
        TransactionBatch(txes)
    }

    /// Add a transaction to the batch
    pub fn push(&mut self, tx: Transaction) {
        self.0.push(tx);
    }

    /// Add a set of transaction to the batch
    pub fn append(&mut self, txes: TransactionBatch) {
        let mut txes1 = txes.clone();
        self.0.append(&mut txes1.0);
    }

    /// Get the number of transactions in the batch
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether or not the batch is empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get transactions from a batch
    pub fn into_vec(self) -> Vec<Transaction> {
        self.0
    }

    /// Get transactions from a batch
    pub fn to_vec(&self) -> Vec<Transaction> {
        self.0.to_vec()
    }

    /// Get transactions from a batch
    pub fn as_slice(&self) -> &[Transaction] {
        &self.0
    }

    /// The size of this batch in bytes
    pub fn size_bytes(&self) -> usize {
        self.as_slice()
            .iter()
            .map(|tx| tx.size_bytes())
            .sum::<usize>()
    }
}

impl proto::Protobuf for TransactionBatch {
    type Proto = p2p_proto::TransactionBatch;

    fn from_proto(proto: Self::Proto) -> Result<Self, proto::Error> {
        Ok(Self::new(
            proto
                .transactions
                .into_iter()
                .map(Transaction::from_proto)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    fn to_proto(&self) -> Result<Self::Proto, proto::Error> {
        Ok(p2p_proto::TransactionBatch {
            transactions: self
                .as_slice()
                .iter()
                .map(Transaction::to_proto)
                .collect::<Result<_, _>>()?,
        })
    }
}
