use crate::prelude::*;

#[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
pub async fn on_rebroadcast_timeout<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let (height, round) = (state.driver.height(), state.driver.round());

    // FaB: Line 111 - rebroadcast lastPrevote_p
    if let Some(vote) = state.last_signed_prevote.as_ref() {
        warn!(
            %height, %round, vote_height = %vote.height(), vote_round = %vote.round(),
            "Rebroadcasting prevote at {:?} step",
            state.driver.step()
        );

        perform!(co, Effect::RepublishVote(vote.clone(), Default::default()));
    };

    // FaB: Line 113 - if prevotedValue_p != nil, rebroadcast prevotedProposalMsg_p
    if let Some(proposal) = state.driver.prevoted_proposal_msg() {
        let proposer = state.get_proposer(proposal.height(), proposal.round()).clone();

        warn!(
            %height, %round,
            proposal_height = %proposal.height(),
            proposal_round = %proposal.round(),
            value_id = ?proposal.value().id(),
            "Rebroadcasting proposal for prevoted value"
        );

        perform!(
            co,
            Effect::RestreamProposal(
                proposal.height(),
                proposal.round(),
                Round::Nil, // FaB: valid_round not used in FaB
                proposer,
                proposal.value().id(),
                Default::default()
            )
        );
    }

    // FaB: Removed precommit rebroadcast - no precommits in FaB
    if let Some(vote) = state.last_signed_precommit.as_ref() {
        warn!(
            %height, %round, vote_height = %vote.height(), vote_round = %vote.round(),
            step = ?state.driver.step(),
            "Rebroadcasting precommit (should not happen in FaB)"
        );
        perform!(co, Effect::RepublishVote(vote.clone(), Default::default()));
    };

    if let Some(cert) = state.round_certificate() {
        if cert.enter_round == round {
            warn!(
                %cert.certificate.height,
                %round,
                %cert.certificate.round,
                number_of_votes = cert.certificate.round_signatures.len(),
                "Rebroadcasting round certificate"
            );
            perform!(
                co,
                Effect::RepublishRoundCertificate(cert.certificate.clone(), Default::default())
            );
        }
    };

    #[cfg(feature = "metrics")]
    metrics.rebroadcast_timeouts.inc();

    let timeout = Timeout::rebroadcast(round);
    perform!(co, Effect::ScheduleTimeout(timeout, Default::default()));

    Ok(())
}
