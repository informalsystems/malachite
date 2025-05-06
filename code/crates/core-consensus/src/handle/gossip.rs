use crate::handle::validator_set::get_validator_set;
use crate::handle::{driver::apply_driver_input, vote::verify_signed_vote};
use crate::prelude::*;

use super::signature::verify_polka_certificate;

pub async fn on_polka_certificate<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    certificate: PolkaCertificate<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(%certificate.height, %certificate.round, "Received polka certificate");

    if certificate.height != state.height() {
        warn!(
            %certificate.height,
            consensus.height = %state.height(),
            "Polka certificate height mismatch"
        );

        return Ok(());
    }

    let validator_set = get_validator_set(co, state, certificate.height)
        .await?
        .ok_or_else(|| Error::ValidatorSetNotFound(certificate.height))?;

    let validity = verify_polka_certificate(
        co,
        certificate.clone(),
        validator_set.into_owned(),
        state.params.threshold_params,
    )
    .await?;

    if let Err(e) = validity {
        warn!(?certificate, "Invalid polka certificate: {e}");
        return Ok(());
    }

    apply_driver_input(
        co,
        state,
        metrics,
        DriverInput::PolkaCertificate(certificate),
    )
    .await
}

pub async fn on_round_certificate<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    certificate: RoundCertificate<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(%certificate.height, %certificate.round, "Received round certificate");

    if certificate.height != state.height() {
        warn!(
            %certificate.height,
            consensus.height = %state.height(),
            "Polka certificate height mismatch"
        );

        return Ok(());
    }

    for signature in certificate.round_signatures {
        let vote_type = signature.vote_type;
        let vote: SignedVote<Ctx> = match vote_type {
            VoteType::Prevote => SignedVote::new(
                state.ctx.new_prevote(
                    certificate.height,
                    certificate.round,
                    signature.value_id,
                    signature.address,
                ),
                signature.signature,
            ),
            VoteType::Precommit => SignedVote::new(
                state.ctx.new_precommit(
                    certificate.height,
                    certificate.round,
                    signature.value_id,
                    signature.address,
                ),
                signature.signature,
            ),
        };

        if !verify_signed_vote(co, state, &vote).await? {
            warn!(?vote, "Invalid vote");
            continue;
        }
        apply_driver_input(co, state, metrics, DriverInput::Vote(vote)).await?;
    }
    Ok(())
}
