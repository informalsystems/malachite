use crate::prelude::*;

use crate::handle::driver::apply_driver_input;
use crate::types::{LocallyProposedValue, ProposedValue};

pub async fn on_propose<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    value: LocallyProposedValue<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.driver.height() != value.height {
        warn!(
            "Ignoring proposal for height {}, current height: {}",
            value.height,
            state.driver.height()
        );

        return Ok(());
    }

    if state.driver.round() != value.round {
        warn!(
            "Ignoring propose value for round {}, current round: {}",
            value.round,
            state.driver.round()
        );

        return Ok(());
    }

    #[cfg(feature = "metrics")]
    metrics.consensus_start();

    state.store_value(&ProposedValue {
        height: value.height,
        round: value.round,
        valid_round: Round::Nil,
        proposer: state.address().clone(),
        value: value.value.clone(),
        validity: Validity::Valid,
    });

    perform!(
        co,
        Effect::WalAppendProposedValue(value.clone(), Default::default())
    );

    apply_driver_input(
        co,
        state,
        metrics,
        DriverInput::ProposeValue(value.round, value.value),
    )
    .await
}
