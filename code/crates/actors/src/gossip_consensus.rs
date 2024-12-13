use std::collections::{BTreeSet, HashMap};
use std::marker::PhantomData;

use async_trait::async_trait;
use derive_where::derive_where;
use eyre::eyre;
use libp2p::identity::Keypair;
use libp2p::request_response;
use ractor::port::OutputPortSubscriber;
use ractor::{Actor, ActorProcessingErr, ActorRef, OutputPort, RpcReplyPort};
use tokio::task::JoinHandle;
use tracing::{error, trace};

use malachite_sync::{
    self as sync, InboundRequestId, OutboundRequestId, RawMessage, Request, Response,
};

use malachite_codec as codec;
use malachite_consensus::SignedConsensusMsg;
use malachite_core_types::{Context, SignedProposal, SignedVote};
use malachite_gossip_consensus::handle::CtrlHandle;
use malachite_gossip_consensus::{Channel, Config, Event, Multiaddr, PeerId};
use malachite_metrics::SharedRegistry;

use crate::consensus::ConsensusCodec;
use crate::sync::SyncCodec;
use crate::util::streaming::StreamMessage;

pub type GossipConsensusRef<Ctx> = ActorRef<Msg<Ctx>>;
pub type GossipConsensusMsg<Ctx> = Msg<Ctx>;

pub struct GossipConsensus<Ctx, Codec> {
    codec: Codec,
    span: tracing::Span,
    marker: PhantomData<Ctx>,
}

impl<Ctx, Codec> GossipConsensus<Ctx, Codec> {
    pub fn new(codec: Codec, span: tracing::Span) -> Self {
        Self {
            codec,
            span,
            marker: PhantomData,
        }
    }
}

impl<Ctx, Codec> GossipConsensus<Ctx, Codec>
where
    Ctx: Context,
    Codec: ConsensusCodec<Ctx>,
    Codec: SyncCodec<Ctx>,
{
    pub async fn spawn(
        keypair: Keypair,
        config: Config,
        metrics: SharedRegistry,
        codec: Codec,
        span: tracing::Span,
    ) -> Result<ActorRef<Msg<Ctx>>, ractor::SpawnErr> {
        let args = Args {
            keypair,
            config,
            metrics,
        };

        let (actor_ref, _) = Actor::spawn(None, Self::new(codec, span), args).await?;
        Ok(actor_ref)
    }
}

pub struct Args {
    pub keypair: Keypair,
    pub config: Config,
    pub metrics: SharedRegistry,
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum GossipEvent<Ctx: Context> {
    Listening(Multiaddr),

    PeerConnected(PeerId),
    PeerDisconnected(PeerId),

    Vote(PeerId, SignedVote<Ctx>),

    Proposal(PeerId, SignedProposal<Ctx>),
    ProposalPart(PeerId, StreamMessage<Ctx::ProposalPart>),

    Status(PeerId, Status<Ctx>),

    Request(InboundRequestId, PeerId, Request<Ctx>),
    Response(OutboundRequestId, PeerId, Response<Ctx>),
}

pub enum State<Ctx: Context> {
    Stopped,
    Running {
        peers: BTreeSet<PeerId>,
        output_port: OutputPort<GossipEvent<Ctx>>,
        ctrl_handle: CtrlHandle,
        recv_task: JoinHandle<()>,
        inbound_requests: HashMap<InboundRequestId, request_response::InboundRequestId>,
    },
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct Status<Ctx: Context> {
    pub height: Ctx::Height,
    pub history_min_height: Ctx::Height,
}

impl<Ctx: Context> Status<Ctx> {
    pub fn new(height: Ctx::Height, history_min_height: Ctx::Height) -> Self {
        Self {
            height,
            history_min_height,
        }
    }
}

pub enum Msg<Ctx: Context> {
    /// Subscribe this actor to receive gossip events
    Subscribe(OutputPortSubscriber<GossipEvent<Ctx>>),

    /// Publish a signed consensus message
    Publish(SignedConsensusMsg<Ctx>),

    /// Publish a proposal part
    PublishProposalPart(StreamMessage<Ctx::ProposalPart>),

    /// Broadcast status to all direct peers
    BroadcastStatus(Status<Ctx>),

    /// Send a request to a peer, returning the outbound request ID
    OutgoingRequest(PeerId, Request<Ctx>, RpcReplyPort<OutboundRequestId>),

    /// Send a response for a request to a peer
    OutgoingResponse(InboundRequestId, Response<Ctx>),

    /// Request for number of peers from gossip
    GetState { reply: RpcReplyPort<usize> },

    // Event emitted by the gossip layer
    #[doc(hidden)]
    NewEvent(Event),
}

#[async_trait]
impl<Ctx, Codec> Actor for GossipConsensus<Ctx, Codec>
where
    Ctx: Context,
    Codec: Send + Sync + 'static,
    Codec: codec::Codec<Ctx::ProposalPart>,
    Codec: codec::Codec<SignedConsensusMsg<Ctx>>,
    Codec: codec::Codec<StreamMessage<Ctx::ProposalPart>>,
    Codec: codec::Codec<sync::Status<Ctx>>,
    Codec: codec::Codec<sync::Request<Ctx>>,
    Codec: codec::Codec<sync::Response<Ctx>>,
{
    type Msg = Msg<Ctx>;
    type State = State<Ctx>;
    type Arguments = Args;

    async fn pre_start(
        &self,
        myself: ActorRef<Msg<Ctx>>,
        args: Args,
    ) -> Result<Self::State, ActorProcessingErr> {
        let handle =
            malachite_gossip_consensus::spawn(args.keypair, args.config, args.metrics).await?;

        let (mut recv_handle, ctrl_handle) = handle.split();

        let recv_task = tokio::spawn(async move {
            while let Some(event) = recv_handle.recv().await {
                if let Err(e) = myself.cast(Msg::NewEvent(event)) {
                    error!("Actor has died, stopping gossip consensus: {e:?}");
                    break;
                }
            }
        });

        Ok(State::Running {
            peers: BTreeSet::new(),
            output_port: OutputPort::default(),
            ctrl_handle,
            recv_task,
            inbound_requests: HashMap::new(),
        })
    }

    async fn post_start(
        &self,
        _myself: ActorRef<Msg<Ctx>>,
        _state: &mut State<Ctx>,
    ) -> Result<(), ActorProcessingErr> {
        Ok(())
    }

    #[tracing::instrument(name = "gossip.consensus", parent = &self.span, skip_all)]
    async fn handle(
        &self,
        _myself: ActorRef<Msg<Ctx>>,
        msg: Msg<Ctx>,
        state: &mut State<Ctx>,
    ) -> Result<(), ActorProcessingErr> {
        let State::Running {
            peers,
            output_port,
            ctrl_handle,
            inbound_requests,
            ..
        } = state
        else {
            return Ok(());
        };

        match msg {
            Msg::Subscribe(subscriber) => subscriber.subscribe_to_port(output_port),

            Msg::Publish(msg) => match self.codec.encode(&msg) {
                Ok(data) => ctrl_handle.publish(Channel::Consensus, data).await?,
                Err(e) => error!("Failed to encode gossip message: {e:?}"),
            },

            Msg::PublishProposalPart(msg) => {
                trace!(
                    stream_id = %msg.stream_id,
                    sequence = %msg.sequence,
                    "Broadcasting proposal part"
                );

                let data = self.codec.encode(&msg);
                match data {
                    Ok(data) => ctrl_handle.publish(Channel::ProposalParts, data).await?,
                    Err(e) => error!("Failed to encode proposal part: {e:?}"),
                }
            }

            Msg::BroadcastStatus(status) => {
                let status = sync::Status {
                    peer_id: ctrl_handle.peer_id(),
                    height: status.height,
                    history_min_height: status.history_min_height,
                };

                let data = self.codec.encode(&status);
                match data {
                    Ok(data) => ctrl_handle.broadcast(Channel::Sync, data).await?,
                    Err(e) => error!("Failed to encode status message: {e:?}"),
                }
            }

            Msg::OutgoingRequest(peer_id, request, reply_to) => {
                let request = self.codec.encode(&request);

                match request {
                    Ok(data) => {
                        let p2p_request_id = ctrl_handle.sync_request(peer_id, data).await?;
                        reply_to.send(OutboundRequestId::new(p2p_request_id))?;
                    }
                    Err(e) => error!("Failed to encode request message: {e:?}"),
                }
            }

            Msg::OutgoingResponse(request_id, response) => {
                let response = self.codec.encode(&response);

                match response {
                    Ok(data) => {
                        let request_id = inbound_requests
                            .remove(&request_id)
                            .ok_or_else(|| eyre!("Unknown inbound request ID: {request_id}"))?;

                        ctrl_handle.sync_reply(request_id, data).await?
                    }
                    Err(e) => {
                        error!(%request_id, "Failed to encode response message: {e:?}");
                        return Ok(());
                    }
                };
            }

            Msg::NewEvent(Event::Listening(addr)) => {
                output_port.send(GossipEvent::Listening(addr));
            }

            Msg::NewEvent(Event::PeerConnected(peer_id)) => {
                peers.insert(peer_id);
                output_port.send(GossipEvent::PeerConnected(peer_id));
            }

            Msg::NewEvent(Event::PeerDisconnected(peer_id)) => {
                peers.remove(&peer_id);
                output_port.send(GossipEvent::PeerDisconnected(peer_id));
            }

            Msg::NewEvent(Event::Message(Channel::Consensus, from, data)) => {
                let msg = match self.codec.decode(data) {
                    Ok(msg) => msg,
                    Err(e) => {
                        error!(%from, "Failed to decode gossip message: {e:?}");
                        return Ok(());
                    }
                };

                let event = match msg {
                    SignedConsensusMsg::Vote(vote) => GossipEvent::Vote(from, vote),
                    SignedConsensusMsg::Proposal(proposal) => GossipEvent::Proposal(from, proposal),
                };

                output_port.send(event);
            }

            Msg::NewEvent(Event::Message(Channel::ProposalParts, from, data)) => {
                let msg: StreamMessage<Ctx::ProposalPart> = match self.codec.decode(data) {
                    Ok(stream_msg) => stream_msg,
                    Err(e) => {
                        error!(%from, "Failed to decode stream message: {e:?}");
                        return Ok(());
                    }
                };

                trace!(
                    %from,
                    stream_id = %msg.stream_id,
                    sequence = %msg.sequence,
                    "Received proposal part"
                );

                output_port.send(GossipEvent::ProposalPart(from, msg));
            }

            Msg::NewEvent(Event::Message(Channel::Sync, from, data)) => {
                let status: sync::Status<Ctx> = match self.codec.decode(data) {
                    Ok(status) => status,
                    Err(e) => {
                        error!(%from, "Failed to decode status message: {e:?}");
                        return Ok(());
                    }
                };

                if from != status.peer_id {
                    error!(%from, %status.peer_id, "Mismatched peer ID in status message");
                    return Ok(());
                }

                trace!(%from, height = %status.height, "Received status");

                output_port.send(GossipEvent::Status(
                    status.peer_id,
                    Status::new(status.height, status.history_min_height),
                ));
            }

            Msg::NewEvent(Event::Sync(raw_msg)) => match raw_msg {
                RawMessage::Request {
                    request_id,
                    peer,
                    body,
                } => {
                    let request: sync::Request<Ctx> = match self.codec.decode(body) {
                        Ok(request) => request,
                        Err(e) => {
                            error!(%peer, "Failed to decode sync request: {e:?}");
                            return Ok(());
                        }
                    };

                    inbound_requests.insert(InboundRequestId::new(request_id), request_id);

                    output_port.send(GossipEvent::Request(
                        InboundRequestId::new(request_id),
                        peer,
                        request,
                    ));
                }

                RawMessage::Response {
                    request_id,
                    peer,
                    body,
                } => {
                    let response: sync::Response<Ctx> = match self.codec.decode(body) {
                        Ok(response) => response,
                        Err(e) => {
                            error!(%peer, "Failed to decode sync response: {e:?}");
                            return Ok(());
                        }
                    };

                    output_port.send(GossipEvent::Response(
                        OutboundRequestId::new(request_id),
                        peer,
                        response,
                    ));
                }
            },

            Msg::GetState { reply } => {
                let number_peers = match state {
                    State::Stopped => 0,
                    State::Running { peers, .. } => peers.len(),
                };
                reply.send(number_peers)?;
            }
        }

        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Msg<Ctx>>,
        state: &mut State<Ctx>,
    ) -> Result<(), ActorProcessingErr> {
        let state = std::mem::replace(state, State::Stopped);

        if let State::Running {
            ctrl_handle,
            recv_task,
            ..
        } = state
        {
            ctrl_handle.wait_shutdown().await?;
            recv_task.await?;
        }

        Ok(())
    }
}
