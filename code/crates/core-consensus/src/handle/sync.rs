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

// FaB: TODO - This function needs to be updated for FaB
// FaB: In FaB, sync should receive a Decision message (value + 4f+1 prevote certificate)
// FaB: Certificate is now Vec<SignedVote<Ctx>> containing 4f+1 prevotes
// FaB: For now, this is stubbed out to allow compilation
async fn process_certificate<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    certificate: Certificate<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    // FaB: Sync protocol needs updating for FaB
    // FaB: Should receive (value, certificate) and apply as DriverInput::ReceiveDecision
    warn!("FaB: Sync protocol not yet updated for FaB - Certificate handling stubbed out");
    Ok(())
}
