#![allow(unused_variables, unused_imports)]

use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use bytesize::ByteSize;
use eyre::eyre;
use ractor::{async_trait, Actor, ActorProcessingErr, SpawnErr};
use sha3::Digest;
use tokio::time::Instant;
use tracing::{debug, error, trace};

use malachite_actors::consensus::ConsensusMsg;
use malachite_actors::gossip_consensus::{GossipConsensusMsg, GossipConsensusRef};
use malachite_actors::host::{LocallyProposedValue, ProposedValue};
use malachite_actors::util::streaming::{StreamContent, StreamId, StreamMessage};
use malachite_common::{Extension, Round, Validity};
use malachite_metrics::Metrics;

use crate::actor::HostState;
use crate::build_proposal::build_proposal_parts;
use crate::context::AbciContext;
use crate::part_store::PartStore;
use crate::streaming::PartStreamsMap;
use crate::types::{Address, BlockHash, Height, Proposal, ProposalPart, ValidatorSet};

#[tracing::instrument(skip_all, fields(%height, %round))]
pub fn build_value_from_parts(
    parts: &[Arc<ProposalPart>],
    height: Height,
    round: Round,
) -> Option<ProposedValue<AbciContext>> {
    let (value, validator_address, validity) =
        build_proposal_content_from_parts(parts, height, round)?;

    Some(ProposedValue {
        validator_address,
        height,
        round,
        value,
        validity,
        extension: None,
    })
}

#[tracing::instrument(skip_all, fields(%height, %round))]
pub fn build_proposal_content_from_parts(
    parts: &[Arc<ProposalPart>],
    height: Height,
    round: Round,
) -> Option<(BlockHash, Address, Validity)> {
    if parts.is_empty() {
        return None;
    }

    let Some(init) = parts.iter().find_map(|part| part.as_init()) else {
        error!("No Init part found in the proposal parts");
        return None;
    };

    let Some(fin) = parts.iter().find_map(|part| part.as_fin()) else {
        error!("No Fin part found in the proposal parts");
        return None;
    };

    trace!(parts.len = %parts.len(), "Building proposal content from parts");

    let block_hash = {
        let mut block_hasher = sha3::Keccak256::new();
        for part in parts {
            block_hasher.update(part.to_sign_bytes());
        }
        BlockHash::new(block_hasher.finalize().into())
    };

    trace!(%block_hash, "Computed block hash");

    // TODO: How to compute validity?
    let validity = Validity::Valid;

    Some((block_hash, init.proposer.clone(), validity))
}

#[tracing::instrument(skip_all, fields(
        part.height = %height,
        part.round = %round,
        part.message = ?part.part_type(),
    ))]
pub async fn build_value_from_part(
    state: &mut HostState,
    height: Height,
    round: Round,
    part: ProposalPart,
) -> Option<ProposedValue<AbciContext>> {
    state.part_store.store(height, round, part.clone());
    let all_parts = state.part_store.all_parts(height, round);

    // TODO: Do more validations, e.g. there is no higher tx proposal part,
    //       check that we have received the proof, etc.
    let Some(fin) = all_parts.iter().find_map(|part| part.as_fin()) else {
        debug!("Final proposal part has not been received yet");
        return None;
    };

    let block_size: usize = all_parts.iter().map(|p| p.size_bytes()).sum();
    let tx_count: usize = all_parts.iter().map(|p| p.tx_count()).sum();

    debug!(
        %tx_count, %block_size, num_parts = %all_parts.len(),
        "All parts have been received already, building value"
    );

    build_value_from_parts(&all_parts, height, round)
}
