use bytes::Bytes;
use bytesize::ByteSize;
use rand::RngCore;
use sha3::Digest;
use tokio::time::Instant;
use tracing::trace;

use malachite_common::Round;

use crate::actor::HostParams;
use crate::types::*;

pub async fn build_proposal_parts(
    height: Height,
    round: Round,
    params: &HostParams,
    app_txes: Vec<Transaction>,
) -> Result<(BlockHash, Vec<ProposalPart>), Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();

    let mut sequence = 0;
    let mut block_size = 0;
    let mut block_tx_count = 0;
    let mut block_hasher = sha3::Keccak256::new();

    let mut parts = vec![];

    // Init
    {
        let part = ProposalPart::Init(ProposalInit {
            block_number: height,
            fork_id: 1, // TODO: Add fork id
            proposal_round: round,
            proposer: params.address.clone(),
        });

        block_hasher.update(part.to_sign_bytes());
        parts.push(part);
        sequence += 1;
    }

    trace!(%height, %round, %sequence, "Building local value");

    let max_block_size = params.max_block_size.as_u64() as usize;

    let mut txes = Vec::new();
    let mut tx_count = 0;

    for tx in app_txes {
        if block_size + tx.size_bytes() > max_block_size {
            break;
        }

        block_size += tx.size_bytes();
        tx_count += 1;

        txes.push(tx);
    }

    block_tx_count += tx_count;

    trace!(
        %sequence,
        "Created a tx batch with {tx_count} tx-es of size {} in {:?}",
        ByteSize::b(block_size as u64),
        start.elapsed()
    );

    // Transactions
    {
        let part = ProposalPart::Transactions(Transactions::new(txes));

        block_hasher.update(part.to_sign_bytes());
        parts.push(part);
        sequence += 1;
    }

    // BlockProof
    {
        // TODO: Compute actual "proof"
        let mut rng = rand::rngs::OsRng;
        let mut proof = Vec::with_capacity(32);
        rng.fill_bytes(&mut proof);

        let part = ProposalPart::BlockProof(BlockProof::new(vec![Bytes::from(proof)]));

        block_hasher.update(part.to_sign_bytes());
        parts.push(part);
        sequence += 1;
    }

    // Fin
    {
        // TODO: Compute actual "valid_round"
        let part = ProposalPart::Fin(ProposalFin { valid_round: None });

        block_hasher.update(part.to_sign_bytes());
        parts.push(part);
        sequence += 1;
    }

    let block_hash = BlockHash::new(block_hasher.finalize().into());
    let block_size = ByteSize::b(block_size as u64);

    trace!(
        tx_count = %block_tx_count, size = %block_size, hash = %block_hash, parts = %sequence,
        "Built block in {:?}", start.elapsed()
    );

    Ok((block_hash, parts))
}
