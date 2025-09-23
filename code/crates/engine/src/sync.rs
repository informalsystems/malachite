use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use derive_where::derive_where;
use eyre::eyre;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use rand::SeedableRng;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn, Instrument};

use crate::consensus::{ConsensusMsg, ConsensusRef};
use crate::host::{HostMsg, HostRef};
use crate::network::{NetworkEvent, NetworkMsg, NetworkRef, Status};
use crate::util::ticker::ticker;
use crate::util::timers::{TimeoutElapsed, TimerScheduler};
use malachitebft_codec as codec;
use malachitebft_core_consensus::PeerId;
use malachitebft_core_types::{CommitCertificate, Context, Height};
use malachitebft_sync::Response::ValueResponse;
use malachitebft_sync::{
    self as sync, HeightStartType, InboundRequestId, OutboundRequestId, RawDecidedValue, Request,
    Response, Resumable,
};

/// Codec for sync protocol messages
///
/// This trait is automatically implemented for any type that implements:
/// - [`codec::Codec<sync::Status<Ctx>>`]
/// - [`codec::Codec<sync::Request<Ctx>>`]
/// - [`codec::Codec<sync::Response<Ctx>>`]
pub trait SyncCodec<Ctx>
where
    Ctx: Context,
    Self: codec::Codec<sync::Status<Ctx>>,
    Self: codec::Codec<sync::Request<Ctx>>,
    Self: codec::Codec<sync::Response<Ctx>>,
{
}

impl<Ctx, Codec> SyncCodec<Ctx> for Codec
where
    Ctx: Context,
    Codec: codec::Codec<sync::Status<Ctx>>,
    Codec: codec::Codec<sync::Request<Ctx>>,
    Codec: codec::Codec<sync::Response<Ctx>>,
{
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Timeout {
    Request(OutboundRequestId),
}

type Timers = TimerScheduler<Timeout>;

pub type SyncRef<Ctx> = ActorRef<Msg<Ctx>>;

#[derive_where(Clone, Debug)]
pub struct RawDecidedBlock<Ctx: Context> {
    pub height: Ctx::Height,
    pub certificate: CommitCertificate<Ctx>,
    pub value_bytes: Bytes,
}

#[derive_where(Clone, Debug)]
pub struct InflightRequest<Ctx: Context> {
    pub peer_id: PeerId,
    pub request_id: OutboundRequestId,
    pub request: Request<Ctx>,
}

pub type InflightRequests<Ctx> = HashMap<OutboundRequestId, InflightRequest<Ctx>>;

#[derive_where(Debug)]
pub enum Msg<Ctx: Context> {
    /// Internal tick
    Tick,

    /// Receive an even from gossip layer
    NetworkEvent(NetworkEvent<Ctx>),

    /// Consensus has decided on a value at the given height
    Decided(Ctx::Height),

    /// Consensus has (re)started a new height.
    ///
    /// The second argument indicates whether this is a restart or not.
    StartedHeight(Ctx::Height, HeightStartType),

    /// Host has a response for the blocks request
    GotDecidedValues(
        InboundRequestId,
        RangeInclusive<Ctx::Height>,
        Vec<RawDecidedValue<Ctx>>,
    ),

    /// A timeout has elapsed
    TimeoutElapsed(TimeoutElapsed<Timeout>),

    /// We received an invalid value (either certificate or value) from a peer
    InvalidValue(PeerId, Ctx::Height),

    /// An error occurred while processing a value
    ValueProcessingError(PeerId, Ctx::Height),

    /// Sets the consensus actor to be used by the sync actor.
    SetConsensusActor(ConsensusRef<Ctx>, RpcReplyPort<()>),
}

impl<Ctx: Context> From<NetworkEvent<Ctx>> for Msg<Ctx> {
    fn from(event: NetworkEvent<Ctx>) -> Self {
        Msg::NetworkEvent(event)
    }
}

impl<Ctx: Context> From<TimeoutElapsed<Timeout>> for Msg<Ctx> {
    fn from(elapsed: TimeoutElapsed<Timeout>) -> Self {
        Msg::TimeoutElapsed(elapsed)
    }
}

#[derive(Debug)]
pub struct Params {
    pub status_update_interval: Duration,
    pub request_timeout: Duration,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            status_update_interval: Duration::from_secs(5),
            request_timeout: Duration::from_secs(10),
        }
    }
}

pub struct State<Ctx: Context> {
    /// The state of the sync state machine
    sync: sync::State<Ctx>,

    /// Scheduler for timers
    timers: Timers,

    /// In-flight requests
    inflight: InflightRequests<Ctx>,

    /// Task for sending status updates
    ticker: JoinHandle<()>,

    consensus: Option<ConsensusRef<Ctx>>,
}

#[allow(dead_code)]
pub struct Sync<Ctx: Context> {
    ctx: Ctx,
    gossip: NetworkRef<Ctx>,
    host: HostRef<Ctx>,
    params: Params,
    sync_config: sync::Config,
    metrics: sync::Metrics,
    span: tracing::Span,
}

impl<Ctx> Sync<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        ctx: Ctx,
        gossip: NetworkRef<Ctx>,
        host: HostRef<Ctx>,
        params: Params,
        sync_config: sync::Config,
        metrics: sync::Metrics,
        span: tracing::Span,
    ) -> Self {
        Self {
            ctx,
            gossip,
            host,
            params,
            sync_config,
            metrics,
            span,
        }
    }

    pub async fn spawn(
        ctx: Ctx,
        gossip: NetworkRef<Ctx>,
        host: HostRef<Ctx>,
        params: Params,
        sync_config: sync::Config,
        metrics: sync::Metrics,
        span: tracing::Span,
    ) -> Result<SyncRef<Ctx>, ractor::SpawnErr> {
        let actor = Self::new(ctx, gossip, host, params, sync_config, metrics, span);
        let (actor_ref, _) = Actor::spawn(None, actor, ()).await?;
        Ok(actor_ref)
    }

    async fn process_input(
        &self,
        myself: &ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
        input: sync::Input<Ctx>,
    ) -> Result<(), ActorProcessingErr> {
        malachitebft_sync::process!(
            input: input,
            state: &mut state.sync,
            metrics: &self.metrics,
            with: effect => {
                self.handle_effect(myself, &mut state.consensus, &mut state.timers, &mut state.inflight, effect).await
            }
        )
    }

    async fn get_history_min_height(&self) -> Result<Ctx::Height, ActorProcessingErr> {
        ractor::call!(self.host, |reply_to| HostMsg::GetHistoryMinHeight {
            reply_to
        })
        .map_err(|e| eyre!("Failed to get earliest history height: {e:?}").into())
    }

    async fn handle_effect(
        &self,
        myself: &ActorRef<Msg<Ctx>>,
        consensus_actor: &mut Option<ActorRef<ConsensusMsg<Ctx>>>,
        timers: &mut Timers,
        inflight: &mut InflightRequests<Ctx>,
        effect: sync::Effect<Ctx>,
    ) -> Result<sync::Resume<Ctx>, ActorProcessingErr> {
        use sync::Effect;

        match effect {
            Effect::BroadcastStatus(height, r) => {
                let history_min_height = self.get_history_min_height().await?;

                self.gossip.cast(NetworkMsg::BroadcastStatus(Status::new(
                    height,
                    history_min_height,
                )))?;

                Ok(r.resume_with(()))
            }

            Effect::NotifyConsensusToProcessSyncResponse(
                request_id,
                peer_id,
                value_response,
                r,
            ) => {
                let consensus_actor = consensus_actor.as_ref().unwrap();
                consensus_actor.cast(ConsensusMsg::ProcessSyncResponse(
                    request_id,
                    peer_id,
                    ValueResponse(value_response),
                ))?;

                Ok(r.resume_with(()))
            }

            Effect::SendValueRequest(peer_id, value_request, r) => {
                let request = Request::ValueRequest(value_request);
                let result = ractor::call!(self.gossip, |reply_to| {
                    NetworkMsg::OutgoingRequest(peer_id, request.clone(), reply_to)
                });

                match result {
                    Ok(request_id) => {
                        let request_id = OutboundRequestId::new(request_id);

                        timers.start_timer(
                            Timeout::Request(request_id.clone()),
                            self.params.request_timeout,
                        );

                        inflight.insert(
                            request_id.clone(),
                            InflightRequest {
                                peer_id,
                                request_id: request_id.clone(),
                                request,
                            },
                        );

                        Ok(r.resume_with(Some(request_id)))
                    }
                    Err(e) => {
                        error!("Failed to send request to network layer: {e}");
                        Ok(r.resume_with(None))
                    }
                }
            }

            Effect::SendValueResponse(request_id, value_response, r) => {
                let response = Response::ValueResponse(value_response);
                self.gossip
                    .cast(NetworkMsg::OutgoingResponse(request_id, response))?;

                Ok(r.resume_with(()))
            }

            Effect::GetDecidedValues(request_id, range, r) => {
                let mut values = Vec::new();
                let mut height = *range.start();

                let mut response_size_bytes = 0;
                while height <= *range.end() {
                    let value = self
                        .host
                        .call(
                            |reply_to| HostMsg::GetDecidedValue { height, reply_to },
                            None,
                        )
                        .await?
                        .success_or(eyre!("Failed to get decided value for height {height}"))?;

                    if let Some(value) = value {
                        let value_response = ValueResponse(sync::ValueResponse::new(
                            *range.start(),
                            vec![value.clone()],
                        ));

                        let result = ractor::call!(self.gossip, move |reply_to| {
                            NetworkMsg::GetResponseSize(value_response.clone(), reply_to)
                        });

                        let total_value_size_bytes = match result {
                            Ok(value_in_bytes) => value_in_bytes,
                            Err(e) => {
                                error!("Failed to get response size for value, stopping at for height {}: {:?}", height, e);
                                break;
                            }
                        };

                        // check if adding this value would exceed the max-response limit
                        if response_size_bytes + total_value_size_bytes
                            > self.sync_config.max_response_size
                        {
                            warn!("Maximum byte size limit ({} bytes) would be exceeded (current: {} + upcoming value: {}), stopping at height {}",
                              self.sync_config.max_response_size, response_size_bytes, total_value_size_bytes, height);
                            break;
                        }

                        response_size_bytes += total_value_size_bytes;
                        values.push(value);

                        if response_size_bytes == self.sync_config.max_response_size {
                            info!(
                                "Reached maximum byte size limit ({} bytes) exactly at height {}",
                                self.sync_config.max_response_size, height
                            );

                            break;
                        }
                    } else {
                        warn!("Decided value not found for height {height}");
                        break;
                    }

                    height = height.increment();
                }

                myself.cast(Msg::<Ctx>::GotDecidedValues(request_id, range, values))?;

                Ok(r.resume_with(()))
            }
        }
    }

    async fn handle_msg(
        &self,
        myself: ActorRef<Msg<Ctx>>,
        msg: Msg<Ctx>,
        state: &mut State<Ctx>,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            Msg::Tick => {
                self.process_input(&myself, state, sync::Input::Tick)
                    .await?;
            }

            Msg::NetworkEvent(NetworkEvent::PeerDisconnected(peer_id)) => {
                info!(%peer_id, "Disconnected from peer");

                if state.sync.peers.remove(&peer_id).is_some() {
                    debug!(%peer_id, "Removed disconnected peer");
                }
            }

            Msg::NetworkEvent(NetworkEvent::Status(peer_id, status)) => {
                let status = sync::Status {
                    peer_id,
                    tip_height: status.tip_height,
                    history_min_height: status.history_min_height,
                };

                self.process_input(&myself, state, sync::Input::Status(status))
                    .await?;
            }

            Msg::NetworkEvent(NetworkEvent::SyncRequest(request_id, from, request)) => {
                match request {
                    Request::ValueRequest(value_request) => {
                        self.process_input(
                            &myself,
                            state,
                            sync::Input::ValueRequest(request_id, from, value_request),
                        )
                        .await?;
                    }
                };
            }

            Msg::NetworkEvent(NetworkEvent::SyncResponse(request_id, peer, response)) => {
                // Cancel the timer associated with the request for which we just received a response
                state.timers.cancel(&Timeout::Request(request_id.clone()));

                match response {
                    Some(Response::ValueResponse(value_response)) => {
                        self.process_input(
                            &myself,
                            state,
                            sync::Input::ValueResponse(request_id, peer, Some(value_response)),
                        )
                        .await?;
                    }

                    None => {
                        self.process_input(
                            &myself,
                            state,
                            sync::Input::ValueResponse(request_id, peer, None),
                        )
                        .await?;
                    }
                }
            }

            Msg::NetworkEvent(_) => {
                // Ignore other gossip events
            }

            // (Re)Started a new height
            Msg::StartedHeight(height, restart) => {
                self.process_input(&myself, state, sync::Input::StartedHeight(height, restart))
                    .await?
            }

            // Decided on a value
            Msg::Decided(height) => {
                self.process_input(&myself, state, sync::Input::Decided(height))
                    .await?;
            }

            Msg::GotDecidedValues(request_id, range, blocks) => {
                self.process_input(
                    &myself,
                    state,
                    sync::Input::GotDecidedValues(request_id, range, blocks),
                )
                .await?;
            }

            Msg::InvalidValue(peer, height) => {
                self.process_input(&myself, state, sync::Input::InvalidValue(peer, height))
                    .await?
            }

            Msg::ValueProcessingError(peer, height) => {
                self.process_input(
                    &myself,
                    state,
                    sync::Input::ValueProcessingError(peer, height),
                )
                .await?
            }

            Msg::TimeoutElapsed(elapsed) => {
                let Some(timeout) = state.timers.intercept_timer_msg(elapsed) else {
                    // Timer was cancelled or already processed, ignore
                    return Ok(());
                };

                warn!(?timeout, "Timeout elapsed");

                match timeout {
                    Timeout::Request(request_id) => {
                        if let Some(inflight) = state.inflight.remove(&request_id) {
                            self.process_input(
                                &myself,
                                state,
                                sync::Input::SyncRequestTimedOut(
                                    request_id,
                                    inflight.peer_id,
                                    inflight.request,
                                ),
                            )
                            .await?;
                        } else {
                            debug!(%request_id, "Timeout for unknown request");
                        }
                    }
                }
            }

            Msg::SetConsensusActor(consensus_actor, reply_to) => {
                state.consensus = Some(consensus_actor);
                // reply to acknowledge we've updated the state and to not make the consensus actor wait
                let _ = reply_to.send(());
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<Ctx> Actor for Sync<Ctx>
where
    Ctx: Context,
{
    type Msg = Msg<Ctx>;
    type State = State<Ctx>;
    type Arguments = ();

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        self.gossip
            .cast(NetworkMsg::Subscribe(Box::new(myself.clone())))?;

        let ticker = tokio::spawn(
            ticker(self.params.status_update_interval, myself.clone(), || {
                Msg::Tick
            })
            .in_current_span(),
        );

        let rng = Box::new(rand::rngs::StdRng::from_entropy());

        Ok(State {
            sync: sync::State::new(rng, self.sync_config),
            timers: Timers::new(Box::new(myself.clone())),
            inflight: HashMap::new(),
            ticker,
            consensus: None,
        })
    }

    #[tracing::instrument(
        name = "sync",
        parent = &self.span,
        skip_all,
        fields(
            height.tip = %state.sync.tip_height,
        ),
    )]
    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Err(e) = self.handle_msg(myself, msg, state).await {
            error!("Error handling message: {e:?}");
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.ticker.abort();
        Ok(())
    }
}
