use crate::prelude::*;

use crate::handle::driver::apply_driver_input;
use crate::types::ProposedValue;

use super::decide::decide_current_no_timeout;

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

    state.store_value(&proposed_value);

    let proposals = state.full_proposals_for_value(&proposed_value);
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
            DriverInput::Proposal(signed_proposal, proposed_value.validity),
        )
        .await?;
    }

    if origin == ValueOrigin::Sync && state.driver.step_is_commit() {
        decide_current_no_timeout(co, state, metrics).await?;
    }

    Ok(())
}
