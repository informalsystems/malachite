use libp2p::{
    core::ConnectedPoint,
    swarm::{ConnectionId, DialError},
    PeerId, Swarm,
};
use tracing::{debug, error, warn};

use crate::{controller::PeerData, dial::DialData, Discovery, DiscoveryClient};

impl<C> Discovery<C>
where
    C: DiscoveryClient,
{
    pub fn can_dial(&self) -> bool {
        self.controller.dial.can_perform()
    }

    fn should_dial(
        &self,
        swarm: &Swarm<C>,
        dial_data: &DialData,
        check_already_dialed: bool,
    ) -> bool {
        dial_data.peer_id().as_ref().is_none_or(|id| {
            // Is not itself (peer id)
            id != swarm.local_peer_id()
            // Is not already connected
            && !swarm.is_connected(id)
        })
            // Has not already dialed, or has dialed but retries are allowed
            && (!check_already_dialed || !self.controller.dial_is_done_on(dial_data) || dial_data.retry.count() != 0)
            // Is not itself (listen addresses)
            && !swarm.listeners().any(|addr| dial_data.listen_addrs().contains(addr))
    }

    pub fn dial_peer(&mut self, swarm: &mut Swarm<C>, dial_data: DialData) {
        // Not checking if the peer was already dialed because it is done when
        // adding to the dial queue
        if !self.should_dial(swarm, &dial_data, false) {
            return;
        }

        let Some(dial_opts) = dial_data.build_dial_opts() else {
            warn!(
                "No addresses to dial for peer {:?}, skipping dial attempt",
                dial_data.peer_id()
            );
            return;
        };
        let connection_id = dial_opts.connection_id();

        self.controller.dial_register_done_on(&dial_data);

        self.controller
            .dial
            .register_in_progress(connection_id, dial_data.clone());

        // Do not count retries as new interactions
        if dial_data.retry.count() == 0 {
            self.metrics.increment_total_dials();
        }

        debug!(
            %connection_id,
            "Dialing peer {:?} at {:?}, retry #{}",
            dial_data.peer_id(),
            dial_data.listen_addrs(),
            dial_data.retry.count()
        );

        if let Err(e) = swarm.dial(dial_opts) {
            error!(
                %connection_id,
                "Error dialing peer {:?} at {:?}: {}",
                dial_data.peer_id(),
                dial_data.listen_addrs(),
                e
            );

            self.handle_failed_connection(swarm, connection_id, e);
        }
    }

    pub fn handle_connection(
        &mut self,
        swarm: &mut Swarm<C>,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        match endpoint {
            ConnectedPoint::Dialer { .. } => {
                debug!(peer = %peer_id, %connection_id, "Connected to peer");
            }
            ConnectedPoint::Listener { .. } => {
                debug!(peer = %peer_id, %connection_id, "Accepted incoming connection from peer");
            }
        }

        // Needed in case the peer was dialed without knowing the peer id
        self.controller
            .dial
            .register_done_on(PeerData::PeerId(peer_id));

        // This check is necessary to handle the case where two
        // nodes dial each other at the same time, which can lead
        // to a connection established (dialer) event for one node
        // after the connection established (listener) event on the
        // same node. Hence it is possible that the peer was already
        // added to the active connections.
        if self.active_connections.contains_key(&peer_id) {
            self.controller.dial.remove_in_progress(&connection_id);
            // Trigger potential extension step
            self.make_extension_step(swarm);
            return;
        }

        // Needed in case the peer was dialed without knowing the peer id
        self.controller
            .dial_add_peer_id_to_dial_data(connection_id, peer_id);
    }

    pub fn handle_failed_connection(
        &mut self,
        swarm: &mut Swarm<C>,
        connection_id: ConnectionId,
        error: DialError,
    ) {
        if let Some(mut dial_data) = self.controller.dial.remove_in_progress(&connection_id) {
            // Skip retrying for errors that will occur again
            if matches!(
                error,
                DialError::LocalPeerId { .. }
                    | DialError::NoAddresses
                    | DialError::WrongPeerId { .. }
            ) {
                self.make_extension_step(swarm);
                return;
            }

            if dial_data.retry.count() < self.config.dial_max_retries {
                // Retry dialing after a delay
                dial_data.retry.inc_count();

                let next_delay = dial_data.retry.next_delay();

                self.controller
                    .dial
                    .add_to_queue(dial_data.clone(), Some(next_delay));
            } else {
                // No more trials left
                error!(
                    "Failed to dial peer {:?} at {:?} after {} trials",
                    dial_data.peer_id(),
                    dial_data.listen_addrs(),
                    dial_data.retry.count(),
                );

                self.metrics.increment_total_failed_dials();

                // For bootstrap nodes, clear the done_on flag so they can be retried
                // by the periodic timer. We check and clear by address since bootstrap
                // nodes may not have peer_id
                let is_bootstrap = self.bootstrap_nodes.iter().any(|(_, addrs)| {
                    dial_data
                        .listen_addrs()
                        .iter()
                        .any(|dial_addr| addrs.contains(dial_addr))
                });

                if is_bootstrap {
                    // Clear done_on by address
                    for addr in dial_data.listen_addrs() {
                        self.controller
                            .dial
                            .remove_done_on(&crate::controller::PeerData::Multiaddr(addr));
                    }
                    debug!(
                        "Cleared dial history for bootstrap node addrs={:?} - will be retried by timer",
                        dial_data.listen_addrs()
                    );
                }

                self.make_extension_step(swarm);
            }
        }
    }

    pub(crate) fn add_to_dial_queue(&mut self, swarm: &Swarm<C>, dial_data: DialData) {
        if self.should_dial(swarm, &dial_data, true) {
            // Already register as dialed address to avoid flooding the dial queue
            // with the same dial attempts.
            self.controller.dial_register_done_on(&dial_data);

            self.controller.dial.add_to_queue(dial_data, None);
        }
    }

    pub fn dial_bootstrap_nodes(&mut self, swarm: &Swarm<C>) {
        for (peer_id, listen_addrs) in &self.bootstrap_nodes.clone() {
            // For bootstrap nodes, check if already attempted (done_on flag)
            // This prevents overlapping Fibonacci retry sequences since done_on is only cleared
            // after all retries are exhausted
            // The Fibonacci retry sequence is started when a connection fails, see handle_failed_connection()
            // We check by address since bootstrap nodes may not have peer_id
            let already_attempted = listen_addrs.iter().any(|addr| {
                self.controller
                    .dial
                    .is_done_on(&crate::controller::PeerData::Multiaddr(addr.clone()))
            });

            if already_attempted {
                continue;
            }

            let dial_data = DialData::new(*peer_id, listen_addrs.clone());

            // For bootstrap nodes, always attempt to dial even if previously failed
            // This ensures persistent peers are retried indefinitely
            if self.should_dial(swarm, &dial_data, false) {
                debug!(
                    "Adding bootstrap node to dial queue: peer_id={:?}, queue_len_before={}, in_progress_len={}",
                    dial_data.peer_id(),
                    self.controller.dial.queue_len(),
                    self.controller.dial.is_idle().1
                );
                self.controller.dial_register_done_on(&dial_data);
                self.controller.dial.add_to_queue(dial_data, None);
            }
        }
    }
}
