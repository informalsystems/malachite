//! Implementation of a host actor for bridiging consensus and the application via a set of channels.

use ractor::{async_trait, Actor, ActorProcessingErr, ActorRef, SpawnErr};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::{error, warn};

use malachitebft_engine::host::HostMsg;

use crate::app::metrics::Metrics;
use crate::app::types::core::Context;
use crate::msgs::AppMsg;

/// Actor for bridging consensus and the application via a set of channels.
///
/// This actor is responsible for forwarding messages from the
/// consensus actor to the application over a channel, and vice-versa.
pub struct Connector<Ctx>
where
    Ctx: Context,
{
    sender: mpsc::Sender<AppMsg<Ctx>>,

    // TODO: add some metrics
    #[allow(dead_code)]
    metrics: Metrics,
}

impl<Ctx> Connector<Ctx>
where
    Ctx: Context,
{
    pub fn new(sender: mpsc::Sender<AppMsg<Ctx>>, metrics: Metrics) -> Self {
        Connector { sender, metrics }
    }

    pub async fn spawn(
        sender: mpsc::Sender<AppMsg<Ctx>>,
        metrics: Metrics,
    ) -> Result<ActorRef<HostMsg<Ctx>>, SpawnErr>
    where
        Ctx: Context,
    {
        let (actor_ref, _) = Actor::spawn(None, Self::new(sender, metrics), ()).await?;
        Ok(actor_ref)
    }
}

impl<Ctx> Connector<Ctx>
where
    Ctx: Context,
{
    async fn handle_msg(
        &self,
        _myself: ActorRef<HostMsg<Ctx>>,
        msg: HostMsg<Ctx>,
        _state: &mut (),
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            HostMsg::ConsensusReady { reply_to } => {
                let (reply, rx) = oneshot::channel();
                self.sender.send(AppMsg::ConsensusReady { reply }).await?;

                let (start_height, validator_set) = rx.await?;
                reply_to.send((start_height, validator_set))?;
            }

            HostMsg::StartedRound {
                height,
                round,
                proposer,
                role,
                reply_to,
            } => {
                let (reply_value, rx_values) = oneshot::channel();

                self.sender
                    .send(AppMsg::StartedRound {
                        height,
                        round,
                        proposer,
                        role,
                        reply_value,
                    })
                    .await?;

                // Do not block processing of other messages while waiting for the values
                tokio::spawn(async move {
                    if let Ok(values) = rx_values.await {
                        if let Err(e) = reply_to.send(values) {
                            error!("Failed to send back undecided values: {e}");
                        }
                    }
                });
            }

            HostMsg::GetValue {
                height,
                round,
                timeout,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::GetValue {
                        height,
                        round,
                        timeout,
                        reply,
                    })
                    .await?;

                reply_to.send(rx.await?)?;
            }

            HostMsg::ExtendVote {
                height,
                round,
                value_id,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::ExtendVote {
                        height,
                        round,
                        value_id,
                        reply,
                    })
                    .await?;

                reply_to.send(rx.await?)?;
            }

            HostMsg::VerifyVoteExtension {
                height,
                round,
                value_id,
                extension,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::VerifyVoteExtension {
                        height,
                        round,
                        value_id,
                        extension,
                        reply,
                    })
                    .await?;

                reply_to.send(rx.await?)?;
            }

            HostMsg::RestreamValue {
                height,
                round,
                valid_round,
                address,
                value_id,
            } => {
                self.sender
                    .send(AppMsg::RestreamProposal {
                        height,
                        round,
                        valid_round,
                        address,
                        value_id,
                    })
                    .await?
            }

            HostMsg::GetHistoryMinHeight { reply_to } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::GetHistoryMinHeight { reply })
                    .await?;

                reply_to.send(rx.await?)?;
            }

            HostMsg::ReceivedProposalPart {
                from,
                part,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::ReceivedProposalPart { from, part, reply })
                    .await?;

                if let Some(value) = rx.await? {
                    reply_to.send(value)?;
                }
            }

            HostMsg::Decided {
                certificate,
                extensions,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::Decided {
                        certificate,
                        extensions,
                        reply,
                    })
                    .await?;

                let next = rx.await?;

                if let Err(e) = reply_to.send(next) {
                    error!("Failed to send next height and validator set: {e}");
                }
            }

            HostMsg::GetDecidedValue { height, reply_to } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::GetDecidedValue { height, reply })
                    .await?;

                reply_to.send(rx.await?)?;
            }

            HostMsg::ProcessSyncedValue {
                height,
                round,
                proposer,
                value_bytes,
                reply_to,
            } => {
                let (reply, rx) = oneshot::channel();

                self.sender
                    .send(AppMsg::ProcessSyncedValue {
                        height,
                        round,
                        proposer,
                        value_bytes,
                        reply,
                    })
                    .await?;

                if let Some(value) = rx.await? {
                    if let Err(e) = reply_to.send(value) {
                        error!("Failed to send processed synced value: {e}");
                    }
                } else {
                    warn!("Failed to decode synced value");
                }
            }
        };

        Ok(())
    }
}

#[async_trait]
impl<Ctx> Actor for Connector<Ctx>
where
    Ctx: Context,
{
    type Msg = HostMsg<Ctx>;
    type State = ();
    type Arguments = ();

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Err(e) = self.handle_msg(myself, msg, state).await {
            tracing::error!("Error processing message: {e}");
        }

        Ok(())
    }
}
