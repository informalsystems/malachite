use std::cmp::max;
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

use libp2p::StreamProtocol;

use malachitebft_core_types::{Context, Height};
use malachitebft_peer::PeerId;
use tracing::warn;

use crate::scoring::{ema, PeerScorer, Strategy};
use crate::{Behaviour, Config, OutboundRequestId, PeerInfo, PeerKind, Status};

pub struct State<Ctx>
where
    Ctx: Context,
{
    rng: Box<dyn rand::RngCore + Send>,

    /// Configuration for the sync state and behaviour.
    pub config: Config,

    /// Consensus has started
    pub started: bool,

    /// Height of last decided value
    pub tip_height: Ctx::Height,

    /// Next height to send a sync request.
    /// Invariant: `sync_height > tip_height`
    pub sync_height: Ctx::Height,

    /// The pending requests and their state.
    pub pending_requests: PendingRequests<Ctx>,

    /// The set of peers we are connected to in order to get values, certificates and votes.
    pub peers: BTreeMap<PeerId, PeerInfo<Ctx>>,

    /// Peer scorer for scoring peers based on their performance.
    pub peer_scorer: PeerScorer,
}

impl<Ctx> State<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        // Random number generator for selecting peers
        rng: Box<dyn rand::RngCore + Send>,
        // Sync configuration
        config: Config,
    ) -> Self {
        let peer_scorer = match config.scoring_strategy {
            Strategy::Ema => PeerScorer::new(ema::ExponentialMovingAverage::default()),
        };

        Self {
            rng,
            config,
            started: false,
            tip_height: Ctx::Height::ZERO,
            sync_height: Ctx::Height::ZERO,
            pending_requests: PendingRequests::new(),
            peers: BTreeMap::new(),
            peer_scorer,
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
            PeerInfo {
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

    /// Select at random a peer that can provide the given range of values, while excluding the given peer if provided.
    ///
    /// If there is no peer with all requested values, select a peer that has a tip at or above the start of the range.
    /// Prefer peers that support batching (v2 sync protocol).
    /// Return the peer ID and the range of heights that the peer can provide.
    pub fn random_peer_with_except(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
        except: Option<PeerId>,
    ) -> Option<(PeerId, RangeInclusive<Ctx::Height>)> {
        // Peers that support batching (v2 sync protocol).
        let v2_peers = self
            .peers
            .iter()
            .filter(|(&peer, _)| except.is_none() || except.is_some_and(|p| p != peer))
            .filter(|(_, detail)| detail.kind == PeerKind::SyncV2);

        // Peers that support batching and can provide the whole range of values.
        let v2_peers_with_whole_range = v2_peers
            .clone()
            .filter(|(_, detail)| {
                detail.status.history_min_height <= *range.start()
                    && *range.end() <= detail.status.tip_height
            })
            .map(|(peer, _)| (peer.clone(), range.clone()))
            .collect::<HashMap<_, _>>();

        // Prefer peers that have the whole range of values in their history.
        let v2_peers_with_range = if !v2_peers_with_whole_range.is_empty() {
            v2_peers_with_whole_range
        } else {
            // Otherwise, find peers that have a tip at or above the start of the range.
            v2_peers
                .filter(|(_, detail)| detail.status.history_min_height <= *range.start())
                .map(|(peer, detail)| (peer.clone(), *range.start()..=detail.status.tip_height))
                .filter(|(_, range)| !range.is_empty())
                .collect::<HashMap<_, _>>()
        };

        // Prefer peers with a higher version of the sync protocol.
        let peers_with_range = if !v2_peers_with_range.is_empty() {
            v2_peers_with_range
        } else {
            // Fallback to v1 peers that have a tip at or above the start of the range.
            self.peers
                .iter()
                .filter(|(&peer, _)| except.is_none() || except.is_some_and(|p| p != peer))
                .filter(|(_, detail)| detail.kind == PeerKind::SyncV1)
                .map(|(peer, _)| (peer.clone(), *range.start()..=*range.start()))
                .collect::<HashMap<_, _>>()
        };

        // Select a peer at random.
        let peer_ids = peers_with_range.keys().cloned().collect::<Vec<_>>();
        self.peer_scorer
            .select_peer(&peer_ids, &mut self.rng)
            .map(|peer_id| (peer_id, peers_with_range.get(&peer_id).unwrap().clone()))
    }

    /// Same as [`Self::random_peer_with_except`] but without excluding any peer.
    pub fn random_peer_with(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
    ) -> Option<(PeerId, RangeInclusive<Ctx::Height>)>
    where
        Ctx: Context,
    {
        self.random_peer_with_except(range, None)
    }

    /// Return a new range of heights, trimming from the beginning any height
    /// that is not waiting for a response (because consensus or a peer is or has
    /// already validated it).
    pub fn trim_validated_heights(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
    ) -> RangeInclusive<Ctx::Height> {
        // Skip heights validated by consensus.
        let mut start = max(self.tip_height.increment(), *range.start());

        // Skip heights that are being or have been validated.
        while start <= *range.end() && self.pending_requests.is_being_or_has_been_validated(&start)
        {
            start = start.increment();
        }
        start..=*range.end()
    }

    /// Update the next height to sync to the given height.
    pub fn update_sync_height_to(&mut self, height: Ctx::Height) {
        self.sync_height = max(self.sync_height, height.increment());
    }
}

/// State of a single requested value.
///
/// State transitions: WaitingResponse -> WaitingValidation -> Validated
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestedValueState {
    /// Initial state: waiting for a response from a peer
    WaitingResponse,
    /// Response received: waiting for value validation by consensus
    WaitingValidation,
    /// Value validated by consensus
    Validated,
}

/// A pending request is a collection of requested values in a certain range of heights.
/// A requested value at a certain height is waiting for a response from a peer or validation by consensus.
pub struct PendingRequests<Ctx>
where
    Ctx: Context,
{
    /// The range of heights for requested values for each pending request.
    pub ranges_per_request: BTreeMap<OutboundRequestId, RangeInclusive<Ctx::Height>>,

    /// The state and pending request corresponding to each height.
    pub height_states: BTreeMap<Ctx::Height, (OutboundRequestId, RequestedValueState)>,

    /// The number of non-validated heights on each pending request.
    pub heights_pending_validation_per_request: BTreeMap<OutboundRequestId, u64>,
}

impl<Ctx> PendingRequests<Ctx>
where
    Ctx: Context,
{
    pub fn new() -> Self {
        Self {
            ranges_per_request: BTreeMap::new(),
            height_states: BTreeMap::new(),
            heights_pending_validation_per_request: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> u64 {
        self.ranges_per_request.len() as u64
    }

    /// Store a pending decided value request for a given range of heights and request ID.
    ///
    /// State transition: None -> WaitingResponse
    pub fn store_request(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
        request_id: &OutboundRequestId,
    ) {
        self.ranges_per_request
            .insert(request_id.clone(), range.clone());

        let range_len = range.end().as_u64() - range.start().as_u64() + 1;
        self.heights_pending_validation_per_request
            .insert(request_id.clone(), range_len);

        // Insert one entry per height in the range.
        let mut height = *range.start();
        while height <= *range.end() {
            self.height_states
                .entry(height)
                .or_insert_with(|| (request_id.clone(), RequestedValueState::WaitingResponse));
            height = height.increment();
        }
    }

    /// Mark that all values in the range have been received from a peer.
    ///
    /// State transition: WaitingResponse -> WaitingValidation
    pub fn mark_values_as_received(
        &mut self,
        request_id: &OutboundRequestId,
        range: RangeInclusive<Ctx::Height>,
    ) {
        // Start from the last height in the range; the first ones may have been
        // validated by consensus (thus removed from state).
        let mut height = *range.end();
        while height >= *range.start() {
            if let Some((req_id, state)) = self.height_states.get_mut(&height) {
                if *req_id != *request_id {
                    // A new request for this height has been made in the meantime, ignore this response.
                    return;
                }
                if *state == RequestedValueState::WaitingResponse {
                    *state = RequestedValueState::WaitingValidation;
                }
            } else {
                break;
            }
            height = height.decrement().unwrap_or_default();
        }
    }

    /// Mark that a value has been validated by consensus.
    ///
    /// State transition: * -> Validated
    pub fn mark_value_as_validated(&mut self, height: Ctx::Height) {
        if let Some((request_id, state)) = self.height_states.get_mut(&height) {
            *state = RequestedValueState::Validated;

            // Decrease counter: if it reaches 0, the whole request will be removed.
            let request_id_clone = request_id.clone();
            self.decrease_pending_validation_counter(&request_id_clone);
        }
    }

    /// Decrease the number of non-validated heights for the request.
    /// If this value reaches 0, remove the whole request to free a place for a new one.
    fn decrease_pending_validation_counter(&mut self, request_id: &OutboundRequestId) {
        let num_heights = self
            .heights_pending_validation_per_request
            .get_mut(&request_id)
            .unwrap();
        *num_heights -= 1;

        if *num_heights == 0 {
            self.remove(request_id);
        }
    }

    /// Remove the request and all associated data.
    /// Return the range of heights that was associated with the request.
    pub fn remove(
        &mut self,
        request_id: &OutboundRequestId,
    ) -> Option<RangeInclusive<Ctx::Height>> {
        if let Some(range) = self.ranges_per_request.remove(&request_id) {
            self.heights_pending_validation_per_request
                .remove(&request_id);

            // Remove pending individual states for all heights in the range.
            let mut height = *range.start();
            while height <= *range.end() {
                self.height_states.remove(&height);
                height = height.increment();
            }

            Some(range)
        } else {
            None
        }
    }

    fn is_being_or_has_been_validated(&self, height: &Ctx::Height) -> bool {
        if let Some((_, state)) = self.height_states.get(height) {
            *state == RequestedValueState::WaitingValidation
                || *state == RequestedValueState::Validated
        } else {
            false
        }
    }

    pub fn get_id_by_height(&self, height: &Ctx::Height) -> Option<OutboundRequestId> {
        self.height_states
            .get(height)
            .map(|(request_id, _)| request_id.clone())
    }

    pub fn get_requested_range_by_id(
        &self,
        request_id: &OutboundRequestId,
    ) -> Option<&RangeInclusive<Ctx::Height>> {
        self.ranges_per_request.get(request_id)
    }
}
