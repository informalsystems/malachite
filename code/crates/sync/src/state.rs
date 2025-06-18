use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use libp2p::StreamProtocol;
use std::time::Duration;

use malachitebft_core_types::{Context, Height};
use malachitebft_peer::PeerId;
use tracing::warn;

use crate::scoring::{PeerScorer, ScoringStrategy};
use crate::{Behaviour, OutboundRequestId, PeerDetails, PeerKind, Status};

pub struct State<Ctx>
where
    Ctx: Context,
{
    rng: Box<dyn rand::RngCore + Send>,

    /// Consensus has started
    pub started: bool,

    /// Height of last decided value
    pub tip_height: Ctx::Height,

    /// Height currently syncing.
    /// If syncing in batches, this is first height in the batch being synced.
    pub sync_height: Ctx::Height,

    /// Decided value requests for these heights have been sent out to peers.
    pub pending_value_requests: BTreeMap<Ctx::Height, BTreeSet<OutboundRequestId>>,

    /// Maps request ID to range of heights for pending decided value requests.
    pub height_range_per_request_id: BTreeMap<OutboundRequestId, RangeInclusive<Ctx::Height>>,

    /// The set of peers we are connected to in order to get values, certificates and votes.
    pub peers: BTreeMap<PeerId, PeerDetails<Ctx>>,

    /// Peer scorer for scoring peers based on their performance.
    pub peer_scorer: PeerScorer,

    /// Threshold for considering a peer inactive, and their score reset to the initial value.
    pub inactive_threshold: Option<Duration>,
}

impl<Ctx> State<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        // Random number generator for selecting peers
        rng: Box<dyn rand::RngCore + Send>,
        // Strategy for scoring peers based on their performance
        scoring_strategy: impl ScoringStrategy + 'static,
        // Threshold for considering a peer inactive, and their score reset to the initial value
        inactive_threshold: Option<Duration>,
    ) -> Self {
        Self {
            rng,
            started: false,
            tip_height: Ctx::Height::ZERO,
            sync_height: Ctx::Height::ZERO,
            pending_value_requests: BTreeMap::new(),
            height_range_per_request_id: BTreeMap::new(),
            peers: BTreeMap::new(),
            peer_scorer: PeerScorer::new(scoring_strategy),
            inactive_threshold,
        }
    }

    pub fn add_peer(&mut self, peer_id: PeerId, protocols: Vec<StreamProtocol>) {
        if self.peers.contains_key(&peer_id) {
            warn!("Peer {} already exists in the state", peer_id);
            return;
        }

        let kind = if protocols.contains(&Behaviour::SYNC_V2_PROTOCOL.0) {
            PeerKind::SyncV2
        } else if protocols.contains(&Behaviour::SYNC_V1_PROTOCOL.0) {
            PeerKind::SyncV1
        } else {
            warn!("Peer {} does not support any known sync protocol", peer_id);
            return;
        };

        self.peers.insert(
            peer_id,
            PeerDetails {
                kind,
                status: Status::default(peer_id),
            },
        );
    }

    pub fn update_status(&mut self, status: Status<Ctx>) {
        // TODO: should we consider status messages from non connected peers?
        if let Some(peer_details) = self.peers.get_mut(&status.peer_id) {
            peer_details.update_status(status);
        }
    }

    /// Same as [`Self::random_peer_with_tip_at_or_above_except`] without excluding any peer.
    pub fn random_peer_with_tip_at_or_above(&mut self, height: Ctx::Height) -> Option<PeerId>
    where
        Ctx: Context,
    {
        self.random_peer_with_tip_at_or_above_except(height, None)
    }

    /// Select at random a peer whose tip is at or above the given height and with min height below the given height.
    /// In other words, `height` is in `status.history_min_height..=status.tip_height` range.
    /// Exclude the given peer if provided.
    pub fn random_peer_with_tip_at_or_above_except(
        &mut self,
        height: Ctx::Height,
        except: Option<PeerId>,
    ) -> Option<PeerId> {
        let peers_at_later_height = self
            .peers
            .iter()
            .filter(|(_, detail)| {
                (detail.status.history_min_height..=detail.status.tip_height).contains(&height)
            })
            .filter(|(&peer, _)| except.is_some_and(|x| x == peer))
            .collect::<Vec<_>>();

        let (v2_peers, v1_peers): (Vec<_>, Vec<_>) = peers_at_later_height
            .iter()
            .partition(|(_, detail)| detail.kind == PeerKind::SyncV2);

        // Prefer peers with higher sync version
        let peers = if !v2_peers.is_empty() {
            v2_peers
        } else {
            v1_peers
        };
        let peer_ids = peers.iter().map(|(&peer, _)| peer).collect::<Vec<_>>();

        self.peer_scorer.select_peer(&peer_ids, &mut self.rng)
    }

    pub fn store_pending_value_request(
        &mut self,
        from: Ctx::Height,
        to: Ctx::Height,
        request_id: OutboundRequestId,
    ) {
        self.height_range_per_request_id
            .insert(request_id.clone(), from..=to);

        let mut height = from;
        loop {
            self.pending_value_requests
                .entry(height)
                .or_default()
                .insert(request_id.clone());
            if height >= to {
                break;
            }
            height = height.increment();
        }
    }

    /// Remove all pending decided value requests for a given height.
    pub fn remove_pending_value_request_by_height(&mut self, height: &Ctx::Height) {
        if let Some(request_ids) = self.pending_value_requests.remove(height) {
            for request_id in request_ids {
                self.height_range_per_request_id.remove(&request_id);
            }
        }
    }

    /// Remove all pending decided value requests for a given range of heights.
    pub fn remove_pending_value_request_by_height_range(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
    ) {
        let mut height = *range.start();
        loop {
            self.remove_pending_value_request_by_height(&height);
            if height >= *range.end() {
                break;
            }
            height = height.increment();
        }
    }

    /// Remove a pending decided value request by its ID and return the height range it was associated with.
    pub fn remove_pending_value_request_by_id(
        &mut self,
        request_id: &OutboundRequestId,
    ) -> Option<RangeInclusive<Ctx::Height>> {
        let range = self.height_range_per_request_id.remove(request_id)?;

        let mut height = *range.start();
        loop {
            if let Some(request_ids) = self.pending_value_requests.get_mut(&height) {
                request_ids.remove(request_id);

                // If there are no more requests for this height, remove the entry
                if request_ids.is_empty() {
                    self.pending_value_requests.remove(&height);
                }

                if height >= *range.end() {
                    break;
                }
                height = height.increment();
            } else {
                break;
            }
        }

        Some(range)
    }

    /// Check if there are any pending decided value requests for a given height.
    pub fn has_pending_value_request(&self, height: &Ctx::Height) -> bool {
        self.pending_value_requests
            .get(height)
            .is_some_and(|ids| !ids.is_empty())
    }

    pub fn requested_batch_size(&self, request_id: &OutboundRequestId) -> Option<usize> {
        self.height_range_per_request_id
            .get(request_id)
            .map(|range| (range.end().as_u64() - range.start().as_u64() + 1) as usize)
    }
}
