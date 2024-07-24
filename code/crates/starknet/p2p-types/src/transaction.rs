use core::fmt;

use malachite_proto as proto;
use starknet_p2p_proto as p2p_proto;

use crate::Hash;

/// Transaction
#[derive(Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Transaction(Vec<u8>);

impl Transaction {
    /// Create a new transaction from bytes
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Get bytes from a transaction
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Get bytes from a transaction
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Size of this transaction in bytes
    pub fn size_bytes(&self) -> usize {
        self.0.len()
    }

    /// Compute the hash this transaction
    ///
    /// TODO: Use hash function from Context
    pub fn hash(&self) -> Hash {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(self.as_bytes());
        Hash::new(hasher.finalize().into())
    }
}

impl fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Transaction({}, {} bytes)",
            self.hash(),
            self.size_bytes()
        )
    }
}

impl proto::Protobuf for Transaction {
    type Proto = p2p_proto::Transaction;

    fn from_proto(proto: Self::Proto) -> Result<Self, proto::Error> {
        use starknet_p2p_proto::transaction::Txn;

        let txn = proto
            .txn
            .ok_or_else(|| proto::Error::missing_field::<Self::Proto>("txn"))?;

        match txn {
            Txn::Dummy(dummy) => Ok(Self::new(dummy.bytes)),
            _ => Ok(Self::new(vec![])),
        }
    }

    fn to_proto(&self) -> Result<Self::Proto, proto::Error> {
        use starknet_p2p_proto::transaction::{Dummy, Txn};

        Ok(Self::Proto {
            txn: Some(Txn::Dummy(Dummy {
                bytes: self.to_bytes(),
            })),
        })
    }
}

/// Transaction batch (used by mempool and block part)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transactions(Vec<Transaction>);

impl Transactions {
    /// Create a new transaction batch
    pub fn new(txes: Vec<Transaction>) -> Self {
        Transactions(txes)
    }

    /// Add a transaction to the batch
    pub fn push(&mut self, tx: Transaction) {
        self.0.push(tx);
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

impl proto::Protobuf for Transactions {
    type Proto = p2p_proto::Transactions;

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
        Ok(p2p_proto::Transactions {
            transactions: self
                .as_slice()
                .iter()
                .map(Transaction::to_proto)
                .collect::<Result<_, _>>()?,
        })
    }
}
