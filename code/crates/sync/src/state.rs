use malachitebft_core_types::{Context, Height};
use malachitebft_peer::PeerId;
use std::cmp::max;
use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

use crate::scoring::{ema, PeerScorer, Strategy};
use crate::{Config, OutboundRequestId, Status};
use crate::handle::excise_height;

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

    /// Requests that are still inflight (i.e., we have not received a response yet).
    pub inflight_requests: BTreeMap<OutboundRequestId, (RangeInclusive<Ctx::Height>, PeerId)>,

    /// The requested heights that we have received and are waiting for consensus verification.
    /// Note that even though a peer requests a range of values as a whole, the consensus verifies each
    /// value on its own. As a result, the consensus informs the sync actor per value (e.g., see
    /// `InvalidValue` and `ValueProcessingError` messages).
    /// For example, we can have a scenario where a request `r` for [10..20] gets an invalid value
    /// for height 15. In such a scenario, `r` might still be waiting for consensus verification
    /// for heights [10..14] and [16..20].
    /// To tackle the above, we store a vec of ranges instead of a single range of heights.
    pub pending_consensus_requests:
        BTreeMap<OutboundRequestId, (Vec<RangeInclusive<Ctx::Height>>, PeerId)>,

    /// The set of peers we are connected to in order to get values, certificates and votes.
    pub peers: BTreeMap<PeerId, Status<Ctx>>,

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
            inflight_requests: BTreeMap::new(),
            pending_consensus_requests: BTreeMap::new(),
            peers: BTreeMap::new(),
            peer_scorer,
        }
    }

    /// The maximum number of parallel requests that can be made to peers.
    /// If the configuration is set to 0, it defaults to 1.
    pub fn max_parallel_requests(&self) -> u64 {
        max(1, self.config.parallel_requests)
    }

    pub fn update_status(&mut self, status: Status<Ctx>) {
        self.peers.insert(status.peer_id, status);
    }

    /// Filter peers to only include those that can provide the given range of values, or at least a prefix of the range.
    ///
    /// If there is no peer with all requested values, select a peer that has a tip at or above the start of the range.
    /// Prefer peers that support batching (v2 sync protocol).
    /// Return the peer ID and the range of heights that the peer can provide.
    pub fn filter_peers_by_range(
        peers: &BTreeMap<PeerId, Status<Ctx>>,
        range: &RangeInclusive<Ctx::Height>,
        except: Option<PeerId>,
    ) -> HashMap<PeerId, RangeInclusive<Ctx::Height>> {
        // Peers that can provide the whole range of values.
        let peers_with_whole_range = peers
            .iter()
            .filter(|(peer, status)| {
                status.history_min_height <= *range.start()
                    && *range.start() <= *range.end()
                    && *range.end() <= status.tip_height
                    && except.is_none_or(|p| p != **peer)
            })
            .map(|(peer, _)| (*peer, range.clone()))
            .collect::<HashMap<_, _>>();

        // Prefer peers that have the whole range of values in their history.
        if !peers_with_whole_range.is_empty() {
            peers_with_whole_range
        } else {
            // Otherwise, just get the peers that can provide a prefix of the range.
            peers
                .iter()
                .filter(|(peer, status)| {
                    status.history_min_height <= *range.start()
                        && except.is_none_or(|p| p != **peer)
                })
                .map(|(peer, status)| (*peer, *range.start()..=status.tip_height))
                .filter(|(_, range)| !range.is_empty())
                .collect::<HashMap<_, _>>()
        }
    }

    /// Select at random a peer that can provide the given range of values, while excluding the given peer if provided.
    pub fn random_peer_with_except(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
        except: Option<PeerId>,
    ) -> Option<(PeerId, RangeInclusive<Ctx::Height>)> {
        // Filtered peers together with the range of heights they can provide.
        let peers_range = Self::filter_peers_by_range(&self.peers, range, except);

        // Select a peer at random.
        let peer_ids = peers_range.keys().cloned().collect::<Vec<_>>();
        self.peer_scorer
            .select_peer(&peer_ids, &mut self.rng)
            .map(|peer_id| (peer_id, peers_range.get(&peer_id).unwrap().clone()))
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

    /// Get the request that contains the given height.
    ///
    /// Assumes a height cannot be in multiple pending requests.
    pub fn get_pending_consensus_request_id_by(
        &self,
        height: Ctx::Height,
    ) -> Option<(OutboundRequestId, PeerId, Vec<RangeInclusive<Ctx::Height>>)> {
        self.pending_consensus_requests
            .iter()
            .find_map(|(request_id, (ranges, stored_peer_id))| {
                if ranges.iter().any(|range| range.contains(&height)) {
                    Some((request_id.clone(), *stored_peer_id, ranges.clone()))
                } else {
                    None
                }
            })
    }

    /// Return a new range of heights, trimming from the beginning any height
    /// that is validated by consensus.
    pub fn trim_validated_heights(
        &mut self,
        range: &RangeInclusive<Ctx::Height>,
    ) -> RangeInclusive<Ctx::Height> {
        let start = max(self.tip_height.increment(), *range.start());
        start..=*range.end()
    }

    /// Prunes all the heights that are <= `up_to_height` from all the pending_consensus_requests
    pub fn prune_pending_consensus_requests(&mut self, up_to_height: &Ctx::Height) {
        self.pending_consensus_requests.retain(|_, (ranges, _)| {
            // We first excise the `up_to_height` from the `ranges` and as a result, all the `ranges`
            // are going to be `[a, b]` with `b < up_to_height` or `a > up_to_height`.
            // We then retain all the ranges `a > up_to_height`, conversely, we remove all the ranges with `b < up_to_height`.
            let _ = excise_height(ranges, *up_to_height);
            ranges.retain(|range| range.end() > up_to_height);
            !ranges.is_empty()
        });
    }
}
