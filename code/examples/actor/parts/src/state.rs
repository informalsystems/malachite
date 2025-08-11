use bytes::Bytes;
use malachitebft_core_consensus::Role;
use mempool::{RawTx, TxHash};
use sha3::Digest;
use std::mem::size_of;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::types::block::Block;
use crate::types::hash::Hash;
use crate::types::signing::Ed25519Provider;
use mempool::CheckTxOutcome as MempoolCheckTxOutcome;
use rand::RngCore;
use tracing::{debug, error, trace};

use crate::streaming::PartStreamsMap;
use malachitebft_core_types::{Round, Validity};
use malachitebft_engine::consensus::ConsensusRef;
use malachitebft_engine::host::ProposedValue;
use malachitebft_engine::util::streaming::StreamId;

use crate::mock_host::MockHost;
use crate::store::{BlockStore, StoreError};
use crate::types::address::Address;
use crate::types::context::MockContext;
use crate::types::height::Height;
use crate::types::proposal_part::ProposalPart;
use crate::types::transaction::{Transaction, TransactionBatch};
use crate::types::value::Value;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub enum CheckTxOutcome {
    Success(Hash),
    Error(Hash, String),
}

impl MempoolCheckTxOutcome for CheckTxOutcome {
    fn is_valid(&self) -> bool {
        matches!(self, CheckTxOutcome::Success(_))
    }

    fn hash(&self) -> TxHash {
        match self {
            CheckTxOutcome::Success(hash) => TxHash(Bytes::from(hash.to_vec())),
            CheckTxOutcome::Error(hash, _) => TxHash(Bytes::from(hash.to_vec())),
        }
    }
}

pub struct HostState {
    pub ctx: MockContext,
    pub signing_provider: Ed25519Provider,
    pub height: Height,
    pub round: Round,
    pub proposer: Option<Address>,
    pub role: Role,
    pub host: MockHost,
    pub consensus: Option<ConsensusRef<MockContext>>,
    pub part_streams_map: PartStreamsMap,
    pub block_store: BlockStore,
    pub nonce: u64,
}

impl HostState {
    pub async fn new<R>(
        ctx: MockContext,
        signing_provider: Ed25519Provider,
        host: MockHost,
        db_path: impl AsRef<Path>,
        rng: &mut R,
    ) -> Self
    where
        R: RngCore,
    {
        Self {
            ctx,
            signing_provider,
            height: Height::new(1),
            round: Round::Nil,
            proposer: None,
            role: Role::None,
            host,
            consensus: None,
            part_streams_map: PartStreamsMap::new(),
            block_store: BlockStore::new(db_path).await.unwrap(),
            nonce: rng.next_u64(),
        }
    }

    pub fn check_tx(&self, tx: &RawTx) -> AppResult<Box<dyn MempoolCheckTxOutcome>> {
        // Create transaction to compute hash, then create TxHash consistently with removal logic
        let transaction = Transaction::new(tx.0.clone());
        // Use the same hash format as removal: TxHash from hash bytes
        let tx_hash = transaction.hash().clone();
        Ok(Box::new(CheckTxOutcome::Success(tx_hash)))
    }

    pub fn stream_id(&mut self) -> StreamId {
        let mut bytes = Vec::with_capacity(size_of::<u64>() + size_of::<u32>());
        bytes.extend_from_slice(&self.height.as_u64().to_be_bytes());
        bytes.extend_from_slice(&self.round.as_u32().unwrap().to_be_bytes());
        StreamId::new(bytes.into())
    }

    #[allow(clippy::type_complexity)]
    #[tracing::instrument(skip_all, fields(%height, %round))]
    pub async fn build_proposal_from_parts(
        &self,
        height: Height,
        round: Round,
        parts: &[Arc<ProposalPart>],
    ) -> Result<ProposedValue<MockContext>, StoreError> {
        // We must be here with non-empty `parts`, must have init, fin, commitment and maybe transactions
        assert!(!parts.is_empty(), "Parts must not be empty");

        let init = parts
            .iter()
            .find_map(|part| part.as_init())
            .expect("Init part not found");

        let fin = parts
            .iter()
            .find_map(|part| part.as_fin())
            .expect("Fin part not found");

        // Collect all transactions from the transaction parts
        // We expect that the transaction parts are ordered by sequence number but we don't have a way to check
        // this here, so we just collect them in the order.
        let transactions: Vec<Transaction> = parts
            .iter()
            .filter_map(|part| part.as_data())
            .flat_map(|data| data.transactions.as_slice().iter().cloned())
            .collect();

        // Determine the validity of the proposal
        let validity = self.verify_proposal_validity(&fin.commitment, transactions.clone());

        let block = Block {
            height: init.height,
            block_hash: fin.commitment.clone(),
            transactions: TransactionBatch::new(transactions.clone()),
        };

        self.block_store
            .store_undecided_block(init.height, init.round, block)
            .await?;

        let pol_round = init.valid_round;
        if pol_round.is_defined() {
            debug!("Reassembling a proposal we might have seen before: {init:?}");
        }

        trace!(parts.len = %parts.len(), "Building proposal content from parts");

        Ok(ProposedValue {
            proposer: init.proposer,
            height,
            round,
            valid_round: pol_round,
            value: Value::new(fin.commitment.clone()),
            validity,
        })
    }

    pub fn verify_proposal_validity(
        &self,
        hash: &Hash,
        transactions: Vec<Transaction>,
    ) -> Validity {
        let mut hasher = sha3::Keccak256::new();

        for tx in transactions.iter() {
            hasher.update(tx.hash().as_bytes());
        }

        let transaction_commitment = Hash::new(hasher.finalize().into());

        let valid_proposal = transaction_commitment == *hash;

        if valid_proposal {
            Validity::Valid
        } else {
            error!(
                "ProposalCommitment hash mismatch: {:?} != {:?}",
                transaction_commitment, hash
            );
            Validity::Invalid
        }
    }

    #[tracing::instrument(skip_all, fields(
        part.height = %height,
        part.round = %round,
        part.message = ?part.get_type(),
    ))]
    pub async fn build_value_from_part(
        &mut self,
        stream_id: &StreamId,
        height: Height,
        round: Round,
        part: ProposalPart,
    ) -> Option<ProposedValue<MockContext>> {
        self.host
            .part_store
            .store(stream_id, height, round, part.clone());

        if let ProposalPart::Data(data) = &part {
            if self.host.params.exec_time_per_tx > Duration::from_secs(0) {
                debug!("Simulating tx execution and proof verification");

                // Simulate Tx execution. In the real implementation the results of the execution would be
                // accumulated in some intermediate state structure based on which the proposal commitment
                // will be computed once all parts are received and checked against the received
                // `ProposalCommitment` part (e.g. `state_diff_commitment`) and the `proposal_commitment_hash`
                // in the `Fin` part.
                let num_txes = data.transactions.len() as u32;
                let exec_time = self.host.params.exec_time_per_tx * num_txes;
                tokio::time::sleep(exec_time).await;

                trace!("Simulation took {exec_time:?} to execute {num_txes} txes");
            }
        }

        let parts = self
            .host
            .part_store
            .all_parts_by_stream_id(stream_id.clone(), height, round);

        trace!(
            count = self.host.part_store.blocks_count(),
            "Blocks for which we have parts"
        );

        // TODO: Do more validations, e.g. there is no higher tx proposal part,
        // check that we have received the proof, etc.
        let Some(_fin) = parts.iter().find_map(|part| part.as_fin()) else {
            debug!("Proposal part has not been received yet: Fin");
            return None;
        };

        let block_size: usize = parts.iter().map(|p| p.size_bytes()).sum();
        let tx_count: usize = parts.iter().map(|p| p.tx_count()).sum();

        debug!(
            tx.count = %tx_count, block.size = %block_size, parts.count = %parts.len(),
            "Building proposal content from parts"
        );

        match self.build_proposal_from_parts(height, round, &parts).await {
            Ok(proposed_value) => {
                self.host.part_store.store_value_id(
                    stream_id,
                    height,
                    round,
                    proposed_value.value.id(),
                );
                Some(proposed_value)
            }
            Err(e) => {
                error!("Failed to build proposal from parts: {}", e);
                None
            }
        }
    }
}
