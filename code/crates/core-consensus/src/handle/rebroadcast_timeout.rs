use crate::{prelude::*, VoteSyncMode};

#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub async fn on_rebroadcast_timeout<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    timeout: Timeout,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.params.vote_sync_mode != VoteSyncMode::Rebroadcast {
        return Ok(());
    }

    let (height, round) = (state.driver.height(), state.driver.round());
    warn!(
        %height, %round,
        "Rebroadcasting vote at {:?} step after {:?} timeout",
        state.driver.step(), timeout.kind,
    );

    if let Some(vote) = state.last_signed_prevote.as_ref() {
        perform!(
            co,
            Effect::RebroadcastVote(vote.clone(), Default::default())
        );
    };

    if let Some(vote) = state.last_signed_precommit.as_ref() {
        perform!(
            co,
            Effect::RebroadcastVote(vote.clone(), Default::default())
        );
    };

    if let Some(certificate) = state.round_certificate() {
        // TODO: Should not send a certificate before we enter the round that's triggered by it.
        // For example, check that the certificate's `enter_round` (to be recorded with the certificate) is the same as the current round
        warn!(
            %certificate.height,
            %certificate.round,
            number_of_votes = certificate.round_signatures.len(),
            "Rebroadcasting round certificate"
        );
        perform!(
            co,
            Effect::RebroadcastRoundCertificate(certificate.clone(), Default::default())
        );
    };

    #[cfg(feature = "metrics")]
    metrics.rebroadcast_timeouts.inc();

    perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));

    Ok(())
}
