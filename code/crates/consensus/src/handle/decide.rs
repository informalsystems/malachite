use crate::prelude::*;

pub async fn decide<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    consensus_round: Round,
    proposal: Ctx::Proposal,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let height = proposal.height();
    let proposal_round = proposal.round();
    let value = proposal.value();

    // Restore the commits. Note that they will be removed from `state`
    let commits = state.restore_precommits(height, proposal_round, value);

    // Clean proposals and values
    state.remove_full_proposals(height);

    #[cfg(feature = "debug")]
    {
        for trace in state.driver.round_state.get_traces() {
            debug!("Consensus trace: {trace}");
        }
    }

    perform!(
        co,
        Effect::Decide {
            height,
            round: proposal_round,
            value: value.clone(),
            commits
        }
    );

    metrics.consensus_end();

    // Reinitialize to remove any previous round or equivocating precommits.
    // TODO: Revise when evidence module is added.
    state.signed_precommits.clear();

    metrics.block_end();
    metrics.finalized_blocks.inc();

    metrics
        .consensus_round
        .observe(consensus_round.as_i64() as f64);

    metrics
        .proposal_round
        .observe(proposal_round.as_i64() as f64);

    Ok(())
}