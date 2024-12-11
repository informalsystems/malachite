use std::collections::HashSet;

use libp2p::{
    request_response::{OutboundRequestId, ResponseChannel},
    Multiaddr, PeerId, Swarm,
};
use tracing::{error, info, trace};

use crate::{
    behaviour::{self, Response},
    connection::ConnectionData,
    request::RequestData,
    Discovery, DiscoveryClient,
};

impl<C> Discovery<C>
where
    C: DiscoveryClient,
{
    pub fn can_peers_request(&self) -> bool {
        self.controller.peers_request.can_perform()
    }

    fn should_peers_request(&self, request_data: &RequestData) -> bool {
        // Has not already requested, or has requested but retries are allowed
        !self
            .controller
            .peers_request
            .is_done_on(&request_data.peer_id())
            || request_data.retry.count() != 0
    }

    pub fn peers_request_peer(&mut self, swarm: &mut Swarm<C>, request_data: RequestData) {
        if !self.is_enabled() || !self.should_peers_request(&request_data) {
            return;
        }

        self.controller
            .peers_request
            .register_done_on(request_data.peer_id());

        // Do not count retries as new interactions
        if request_data.retry.count() == 0 {
            self.metrics.increment_total_peer_requests();
        }

        info!(
            "Requesting peers from peer {}, retry #{}",
            request_data.peer_id(),
            request_data.retry.count()
        );

        let request_id = swarm.behaviour_mut().send_request(
            &request_data.peer_id(),
            behaviour::Request::Peers(self.get_all_peers_except(request_data.peer_id())),
        );

        self.controller
            .peers_request
            .register_in_progress(request_id, request_data);
    }

    pub(crate) fn handle_peers_request(
        &mut self,
        swarm: &mut Swarm<C>,
        peer: PeerId,
        channel: ResponseChannel<Response>,
        peers: HashSet<(Option<PeerId>, Multiaddr)>,
    ) {
        // Compute the difference between the discovered peers and the requested peers
        // to avoid sending the requesting peer the peers it already knows.
        let peers_difference = self
            .get_all_peers_except(peer)
            .difference(&peers)
            .cloned()
            .collect();

        if swarm
            .behaviour_mut()
            .send_response(channel, behaviour::Response::Peers(peers_difference))
            .is_err()
        {
            error!("Error sending peers to {peer}");
        } else {
            trace!("Sent peers to {peer}");
        }
    }

    pub(crate) fn handle_peers_response(
        &mut self,
        swarm: &mut Swarm<C>,
        request_id: OutboundRequestId,
        peers: HashSet<(Option<PeerId>, Multiaddr)>,
    ) {
        self.controller
            .peers_request
            .remove_in_progress(&request_id);

        self.process_received_peers(swarm, peers);

        self.make_extension_step(swarm);
    }

    pub(crate) fn handle_failed_peers_request(
        &mut self,
        swarm: &mut Swarm<C>,
        request_id: OutboundRequestId,
    ) {
        if let Some(mut request_data) = self
            .controller
            .peers_request
            .remove_in_progress(&request_id)
        {
            if request_data.retry.count() < self.config.request_max_retries {
                // Retry request after a delay
                request_data.retry.inc_count();

                self.controller
                    .peers_request
                    .add_to_queue(request_data.clone(), Some(request_data.retry.next_delay()));
            } else {
                // No more trials left
                error!(
                    "Failed to send peers request to {0} after {1} trials",
                    request_data.peer_id(),
                    request_data.retry.count(),
                );

                self.metrics.increment_total_failed_peer_requests();

                self.make_extension_step(swarm);
            }
        }
    }

    fn process_received_peers(
        &mut self,
        swarm: &mut Swarm<C>,
        peers: HashSet<(Option<PeerId>, Multiaddr)>,
    ) {
        for (peer_id, listen_addr) in peers {
            self.add_to_dial_queue(swarm, ConnectionData::new(peer_id, listen_addr));
        }
    }

    /// Returns all discovered peers, including bootstrap nodes, except the given peer.
    fn get_all_peers_except(&self, peer: PeerId) -> HashSet<(Option<PeerId>, Multiaddr)> {
        let mut remaining_bootstrap_nodes: Vec<_> = self.bootstrap_nodes.clone();

        let mut peers: HashSet<_> = self
            .discovered_peers
            .iter()
            .filter_map(|(peer_id, info)| {
                if peer_id == &peer {
                    // Remove the peer also from the bootstrap nodes (if it is there)
                    if let Some(addr) = info.listen_addrs.first() {
                        remaining_bootstrap_nodes.retain(|(_, x)| x != addr);
                    }

                    return None;
                }

                info.listen_addrs.first().map(|addr| {
                    remaining_bootstrap_nodes.retain(|(_, x)| x != addr);
                    (Some(*peer_id), addr.clone())
                })
            })
            .collect();

        for (peer_id, addr) in remaining_bootstrap_nodes {
            peers.insert((peer_id, addr));
        }

        peers
    }
}