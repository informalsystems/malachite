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

    /// The requested range of heights.
    pub pending_requests: BTreeMap<OutboundRequestId, RangeInclusive<Ctx::Height>>,

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
            pending_requests: BTreeMap::new(),
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
            .filter(|(&peer, _)| except.is_none_or(|p| p != peer))
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
                .filter(|(&peer, detail)| {
                    except.is_none_or(|p| p != peer) && detail.kind == PeerKind::SyncV1
                })
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
    /// that is not validated by consensus.
    pub fn trim_validated_heights(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
    ) -> RangeInclusive<Ctx::Height> {
        let start = max(self.tip_height.increment(), *range.start());
        start..=*range.end()
    }

    /// When the tip height is higher than the requested range, then the request
    /// has been fully validated and it can be removed.
    pub fn remove_fully_validated_requests(&mut self) {
        self.pending_requests
            .retain(|_, range| range.end() > &self.tip_height);
    }
}
