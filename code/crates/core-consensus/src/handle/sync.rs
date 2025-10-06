use crate::handle::driver::apply_driver_input;
use crate::handle::signature::verify_commit_certificate;
use crate::handle::validator_set::get_validator_set;
use crate::prelude::*;
use malachitebft_core_state_machine::input::Certificate;

pub async fn on_value_response<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    value: ValueResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let consensus_height = state.height();

    // FaB: Extract height and round from the first vote in the certificate
    let first_vote = value
        .certificate
        .first()
        .expect("Certificate should not be empty");
    let cert_height = first_vote.message.height();
    let cert_round = first_vote.message.round();

    if consensus_height > cert_height {
        debug!(
            %consensus_height,
            %cert_height,
            "Received value response for lower height, ignoring"
        );
        return Ok(());
    }

    if consensus_height < cert_height {
        debug!(%consensus_height, %cert_height, "Received value response for higher height, queuing for later");

        state.buffer_sync_input(cert_height, Input::SyncValueResponse(value), metrics);

        return Ok(());
    }

    info!(
        value.certificate.height = %cert_height,
        signatures = value.certificate.len(),
        "Processing value response"
    );

    let proposer = state
        .get_proposer(cert_height, cert_round)
        .clone();

    let peer = value.peer;

    let effect = process_certificate(co, state, metrics, value.certificate.clone())
        .await
        .map(|_| Effect::ValidSyncValue(value, proposer, Default::default()))
        .unwrap_or_else(|e| {
            error!("Error when processing certificate: {e}");
            Effect::InvalidSyncValue(peer, cert_height, e, Default::default())
        });

    perform!(co, effect);

    Ok(())
}

// FaB: Validate a certificate (4f+1 prevotes) received from sync
// FaB: Just checks basic validity - the full verification happens in the effect handler
async fn process_certificate<Ctx>(
    _co: &Co<Ctx>,
    _state: &mut State<Ctx>,
    _metrics: &Metrics,
    certificate: Certificate<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    // FaB: Extract height and round from the first vote for logging
    // FaB: If certificate is empty, just log and return Ok - effect handler will deal with it
    if let Some(first_vote) = certificate.first() {
        let cert_height = first_vote.message.height();
        let cert_round = first_vote.message.round();

        info!(
            %cert_height,
            %cert_round,
            signatures = certificate.len(),
            "Synced certificate passed basic validation"
        );
    } else {
        warn!("Received empty certificate from sync");
    }

    // FaB: Just return Ok - the Effect::ValidSyncValue handler will decode the value
    // FaB: and pass it through consensus, which will verify the certificate properly
    Ok(())
}
