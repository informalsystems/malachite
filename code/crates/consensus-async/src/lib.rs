use std::ops::ControlFlow;
use std::time::Duration;

use derive_where::derive_where;
use tokio::sync::mpsc;

use malachitebft_core_consensus::{Effect, Params, Resumable, Resume};

pub use malachitebft_core_consensus::types::*;
pub use malachitebft_core_consensus::{process, Error as ConsensusError, Input, State};

mod query;
pub use query::{Query, Reply};

#[cfg(feature = "timers")]
mod timers;

#[cfg(feature = "timers")]
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
    state: State<Ctx>,
    #[cfg(feature = "timers")]
    timers: Timers<Timeout>,
    metrics: Metrics,
    rx_input: mpsc::Receiver<(Input<Ctx>, mpsc::Sender<Query<Ctx>>)>,
}

impl<Ctx> Consensus<Ctx>
where
    Ctx: Context,
{
    pub fn new(ctx: Ctx, params: Params<Ctx>, capacity: usize) -> (Self, Handle<Ctx>) {
        let (tx_input, rx_input) = mpsc::channel(capacity);

        (
            Self {
                state: State::new(ctx, params),
                #[cfg(feature = "timers")]
                timers: Timers::new(),
                metrics: Metrics::default(),
                rx_input,
            },
            Handle { capacity, tx_input },
        )
    }

    pub async fn run(mut self) {
        loop {
            match self.run_inner().await {
                Ok(ControlFlow::Continue(())) => continue,
                Ok(ControlFlow::Break(())) => break,
                Err(e) => {
                    tracing::error!("Error: {e}");
                }
            }
        }
    }

    pub async fn run_inner(&mut self) -> Result<ControlFlow<()>, Error<Ctx>> {
        match self.rx_input.recv().await {
            Some((input, tx_query)) => {
                self.process(input, tx_query).await?;
                Ok(ControlFlow::Continue(()))
            }
            None => Ok(ControlFlow::Break(())),
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
                effect,
                #[cfg(feature = "timers")]
                &mut self.timers,
                &tx_query
            ).await
        )
    }
}

async fn handle_effect<Ctx>(
    effect: Effect<Ctx>,
    #[cfg(feature = "timers")] timers: &mut Timers<Timeout>,
    tx_query: &mpsc::Sender<Query<Ctx>>,
) -> Result<Resume<Ctx>, Error<Ctx>>
where
    Ctx: Context,
{
    match effect {
        // Timers
        #[cfg(feature = "timers")]
        Effect::ResetTimeouts(resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        #[cfg(not(feature = "timers"))]
        Effect::ResetTimeouts(resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        #[cfg(feature = "timers")]
        Effect::CancelAllTimeouts(resume) => {
            timers.cancel_all();
            Ok(resume.resume_with(()))
        }

        #[cfg(not(feature = "timers"))]
        Effect::CancelAllTimeouts(resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        #[cfg(feature = "timers")]
        Effect::CancelTimeout(timeout, resume) => {
            timers.cancel(&timeout);
            Ok(resume.resume_with(()))
        }

        #[cfg(not(feature = "timers"))]
        Effect::CancelTimeout(timeout, resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        #[cfg(feature = "timers")]
        Effect::ScheduleTimeout(timeout, resume) => {
            let duration = Duration::from_secs(1);
            timers.start(timeout, duration);

            Ok(resume.resume_with(()))
        }

        #[cfg(not(feature = "timers"))]
        Effect::ScheduleTimeout(timeout, resume) => {
            // TODO
            Ok(resume.resume_with(()))
        }

        // Consensus
        Effect::StartRound(height, round, proposer, resume) => todo!(),
        Effect::Publish(signed_consensus_msg, resume) => todo!(),
        Effect::GetValue(height, round, timeout, resume) => todo!(),
        Effect::RestreamValue(height, round, pol_round, proposer, id, resume) => todo!(),
        Effect::GetValidatorSet(height, resume) => todo!(),
        Effect::Decide(certificate, resume) => todo!(),

        // Vote Sync
        Effect::GetVoteSet(height, round, resume) => todo!(),
        Effect::SendVoteSetResponse(request_id, height, round, vote_set, resume) => todo!(),

        // WAL
        Effect::WalAppendMessage(msg, resume) => todo!(),
        Effect::WalAppendTimeout(timeout, resume) => todo!(),

        // Signing
        Effect::SignVote(vote, resume) => todo!(),
        Effect::SignProposal(proposal, resume) => todo!(),
        Effect::VerifySignature(msg, public_key, resume) => todo!(),
        Effect::VerifyCertificate(certificate, validator_set, threshold_params, resume) => {
            todo!()
        }
    }
}
