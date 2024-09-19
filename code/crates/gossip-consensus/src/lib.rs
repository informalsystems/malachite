// For coverage on nightly
#![allow(unexpected_cfgs)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::collections::HashSet;
use std::error::Error;
use std::ops::ControlFlow;
use std::time::Duration;

use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::swarm::{self, dial_opts::DialOpts, SwarmEvent};
use libp2p::{gossipsub, identify, request_response, SwarmBuilder};
use libp2p_broadcast as broadcast;
use tokio::sync::mpsc;
use tracing::{debug, error, error_span, info, trace, Instrument};

use malachite_discovery::{
    behaviour::{ReqResEvent, Request, Response},
    Discovery,
};
use malachite_metrics::SharedRegistry;

pub use bytes::Bytes;
pub use libp2p::gossipsub::MessageId;
pub use libp2p::identity::Keypair;
pub use libp2p::{Multiaddr, PeerId};

pub mod behaviour;
pub mod handle;
pub mod pubsub;

mod channel;
pub use channel::Channel;

use behaviour::{Behaviour, NetworkEvent};
use handle::Handle;

const METRICS_PREFIX: &str = "malachite_gossip_consensus";

#[derive(Copy, Clone, Debug, Default)]
pub enum PubSubProtocol {
    #[default]
    GossipSub,
    Broadcast,
}

impl PubSubProtocol {
    pub fn is_gossipsub(&self) -> bool {
        matches!(self, Self::GossipSub)
    }

    pub fn is_broadcast(&self) -> bool {
        matches!(self, Self::Broadcast)
    }
}

const PROTOCOL_VERSION: &str = "/malachite-gossip-consensus/v1beta1";

pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: Multiaddr,
    pub persistent_peers: Vec<Multiaddr>,
    pub enable_discovery: bool,
    pub idle_connection_timeout: Duration,
    pub protocol: PubSubProtocol,
}

impl Config {
    fn apply(&self, cfg: swarm::Config) -> swarm::Config {
        cfg.with_idle_connection_timeout(self.idle_connection_timeout)
    }
}

/// An event that can be emitted by the gossip layer
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Listening(Multiaddr),
    Message(Channel, PeerId, Bytes),
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
}

#[derive(Debug)]
pub enum CtrlMsg {
    BroadcastMsg(Channel, Bytes),
    Shutdown,
}

#[derive(Debug)]
pub struct State {
    pub discovery: Discovery,
}

impl State {
    fn new(enable_discovery: bool) -> Self {
        State {
            discovery: Discovery::new(enable_discovery),
        }
    }
}

pub async fn spawn(
    keypair: Keypair,
    config: Config,
    registry: SharedRegistry,
) -> Result<Handle, BoxError> {
    let swarm = registry.with_prefix(METRICS_PREFIX, |registry| -> Result<_, BoxError> {
        Ok(SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_quic()
            .with_dns()?
            .with_bandwidth_metrics(registry)
            .with_behaviour(|kp| {
                Behaviour::new_with_metrics(config.protocol, kp, config.enable_discovery, registry)
            })?
            .with_swarm_config(|cfg| config.apply(cfg))
            .build())
    })?;

    let metrics = registry.with_prefix(METRICS_PREFIX, Metrics::new);

    let (tx_event, rx_event) = mpsc::channel(32);
    let (tx_ctrl, rx_ctrl) = mpsc::channel(32);

    let peer_id = swarm.local_peer_id();
    let span = error_span!("gossip.consensus", peer = %peer_id);
    let task_handle =
        tokio::task::spawn(run(config, metrics, swarm, rx_ctrl, tx_event).instrument(span));

    Ok(Handle::new(tx_ctrl, rx_event, task_handle))
}

async fn run(
    config: Config,
    metrics: Metrics,
    mut swarm: swarm::Swarm<Behaviour>,
    mut rx_ctrl: mpsc::Receiver<CtrlMsg>,
    tx_event: mpsc::Sender<Event>,
) {
    if let Err(e) = swarm.listen_on(config.listen_addr.clone()) {
        error!("Error listening on {}: {e}", config.listen_addr);
        return;
    };

    let mut state = State::new(config.enable_discovery);

    info!(
        "Discovery is {}",
        if state.discovery.is_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    for persistent_peer in config.persistent_peers {
        trace!("Dialing persistent peer: {persistent_peer}");

        let dial_opts = DialOpts::unknown_peer_id()
            .address(persistent_peer.clone())
            .build();
        let connection_id = dial_opts.connection_id();

        if state.discovery.is_enabled {
            state
                .discovery
                .dialed_multiaddrs
                .insert(persistent_peer.clone());
            state.discovery.pending_connections.insert(connection_id);
            state.discovery.total_interactions += 1;
        }

        if let Err(e) = swarm.dial(dial_opts) {
            error!("Error dialing persistent peer {persistent_peer}: {e}");
            if state.discovery.is_enabled {
                state.discovery.pending_connections.remove(&connection_id);
                state.discovery.total_interactions_failed += 1;
            }
        }

        state.discovery.is_done(); // Done if all persistent peers failed
    }

    pubsub::subscribe(&mut swarm, Channel::all()).unwrap(); // FIXME: unwrap

    loop {
        let result = tokio::select! {
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &metrics, &mut swarm, &mut state, &tx_event).await
            }

            Some(ctrl) = rx_ctrl.recv() => {
                handle_ctrl_msg(ctrl, &mut swarm).await
            }
        };

        match result {
            ControlFlow::Continue(()) => continue,
            ControlFlow::Break(()) => break,
        }
    }
}

async fn handle_ctrl_msg(msg: CtrlMsg, swarm: &mut swarm::Swarm<Behaviour>) -> ControlFlow<()> {
    match msg {
        CtrlMsg::BroadcastMsg(channel, data) => {
            let msg_size = data.len();
            let result = pubsub::publish(swarm, channel, data);

            match result {
                Ok(()) => debug!(%channel, "Broadcasted message ({msg_size} bytes)"),
                Err(e) => error!(%channel, "Error broadcasting message: {e}"),
            }

            ControlFlow::Continue(())
        }

        CtrlMsg::Shutdown => ControlFlow::Break(()),
    }
}

async fn handle_swarm_event(
    event: SwarmEvent<NetworkEvent>,
    metrics: &Metrics,
    swarm: &mut swarm::Swarm<Behaviour>,
    state: &mut State,
    tx_event: &mpsc::Sender<Event>,
) -> ControlFlow<()> {
    if let SwarmEvent::Behaviour(NetworkEvent::GossipSub(e)) = &event {
        metrics.record(e);
    } else if let SwarmEvent::Behaviour(NetworkEvent::Identify(e)) = &event {
        metrics.record(e);
    }

    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            debug!("Node is listening on {address}");

            if let Err(e) = tx_event.send(Event::Listening(address)).await {
                error!("Error sending listening event to handle: {e}");
                return ControlFlow::Break(());
            }
        }

        SwarmEvent::ConnectionEstablished {
            peer_id,
            connection_id,
            endpoint,
            ..
        } => {
            match endpoint {
                ConnectedPoint::Dialer { .. } => {
                    debug!("Connected to {peer_id}");
                    if state.discovery.is_enabled {
                        state.discovery.pending_connections.remove(&connection_id);
                        // This call is necessary to record the peer id of a
                        // bootstrap node (which was unknown before)
                        state.discovery.dialed_peer_ids.insert(peer_id.clone());
                        // This check is necessary to handle the case where two
                        // nodes dial each other at the same time, which can lead
                        // to a connection established (dialer) event for one node
                        // after the connection established (listener) event on the
                        // same node. Hence it is possible that the request for
                        // peers was already sent before this event.
                        if state.discovery.requested_peer_ids.contains(&peer_id) {
                            state.discovery.is_done();
                        }
                    }
                }
                ConnectedPoint::Listener { .. } => {
                    debug!("Accepted incoming connection from {peer_id}");
                }
            }
        }

        SwarmEvent::OutgoingConnectionError {
            connection_id,
            error,
            ..
        } => {
            error!("Error dialing peer: {error}");
            if state.discovery.is_enabled {
                state.discovery.pending_connections.remove(&connection_id);
                state.discovery.total_interactions_failed += 1;
                state.discovery.is_done();
            }
        }

        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            trace!("Connection closed with {peer_id}: {:?}", cause);
            state.discovery.peers.remove(&peer_id);
        }

        SwarmEvent::Behaviour(NetworkEvent::Identify(identify::Event::Sent {
            peer_id, ..
        })) => {
            trace!("Sent identity to {peer_id}");
        }

        SwarmEvent::Behaviour(NetworkEvent::Identify(identify::Event::Received {
            peer_id,
            info,
            ..
        })) => {
            trace!(
                "Received identity from {peer_id}: protocol={:?}",
                info.protocol_version
            );

            if info.protocol_version == PROTOCOL_VERSION {
                trace!(
                    "Peer {peer_id} is using compatible protocol version: {:?}",
                    info.protocol_version
                );

                if state.discovery.is_enabled
                    && !state.discovery.is_done
                    && !state.discovery.peers.contains_key(&peer_id)
                {
                    if let Some(behaviour) = swarm.behaviour_mut().request_response.as_mut() {
                        debug!("Requesting peers from {peer_id}");
                        let request_id = behaviour.send_request(&peer_id, Request::Peers);
                        state.discovery.requested_peer_ids.insert(peer_id.clone());
                        state.discovery.pending_requests.insert(request_id);
                    } else {
                        // This should never happen
                        error!("Discovery is enabled but request-response is not available");
                    }
                }

                state.discovery.peers.insert(peer_id, info);
            } else {
                trace!(
                    "Peer {peer_id} is using incompatible protocol version: {:?}",
                    info.protocol_version
                );
            }
        }

        SwarmEvent::Behaviour(NetworkEvent::Ping(event)) => {
            match &event.result {
                Ok(rtt) => {
                    trace!("Received pong from {} in {rtt:?}", event.peer);
                }
                Err(e) => {
                    trace!("Received pong from {} with error: {e}", event.peer);
                }
            }

            // Record metric for round-trip time sending a ping and receiving a pong
            metrics.record(&event);
        }

        SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqResEvent::Message {
            peer,
            message:
                request_response::Message::Request {
                    request, channel, ..
                },
        })) => {
            match request {
                Request::Peers => {
                    debug!("Received request for peers from {peer}");
                    let peers: HashSet<_> = state
                        .discovery
                        .peers
                        .iter()
                        .filter_map(|(peer_id, info)| {
                            if peer_id != &peer {
                                info.listen_addrs
                                    .get(0)
                                    .map(|addr| (peer_id.clone(), addr.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if let Some(behaviour) = swarm.behaviour_mut().request_response.as_mut() {
                        if behaviour
                            .send_response(channel, Response::Peers(peers))
                            .is_err()
                        {
                            error!("Error sending peers to {peer}");
                        } else {
                            trace!("Sent peers to {peer}");
                        }
                    } else {
                        // This should never happen
                        error!("Request-response behaviour is not available");
                    }
                }
            }
        }

        SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqResEvent::Message {
            peer,
            message:
                request_response::Message::Response {
                    response,
                    request_id,
                    ..
                },
        })) => {
            match response {
                Response::Peers(peers) => {
                    state.discovery.pending_requests.remove(&request_id);
                    debug!("Received {} peers from {peer}", peers.len());
                    // TODO check upper bound on number of peers
                    for (peer_id, listen_addr) in peers {
                        // Skip peers that are already connected or dialed
                        if &peer_id == swarm.local_peer_id()
                            || swarm.is_connected(&peer_id)
                            || state.discovery.dialed_peer_ids.contains(&peer_id)
                            || state.discovery.dialed_multiaddrs.contains(&listen_addr)
                        {
                            continue;
                        }

                        let dial_opts = DialOpts::peer_id(peer_id.clone())
                            .addresses(vec![listen_addr.clone()])
                            .build();
                        let connection_id = dial_opts.connection_id();

                        state.discovery.dialed_peer_ids.insert(peer_id.clone());
                        state
                            .discovery
                            .dialed_multiaddrs
                            .insert(listen_addr.clone());
                        state.discovery.pending_connections.insert(connection_id);
                        state.discovery.total_interactions += 1;

                        if let Err(e) = swarm.dial(dial_opts) {
                            error!("Error dialing peer {peer_id}: {e}");
                            state.discovery.pending_connections.remove(&connection_id);
                            state.discovery.total_interactions_failed += 1;
                        }
                    }
                    state.discovery.is_done();
                }
            }
        }

        SwarmEvent::Behaviour(NetworkEvent::RequestResponse(ReqResEvent::OutboundFailure {
            peer,
            request_id,
            error,
        })) => {
            error!("Outbound request to {peer} failed: {error}");
            state.discovery.pending_requests.remove(&request_id);
            state.discovery.total_interactions_failed += 1;
            state.discovery.is_done();
        }

        SwarmEvent::Behaviour(NetworkEvent::GossipSub(event)) => {
            return handle_gossipsub_event(event, metrics, swarm, state, tx_event).await;
        }

        SwarmEvent::Behaviour(NetworkEvent::Broadcast(event)) => {
            return handle_broadcast_event(event, metrics, swarm, state, tx_event).await;
        }

        swarm_event => {
            metrics.record(&swarm_event);
        }
    }

    ControlFlow::Continue(())
}

async fn handle_gossipsub_event(
    event: gossipsub::Event,
    _metrics: &Metrics,
    _swarm: &mut swarm::Swarm<Behaviour>,
    _state: &mut State,
    tx_event: &mpsc::Sender<Event>,
) -> ControlFlow<()> {
    match event {
        gossipsub::Event::Subscribed { peer_id, topic } => {
            if !Channel::has_gossipsub_topic(&topic) {
                trace!("Peer {peer_id} tried to subscribe to unknown topic: {topic}");
                return ControlFlow::Continue(());
            }

            trace!("Peer {peer_id} subscribed to {topic}");

            if let Err(e) = tx_event.send(Event::PeerConnected(peer_id)).await {
                error!("Error sending peer connected event to handle: {e}");
                return ControlFlow::Break(());
            }
        }

        gossipsub::Event::Unsubscribed { peer_id, topic } => {
            if !Channel::has_gossipsub_topic(&topic) {
                trace!("Peer {peer_id} tried to unsubscribe from unknown topic: {topic}");
                return ControlFlow::Continue(());
            }

            trace!("Peer {peer_id} unsubscribed from {topic}");

            if let Err(e) = tx_event.send(Event::PeerDisconnected(peer_id)).await {
                error!("Error sending peer disconnected event to handle: {e}");
                return ControlFlow::Break(());
            }
        }

        gossipsub::Event::Message {
            message_id,
            message,
            ..
        } => {
            let Some(peer_id) = message.source else {
                return ControlFlow::Continue(());
            };

            let Some(channel) = Channel::from_gossipsub_topic_hash(&message.topic) else {
                trace!(
                    "Received message {message_id} from {peer_id} on different channel: {}",
                    message.topic
                );

                return ControlFlow::Continue(());
            };

            trace!(
                "Received message {message_id} from {peer_id} on channel {channel} of {} bytes",
                message.data.len()
            );

            let event = Event::Message(channel, peer_id, Bytes::from(message.data));

            if let Err(e) = tx_event.send(event).await {
                error!("Error sending message to handle: {e}");
                return ControlFlow::Break(());
            }
        }
        gossipsub::Event::GossipsubNotSupported { peer_id } => {
            trace!("Peer {peer_id} does not support GossipSub");
        }
    }

    ControlFlow::Continue(())
}

async fn handle_broadcast_event(
    event: broadcast::Event,
    _metrics: &Metrics,
    _swarm: &mut swarm::Swarm<Behaviour>,
    _state: &mut State,
    tx_event: &mpsc::Sender<Event>,
) -> ControlFlow<()> {
    match event {
        broadcast::Event::Subscribed(peer_id, topic) => {
            if !Channel::has_broadcast_topic(&topic) {
                trace!("Peer {peer_id} tried to subscribe to unknown topic: {topic:?}");
                return ControlFlow::Continue(());
            }

            trace!("Peer {peer_id} subscribed to {topic:?}");

            if let Err(e) = tx_event.send(Event::PeerConnected(peer_id)).await {
                error!("Error sending peer connected event to handle: {e}");
                return ControlFlow::Break(());
            }
        }

        broadcast::Event::Unsubscribed(peer_id, topic) => {
            if !Channel::has_broadcast_topic(&topic) {
                trace!("Peer {peer_id} tried to unsubscribe from unknown topic: {topic:?}");
                return ControlFlow::Continue(());
            }

            trace!("Peer {peer_id} unsubscribed from {topic:?}");

            if let Err(e) = tx_event.send(Event::PeerDisconnected(peer_id)).await {
                error!("Error sending peer disconnected event to handle: {e}");
                return ControlFlow::Break(());
            }
        }

        broadcast::Event::Received(peer_id, topic, message) => {
            let Some(channel) = Channel::from_broadcast_topic(&topic) else {
                trace!("Received message from {peer_id} on different channel: {topic:?}");
                return ControlFlow::Continue(());
            };

            trace!(
                "Received message from {peer_id} on channel {channel} of {} bytes",
                message.len()
            );

            let event = Event::Message(channel, peer_id, Bytes::copy_from_slice(message.as_ref()));

            if let Err(e) = tx_event.send(event).await {
                error!("Error sending message to handle: {e}");
                return ControlFlow::Break(());
            }
        }
    }

    ControlFlow::Continue(())
}
