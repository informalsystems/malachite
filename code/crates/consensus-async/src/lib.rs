#![cfg_attr(docsrs, feature(doc_cfg))]

use config::Timeouts;
use derive_where::derive_where;
use tokio::sync::{mpsc, oneshot};

use malachitebft_core_consensus::{Effect, Params, Resumable, Resume};

pub use malachitebft_core_consensus::types;
pub use malachitebft_core_consensus::{process, Error as ConsensusError, Input, State};

pub mod config;
pub mod query;

use query::*;
use types::*;

mod timers;
use timers::Timers;

#[derive(thiserror::Error)]
#[derive_where(Debug)]
pub enum Error<Ctx>
where
    Ctx: Context,
{
    #[error("Consensus error: {0}")]
    Consensus(#[from] ConsensusError<Ctx>),

    #[error("Send error")]
    SendError(Input<Ctx>),
}

pub struct Handle<Ctx>
where
    Ctx: Context,
{
    capacity: usize,
    tx_input: mpsc::Sender<(Input<Ctx>, mpsc::Sender<Query<Ctx>>)>,
}

impl<Ctx> Handle<Ctx>
where
    Ctx: Context,
{
    pub async fn process(
        &self,
        input: Input<Ctx>,
    ) -> Result<mpsc::Receiver<Query<Ctx>>, Error<Ctx>> {
        let (tx_query, rx_query) = mpsc::channel(self.capacity);

        self.tx_input
            .send((input, tx_query))
            .await
            .map_err(|e| Error::SendError(e.0 .0))?;

        Ok(rx_query)
    }
}

pub struct Consensus<Ctx>
where
    Ctx: Context,
{
    ctx: Ctx,
    state: State<Ctx>,
    timers: Timers<Timeout>,
    timeouts: Timeouts,
    metrics: Metrics,
    rx_input: mpsc::Receiver<(Input<Ctx>, mpsc::Sender<Query<Ctx>>)>,
}

impl<Ctx> Consensus<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        ctx: Ctx,
        params: Params<Ctx>,
        timeouts: Timeouts,
        capacity: usize,
    ) -> (Self, Handle<Ctx>) {
        let (tx_input, rx_input) = mpsc::channel(capacity);

        (
            Self {
                ctx: ctx.clone(),
                state: State::new(ctx, params),
                timers: Timers::new(),
                timeouts,
                metrics: Metrics::default(),
                rx_input,
            },
            Handle { capacity, tx_input },
        )
    }

    pub async fn run(mut self) {
        let mut timer_elapsed = self.timers.subscribe();

        loop {
            tokio::select! {
                _ = timer_elapsed.recv() => {
                    // TODO
                }
                input = self.rx_input.recv() => {
                    match input {
                        Some((input, tx_query)) => {
                            if let Err(e) = self.process(input, tx_query).await {
                                tracing::error!("Error: {e}");
                            }
                        }
                        None => break,
                    }

                }
            }
        }
    }

    pub async fn process(
        &mut self,
        input: Input<Ctx>,
        tx_query: mpsc::Sender<Query<Ctx>>,
    ) -> Result<(), Error<Ctx>> {
        process!(
            input: input,
            state: &mut self.state,
            metrics: &self.metrics,
            with: effect => handle_effect(
                &self.ctx,
                effect,
                &mut self.timers,
                &self.timeouts,
                &tx_query
            ).await
        )
    }
}

macro_rules! query {
    ($tx_query:expr, $resume:expr, $map:expr, $query:expr) => {{
        let (reply, rx) = oneshot::channel();
        $tx_query.send($query(reply).into()).await.unwrap();
        let result = rx.await.unwrap();
        Ok($resume.resume_with($map(result)))
    }};

    ($tx_query:expr, $resume:expr, $query:expr) => {{
        query!($tx_query, $resume, |x| x, $query)
    }};
}

async fn handle_effect<Ctx>(
    ctx: &Ctx,
    effect: Effect<Ctx>,
    timers: &mut Timers<Timeout>,
    timeouts: &Timeouts,
    tx_query: &mpsc::Sender<Query<Ctx>>,
) -> Result<Resume<Ctx>, Error<Ctx>>
where
    Ctx: Context,
{
    match effect {
        // Timers
        Effect::ResetTimeouts(resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        Effect::CancelAllTimeouts(resume) => {
            timers.cancel_all();
            Ok(resume.resume_with(()))
        }

        Effect::CancelTimeout(timeout, resume) => {
            timers.cancel(&timeout);
            Ok(resume.resume_with(()))
        }

        Effect::ScheduleTimeout(timeout, resume) => {
            let duration = timeouts.timeout_duration(timeout.kind);
            timers.start(timeout, duration);

            Ok(resume.resume_with(()))
        }

        // Consensus
        Effect::StartRound(height, round, proposer, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::StartRound(
                height, round, proposer, reply
            ))
        }
        Effect::Publish(signed_consensus_msg, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::Publish(
                signed_consensus_msg,
                reply
            ))
        }
        Effect::GetValue(height, round, timeout, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::GetValue(
                height, round, timeout, reply
            ))
        }
        Effect::RestreamValue(height, round, pol_round, proposer, id, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::RestreamValue(
                height, round, pol_round, proposer, id, reply
            ))
        }
        Effect::GetValidatorSet(height, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::GetValidatorSet(
                height, reply
            ))
        }
        Effect::Decide(certificate, resume) => {
            query!(tx_query, resume, |reply| ConsensusQuery::Decide(
                certificate,
                reply
            ))
        }

        // Vote Sync
        Effect::GetVoteSet(height, round, resume) => {
            query!(tx_query, resume, |reply| SyncQuery::GetVoteSet(
                height, round, reply
            ))
        }
        Effect::SendVoteSetResponse(request_id, height, round, vote_set, resume) => {
            query!(tx_query, resume, |reply| {
                SyncQuery::SendVoteSetResponse(request_id, height, round, vote_set, reply)
            })
        }

        // WAL
        Effect::WalAppendMessage(msg, resume) => {
            query!(tx_query, resume, |reply| WalQuery::AppendMessage(
                msg, reply
            ))
        }
        Effect::WalAppendTimeout(timeout, resume) => {
            query!(tx_query, resume, |reply| WalQuery::AppendTimeout(
                timeout, reply
            ))
        }

        // Signing
        Effect::SignVote(vote, resume) => {
            // query!(tx_query, resume, |reply| SigningQuery::SignVote(
            //     vote, reply
            // ))
            Ok(resume.resume_with(ctx.signing_provider().sign_vote(vote)))
        }
        Effect::SignProposal(proposal, resume) => {
            // query!(tx_query, resume, |reply| SigningQuery::SignProposal(
            //     proposal, reply
            // ))
            Ok(resume.resume_with(ctx.signing_provider().sign_proposal(proposal)))
        }
        Effect::VerifySignature(msg, public_key, resume) => {
            // query!(tx_query, resume, |reply| {
            //     SigningQuery::VerifySignature(msg, public_key, reply)
            // })
            match msg.message {
                ConsensusMsg::Vote(vote) => {
                    let valid = ctx.signing_provider().verify_signed_vote(
                        &vote,
                        &msg.signature,
                        &public_key,
                    );

                    Ok(resume.resume_with(valid.into()))
                }
                ConsensusMsg::Proposal(proposal) => {
                    let valid = ctx.signing_provider().verify_signed_proposal(
                        &proposal,
                        &msg.signature,
                        &public_key,
                    );

                    Ok(resume.resume_with(valid.into()))
                }
            }
        }
        Effect::VerifyCertificate(certificate, validator_set, threshold_params, resume) => {
            // query!(tx_query, resume, |reply| SigningQuery::VerifyCertificate(
            //     certificate,
            //     validator_set,
            //     threshold_params,
            //     reply
            // ))
            Ok(
                resume.resume_with(ctx.signing_provider().verify_certificate(
                    &certificate,
                    &validator_set,
                    threshold_params,
                )),
            )
        }
    }
}
