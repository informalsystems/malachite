use malachitebft_core_consensus::Role;
use sha3::Digest;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::mem::size_of;

use rand::RngCore;
use tracing::{debug, trace};
use crate::types::signing::Ed25519Provider;

use malachitebft_core_types::{Round, Validity};
use malachitebft_engine::consensus::ConsensusRef;
use malachitebft_engine::host::ProposedValue;
use malachitebft_engine::util::streaming::StreamId;
use crate::streaming::PartStreamsMap;

use crate::mock_host::MockHost;
use crate::types::address::Address;
use crate::types::context::MockContext;
use crate::types::height::Height;
use crate::types::proposal_part::{ProposalPart, ProposalFin};
use crate::types::transaction::Transaction;
use crate::store::BlockStore;
use crate::types::value::Value;

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
    ) -> ProposedValue<MockContext> {
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
        let validity = self
            .verify_proposal_validity(fin, transactions)
            .await;

        let pol_round = init.valid_round;
        if pol_round.is_defined() {
            debug!("Reassembling a proposal we might have seen before: {init:?}");
        }

        trace!(parts.len = %parts.len(), "Building proposal content from parts");

        ProposedValue {
            proposer: init.proposer,
            height,
            round,
            valid_round: pol_round,
            value: Value::new(fin.commitment.clone()),
            validity,
        }
    }

    async fn verify_proposal_validity(
        &self,
        _fin: &ProposalFin,
        transactions: Vec<Transaction>,
    ) -> Validity {
        let mut hasher = sha3::Keccak256::new();

        for tx in transactions.iter() {
            hasher.update(tx.hash().as_bytes());
        }

        //let transaction_commitment = Hash::new(hasher.finalize().into());

        // TODO: Check validity of the proposal
        let valid_proposal = true;

        if valid_proposal {
            Validity::Valid
        } else {
            // error!(
            //     "ProposalCommitment hash mismatch: {} != {}",
            //     transaction_commitment, fin.proposal_commitment_hash
            // );
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
        stream_id: StreamId,
        height: Height,
        round: Round,
        part: ProposalPart,
    ) -> Option<ProposedValue<MockContext>> {
        self.host
            .part_store
            .store(&stream_id, height, round, part.clone());

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
            .all_parts_by_stream_id(stream_id, height, round);

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

        self.build_proposal_from_parts(height, round, &parts).await.into()
    }
}
