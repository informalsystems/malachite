use crate::prelude::*;

use crate::handle::driver::apply_driver_input;
use crate::types::{ProposedValue, WalEntry};

use super::decide::try_decide;

/// Handles a proposed value that can originate from multiple sources:
/// 1. Application layer:
///    - In 'parts-only' mode
///    - In 'proposal-and-parts' mode
/// 2. WAL (Write-Ahead Log) during node restart recovery
/// 3. Sync service during state synchronization
///
/// This function processes proposed values based on their height and origin:
/// - Drops values from lower heights
/// - Queues values from higher heights for later processing
/// - For parts-only mode or values from Sync, generates and signs internal Proposal messages
/// - Stores the value and appends it to the WAL if new
/// - Applies any associated proposals to the driver
/// - Attempts immediate decision for values from Sync
///
/// # Arguments
/// * `co` - Coordination object for async operations
/// * `state` - Current consensus state
/// * `metrics` - Metrics collection
/// * `proposed_value` - The proposed value to process
/// * `origin` - Origin of the proposed value (e.g., Sync, Network)
///
/// # Returns
/// Result indicating success or failure of processing the proposed value
pub async fn on_proposed_value<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    proposed_value: ProposedValue<Ctx>,
    origin: ValueOrigin,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.driver.height() > proposed_value.height {
        debug!("Received value for lower height, dropping");
        return Ok(());
    }

    if state.driver.height() < proposed_value.height {
        debug!("Received value for higher height, queuing for later");

        state.buffer_input(
            proposed_value.height,
            Input::ProposedValue(proposed_value, origin),
        );

        return Ok(());
    }

    // If this is the first time we see this value, append it to the WAL, so it can be used for recovery.
    if !state.value_exists(&proposed_value) {
        perform!(
            co,
            Effect::WalAppend(
                WalEntry::ProposedValue(proposed_value.clone()),
                Default::default()
            )
        );
    }

    state.store_value(&proposed_value);

    let validity = proposed_value.validity;
    let proposals = state.proposals_for_value(&proposed_value);

    for signed_proposal in proposals {
        debug!(
            proposal.height = %signed_proposal.height(),
            proposal.round = %signed_proposal.round(),
            "We have a full proposal for this round, checking..."
        );

        apply_driver_input(
            co,
            state,
            metrics,
            DriverInput::Proposal(signed_proposal, validity),
        )
        .await?;
    }

    if origin == ValueOrigin::Sync {
        // The proposed value was provided by Sync, try to decide immediately, without waiting for the Commit timeout.
        // `try_decide` will check that we are in the commit step after applying the proposed value to the state machine.
        try_decide(co, state, metrics).await?;
    }

    Ok(())
}
