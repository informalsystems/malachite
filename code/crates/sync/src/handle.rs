use std::cmp::Ordering;
use std::ops::RangeInclusive;

use derive_where::derive_where;
use tracing::{debug, error, info, warn};

use malachitebft_core_types::{Context, Height};

use crate::co::Co;
use crate::scoring::SyncResult;
use crate::{
    perform, Effect, Error, HeightStartType, InboundRequestId, Metrics, OutboundRequestId, PeerId,
    RawDecidedValue, Request, Resume, State, Status, ValueRequest, ValueResponse,
};

#[derive_where(Debug)]
pub enum Input<Ctx: Context> {
    /// A tick has occurred
    Tick,

    /// A status update has been received from a peer
    Status(Status<Ctx>),

    /// Consensus just started a new height.
    /// The boolean indicates whether this was a restart or a new start.
    StartedHeight(Ctx::Height, HeightStartType),

    /// Consensus just decided on a new value
    Decided(Ctx::Height),

    /// A ValueSync request has been received from a peer
    ValueRequest(InboundRequestId, PeerId, ValueRequest<Ctx>),

    /// A (possibly empty or invalid) ValueSync response has been received
    ValueResponse(OutboundRequestId, PeerId, Option<ValueResponse<Ctx>>),

    /// Got a response from the application to our `GetDecidedValues` request
    GotDecidedValues(
        InboundRequestId,
        RangeInclusive<Ctx::Height>,
        Vec<RawDecidedValue<Ctx>>,
    ),

    /// A request for a value timed out
    SyncRequestTimedOut(OutboundRequestId, PeerId, Request<Ctx>),

    /// We received an invalid value (either certificate or value)
    InvalidValue(PeerId, Ctx::Height),

    /// An error occurred while processing a value
    ValueProcessingError(PeerId, Ctx::Height),
}

pub async fn handle<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    input: Input<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match input {
        Input::Tick => on_tick(co, state, metrics).await,

        Input::Status(status) => on_status(co, state, metrics, status).await,

        Input::StartedHeight(height, restart) => {
            on_started_height(co, state, metrics, height, restart).await
        }

        Input::Decided(height) => on_decided(state, metrics, height).await,

        Input::ValueRequest(request_id, peer_id, request) => {
            on_value_request(co, state, metrics, request_id, peer_id, request).await
        }

        Input::ValueResponse(request_id, peer_id, response) => {
            let requested_range = match take_inflight_if_peer_matches(state, &request_id, peer_id) {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed taking inflight request: {e}");

                    // This peer sent a response for a request id that is not related to the peer, and
                    // hence we update the score of the peer accordingly.
                    state.peer_scorer.update_score(peer_id, SyncResult::Failure);

                    return Ok(());
                }
            };

            let Some(response) = response else {
                debug!(%request_id, %peer_id, "Received invalid response");

                // Received an invalid value from this peer and hence update the peer's score accordingly.
                state.peer_scorer.update_score(peer_id, SyncResult::Failure);

                return Ok(());
            };

            let start = response.start_height;
            let end = response.end_height().unwrap_or(start);
            let range_len = end.as_u64() - start.as_u64() + 1;

            // Check if the response is valid. A valid response starts at the requested start height,
            // has at least one value, and no more than the requested range.
            let is_valid = start.as_u64() == requested_range.start().as_u64()
                && start.as_u64() <= end.as_u64()
                && end.as_u64() <= requested_range.end().as_u64()
                && response.values.len() as u64 == range_len;

            if !is_valid {
                warn!(%request_id, %peer_id, "Received request for wrong range of heights: expected {}..={} ({} values), got {}..={} ({} values)",
                        requested_range.start().as_u64(), requested_range.end().as_u64(), range_len,
                        start.as_u64(), end.as_u64(), response.values.len() as u64);

                // Received a value with invalid range of heights from this peer and hence update the peer's score accordingly.
                state.peer_scorer.update_score(peer_id, SyncResult::Failure);

                return Ok(());
            }

            // Only notify consensus to start processing the response after we've performed sanity checks on the response.
            perform!(
                co,
                Effect::NotifyConsensusToProcessSyncResponse(
                    request_id.clone(),
                    peer_id,
                    response.clone(),
                    Default::default()
                )
            );

            // The peer might have sent us a partial response, so we compute what the actual `end` of
            // the returned values is and update `pending_consensus_requests` accordingly.
            let actual_end = requested_range
                .start()
                .increment_by(response.values.len() as u64)
                .decrement()
                .unwrap();
            state.pending_consensus_requests.insert(
                request_id.clone(),
                (vec![RangeInclusive::new(start, actual_end)], peer_id),
            );

            // Update peer's response time.
            debug!(start = %start, num_values = %response.values.len(), %peer_id, "Received response from peer");
            if let Some(response_time) = metrics.value_response_received(start.as_u64()) {
                state.peer_scorer.update_score_with_metrics(
                    peer_id,
                    SyncResult::Success(response_time),
                    &metrics.scoring,
                );
            }

            Ok(())
        }

        Input::GotDecidedValues(request_id, range, values) => {
            on_got_decided_values(co, state, metrics, request_id, range, values).await
        }

        Input::SyncRequestTimedOut(request_id, peer_id, request) => {
            match request {
                Request::ValueRequest(value_request) => {
                    warn!(%peer_id, range = %DisplayRange::<Ctx>(&value_request.range), "Sync request timed out");

                    state.peer_scorer.update_score(peer_id, SyncResult::Timeout);

                    match take_inflight_if_peer_matches(state, &request_id, peer_id) {
                        Ok(r) => r,
                        Err(e) => {
                            // This should NEVER happen because we should only be able to receive
                            // a time out from a request that we send.
                            error!("Failed taking inflight request: {e}");
                            return Ok(());
                        }
                    };

                    metrics.value_request_timed_out(value_request.range.start().as_u64());
                }
            };

            Ok(())
        }

        Input::InvalidValue(peer_id, height) => {
            error!(%peer_id, %height, "Received invalid value");

            state.peer_scorer.update_score(peer_id, SyncResult::Failure);

            on_invalid_value(state, height, peer_id)
        }

        Input::ValueProcessingError(peer_id, height) => {
            error!(%peer_id, %height, "Error while processing value");

            // NOTE: In contrast to `Input::InvalidValue` we do not update the peer score here, as
            // this is an internal error and not a failure from the peer's side.

            on_invalid_value(state, height, peer_id)
        }
    }
}

pub async fn on_tick<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height.tip = %state.tip_height, "Broadcasting status");

    perform!(
        co,
        Effect::BroadcastStatus(state.tip_height, Default::default())
    );

    if let Some(inactive_threshold) = state.config.inactive_threshold {
        // If we are at or above the inactive threshold, we can prune inactive peers.
        state
            .peer_scorer
            .reset_inactive_peers_scores(inactive_threshold);
    }

    debug!("Peer scores: {:#?}", state.peer_scorer.get_scores());

    Ok(())
}

pub async fn on_status<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    status: Status<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let peer_id = status.peer_id;
    let peer_height = status.tip_height;

    debug!(peer.id = %peer_id, peer.height = %peer_height, "Received peer status");

    state.update_status(status);

    if !state.started {
        // Consensus has not started yet, no need to sync (yet).
        return Ok(());
    }

    if peer_height > state.tip_height {
        warn!(
            height.tip = %state.tip_height,
            height.peer = %peer_height,
            "SYNC REQUIRED: Falling behind"
        );

        // We are lagging behind on one of our peers at least.
        // Request values from any peer already at or above that peer's height.
        request_values(co, state, peer_height, metrics).await?;
    }

    Ok(())
}

pub async fn on_started_height<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    start_type: HeightStartType,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%height, is_restart=%start_type.is_restart(), "Consensus started new height");

    state.started = true;

    // In case of a start, core-consensus should have "consumed" everything from the `sync_input_queue`
    // up to the `height` height, so we remove those pending consensus requests.
    state.prune_pending_consensus_requests(&height);

    // The tip is the last decided value.
    state.tip_height = height.decrement().unwrap_or_default();

    // Trigger potential requests if possible. Consensus just started for height `height` and
    // hence it makes sense to request this height in case some peer already has it.
    request_values(co, state, height, metrics).await?;

    Ok(())
}

pub async fn on_decided<Ctx>(
    state: &mut State<Ctx>,
    _metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%height, "Consensus decided on new value");

    state.tip_height = height;

    // In case of decision, core-consensus should have "consumed" everything from the `sync_input_queue`
    // up to the `tip_height = height` height, so we remove those pending consensus requests.
    state.prune_pending_consensus_requests(&height);

    Ok(())
}

pub async fn on_value_request<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    peer_id: PeerId,
    request: ValueRequest<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(range = %DisplayRange::<Ctx>(&request.range), %peer_id, "Received request for values");

    metrics.value_request_received(request.range.start().as_u64());

    perform!(
        co,
        Effect::GetDecidedValues(request_id, request.range, Default::default())
    );

    Ok(())
}

pub async fn on_got_decided_values<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    range: RangeInclusive<Ctx::Height>,
    values: Vec<RawDecidedValue<Ctx>>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(range = %DisplayRange::<Ctx>(&range), "Received {} values from host", values.len());

    let start = range.start();
    let end = range.end();

    // Validate response from host
    let batch_size = end.as_u64() - start.as_u64() + 1;
    if batch_size != values.len() as u64 {
        warn!(
            "Received {} values from host, expected {batch_size}",
            values.len()
        )
    }

    // Validate the height of each received value
    let mut height = *start;
    for value in values.clone() {
        if value.certificate.height != height {
            error!(
                "Received from host value for height {}, expected for height {height}",
                value.certificate.height
            );
        }
        height = height.increment();
    }

    debug!(%request_id, range = %DisplayRange::<Ctx>(&range), "Sending response to peer");
    perform!(
        co,
        Effect::SendValueResponse(
            request_id,
            ValueResponse::new(*start, values),
            Default::default()
        )
    );

    metrics.value_response_sent(start.as_u64());

    Ok(())
}

fn on_invalid_value<Ctx>(
    state: &mut State<Ctx>,
    height: Ctx::Height,
    peer_id: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let Some((request_id, stored_peer_id, ranges)) =
        state.get_pending_consensus_request_id_by(height)
    else {
        error!(%peer_id, %height, "Received height of invalid value for unknown request");
        return Ok(());
    };

    if stored_peer_id != peer_id {
        error!(%stored_peer_id, %peer_id, %request_id, "Received invalid value from wrong peer");
        return Ok(());
    }

    warn!(
                %request_id, peer_id = %peer_id, stored_peer_id = %stored_peer_id, %height,
                "Received value at height failed consensus verification");

    // remove the height of the invalid value from cloned_ranges
    let mut cloned_ranges = ranges.clone();
    if let Err(msg) = excise_height(&mut cloned_ranges, height) {
        error!(%msg, %request_id, %peer_id, %height, "Failed to break range");
        return Ok(());
    }

    // update the pending requests for `request_id` after excising the height of the invalid value
    if let Some((ranges, _)) = state.pending_consensus_requests.get_mut(&request_id) {
        if ranges.is_empty() {
            state.pending_consensus_requests.remove(&request_id);
        } else {
            state
                .pending_consensus_requests
                .insert(request_id, (cloned_ranges, peer_id));
        }
    }

    Ok(())
}

/// Excise `height` from the first (and only) range in `ranges` that contains it and then append up to two ranges
/// that exclude `height`. We assume that `ranges` does not contain a height more than once.
/// Return `Err` if no range contains `height` or if `height - 1` underflows.
pub fn excise_height<H>(ranges: &mut Vec<RangeInclusive<H>>, height: H) -> Result<(), String>
where
    H: Height,
{
    let Some(i) = ranges.iter_mut().position(|r| r.contains(&height)) else {
        return Err(format!("cannot find range with this height {:?}", height));
    };

    let range = ranges[i].clone();
    // always remove the range we want to "break" into two ranges and then re-push desired ranges
    ranges.remove(i);

    if range.start() != range.end() {
        if *range.start() == height {
            ranges.push(range.start().increment()..=*range.end());
        } else if *range.end() == height {
            let Some(new_end) = range.end().decrement() else {
                return Err(format!("cannot decrement {:?}", range.end()));
            };
            ranges.push(*range.start()..=new_end);
        } else {
            let Some(new_end) = height.decrement() else {
                return Err(format!("cannot decrement {:?}", height));
            };
            ranges.push(*range.start()..=new_end);
            ranges.push(height.increment()..=*range.end());
        }
    }

    Ok(())
}

// Returns the current number of heights we have requested (e.g., inflight requests) or that we
// are currently processing (e.g.., pending consensus verification).
fn current_number_of_heights<Ctx>(state: &State<Ctx>) -> u64
where
    Ctx: Context,
{
    let mut heights = 0;

    for (_, (range, _)) in state.inflight_requests.iter() {
        let requested_heights = range.end().as_u64() - range.start().as_u64() + 1;
        heights += requested_heights;
    }

    for (_, (ranges, _)) in state.pending_consensus_requests.iter() {
        let mut pending_consensus_heights = 0;

        for range in ranges {
            pending_consensus_heights += range.end().as_u64() - range.start().as_u64() + 1;
        }

        heights += pending_consensus_heights;
    }

    heights
}

/// Finds the earliest free window `[a, b]` such that:
/// - `a > exclusive_start_height`
/// - every x in `[a, b]` is not covered by any range in `ranges`
/// - `b` does not exceed `inclusive_up_to_height`
/// - the window length (number of elements) is ≤ `window_size`
///
/// "Earliest" means we pick the smallest valid `a`, then extend `b` as far as possible without
/// crossing the next blocking range, the `inclusive_up_to_height` cap, or the `window_size` limit.
///
/// Returns `None` if no such window exists.
///
/// Examples:
/// - If `window_size == 0`, there is no valid window.
/// - If `window_size == 1`, the window is a single element `[a, a]`.
fn compute_free_window<H>(
    exclusive_start_height: H,
    inclusive_up_to_height: H,
    window_size: u64,
    mut ranges: Vec<RangeInclusive<H>>,
) -> Option<RangeInclusive<H>>
where
    H: Height,
{
    // we can include at most `window_size` elements: grow from `a` by at most `n - 1` increments.
    let mut remaining_increments = match window_size {
        0 => return None,
        1 => 0,
        n => n - 1,
    };

    // order ranges by first `start()` and then `end()` and where overlaps are fine
    ranges.sort_by(|a, b| match a.start().cmp(b.start()) {
        Ordering::Equal => a.end().cmp(b.end()),
        other => other,
    });

    let mut a = exclusive_start_height.increment_by(1);
    if a > inclusive_up_to_height {
        return None;
    }

    let mut i = 0;
    while i < ranges.len() {
        let r = &ranges[i];

        if a < *r.start() {
            // `a` lies strictly before this interval; no later interval can cover `a`
            break;
        } else if a <= *r.end() {
            // `a` is covered; jump to just after this interval and keep going **from here**
            a = r.end().increment();
            if a > inclusive_up_to_height {
                return None;
            }
            i += 1;
            continue;
        } else {
            // a > r.end(): this interval is behind us; advance.
            i += 1;
        }
    }

    // determine the start of the next blocking interval (if any)
    let next_block_start = ranges.iter().find(|r| *r.start() >= a).map(|r| *r.start());
    let mut b = a;

    // grow `b` from `a` without crossing the next range, the `inclusive_up_to_height`, or the window size
    while remaining_increments > 0 {
        let next = b.increment_by(1);

        if next > inclusive_up_to_height {
            break;
        }

        if let Some(s) = &next_block_start {
            if next >= *s {
                break;
            }
        }

        b = next;
        remaining_increments -= 1;
    }

    Some(a..=b)
}

/// Find the earliest "free" range of height `[a, b]` that the node can request for such that:
/// - `a > exclusive_start_height`
/// - every x in `[a, b]` is not covered by any height in `state.pending_consensus_requests` and `state.inflight_requests`
/// - `b` does not exceed `inclusive_up_to_height`
/// - the return range length (number of elements) is ≤ `state.config.batch_size`
///
/// "Earliest" means we pick the smallest valid `a`, then extend `b` as far as possible without
/// crossing the next blocking range, the `inclusive_up_to_height` cap, or the `state.config.batch_size` limit.
///
/// Returns `None` if no such window exists.
fn find_range_of_heights_to_request<Ctx>(
    state: &State<Ctx>,
    up_to_height: Ctx::Height,
) -> Option<RangeInclusive<Ctx::Height>>
where
    Ctx: Context,
{
    let tip = state.tip_height;
    let batch_size = state.config.batch_size as u64;

    let ranges: Vec<_> = state
        .inflight_requests
        .values()
        .map(|(r, _)| r.clone())
        .chain(
            state
                .pending_consensus_requests
                .values()
                .flat_map(|(ranges, _)| ranges.iter().cloned()),
        )
        .collect();

    compute_free_window(tip, up_to_height, batch_size, ranges)
}

/// Request multiple batches of values in parallel that are of height less than `up_to_height`.
async fn request_values<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    up_to_height: Ctx::Height,
    metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let max_parallel_requests = state.max_parallel_requests();

    if state.inflight_requests.len() as u64 >= max_parallel_requests {
        info!(
            %max_parallel_requests,
            inflight_requests = %state.inflight_requests.len(),
            "Maximum number of parallel inflight requests reached, skipping request for values"
        );

        return Ok(());
    };

    // The maximum number of heights/values we can be waiting for at any point in time.
    // Should be equal to the capacity of the `sync_input_queue` so that we do not overflow this queue.
    let max_heights = state.config.batch_size as u64 * state.config.parallel_requests;

    loop {
        let Some(range) = find_range_of_heights_to_request(state, up_to_height) else {
            return Ok(());
        };

        let Some((peer_id, _)) = state.random_peer_with(&range) else {
            return Ok(());
        };

        // Check that we can issue a request for the desired range without exceeding the `max_values`
        // and hence we won't overflow the consensus' `sync_input_queue`.
        let current_number_of_heights = current_number_of_heights(state);
        let asking_for_how_many_heights = range.end().as_u64() - range.start().as_u64() + 1;
        if current_number_of_heights + asking_for_how_many_heights > max_heights {
            return Ok(());
        }

        request_values_from_peer(&co, state, metrics, range.clone(), peer_id).await?;
    }
}

async fn request_values_from_peer<Ctx>(
    co: &Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    range: RangeInclusive<Ctx::Height>,
    peer: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(range = %DisplayRange::<Ctx>(&range), peer.id = %peer, "Requesting sync from peer");

    if range.is_empty() {
        warn!(range.sync = %DisplayRange::<Ctx>(&range), %peer, "Range is empty, skipping request");
        return Ok(());
    }

    // Skip over any heights in the range that are not waiting for a response
    // (meaning that they have been validated by consensus or a peer).
    let range = state.trim_validated_heights(&range);
    if range.is_empty() {
        warn!(%peer, "All values in range {} have been validated, skipping request", DisplayRange::<Ctx>(&range));
        return Ok(());
    }

    // Send request to peer
    let Some(request_id) = perform!(
        co,
        Effect::SendValueRequest(peer, ValueRequest::new(range.clone()), Default::default()),
        Resume::ValueRequestId(id) => id,
    ) else {
        warn!(range = %DisplayRange::<Ctx>(&range), %peer, "Failed to send sync request to peer");
        return Ok(());
    };

    metrics.value_request_sent(range.start().as_u64());

    // Store inflight request
    debug!(%request_id, range = %DisplayRange::<Ctx>(&range), %peer, "Sent sync request to peer");
    state.inflight_requests.insert(request_id, (range, peer));

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum InflightError {
    #[error("received response from unknown {request_id}")]
    UnknownRequestId { request_id: Box<OutboundRequestId> },

    #[error(
        "received response from wrong peer (expected {expected_peer_id}, got {actual_peer_id})"
    )]
    WrongPeer {
        expected_peer_id: Box<PeerId>,
        actual_peer_id: Box<PeerId>,
    },
}

// Deletes an inflight request if one exists under the provided `request_id` and stems from `peer_id`.
// Returns the requested range of this inflight request and the peer that issues the request.
fn take_inflight_if_peer_matches<Ctx>(
    state: &mut State<Ctx>,
    request_id: &OutboundRequestId,
    peer_id: PeerId,
) -> Result<RangeInclusive<Ctx::Height>, InflightError>
where
    Ctx: Context,
{
    let Some((requested_range, stored_peer_id)) = state.inflight_requests.get(request_id).cloned()
    else {
        return Err(InflightError::UnknownRequestId {
            request_id: Box::new(request_id.clone()),
        });
    };

    if stored_peer_id != peer_id {
        return Err(InflightError::WrongPeer {
            expected_peer_id: Box::new(stored_peer_id),
            actual_peer_id: Box::new(peer_id),
        });
    }

    // We received a response to a sync request from the corresponding peer, and hence this request
    // is not in flight anymore.
    state.inflight_requests.remove(request_id);

    Ok(requested_range)
}

struct DisplayRange<'a, Ctx: Context>(&'a RangeInclusive<Ctx::Height>);

impl<'a, Ctx: Context> core::fmt::Display for DisplayRange<'a, Ctx> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}..={}", self.0.start(), self.0.end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt;

    /// fake height for tests that raps an u64 and provides the `Height` methods
    #[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
    struct H(u64);

    impl Height for H {
        const ZERO: Self = H(0);
        const INITIAL: Self = H(0);
        fn increment(&self) -> H {
            H(self.0 + 1)
        }
        fn decrement(&self) -> Option<H> {
            self.0.checked_sub(1).map(H)
        }

        fn increment_by(&self, n: u64) -> H {
            H(self.0 + n)
        }
        fn decrement_by(&self, _n: u64) -> Option<Self> {
            None
        }

        fn as_u64(&self) -> u64 {
            self.0
        }
    }

    impl fmt::Display for H {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "H({})", self.0)
        }
    }

    impl fmt::Debug for H {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "H({})", self.0)
        }
    }

    // helper to make a range quickly
    fn range(a: u64, b: u64) -> core::ops::RangeInclusive<H> {
        H(a)..=H(b)
    }

    #[test]
    fn empty_maps_no_blocks_fits_entire_batch() {
        let out = compute_free_window(H(10), H(100), 5, vec![]).unwrap();
        assert_eq!(*out.start(), H(11));
        assert_eq!(*out.end(), H(15));
    }

    #[test]
    fn capped_by_peer_height() {
        let out = compute_free_window(H(10), H(12), 10, vec![]).unwrap();
        assert_eq!(*out.start(), H(11));
        assert_eq!(*out.end(), H(12));
    }

    #[test]
    fn batch_size_one_returns_singleton() {
        let out = compute_free_window(H(7), H(100), 1, vec![]).unwrap();
        assert_eq!(*out.start(), H(8));
        assert_eq!(*out.end(), H(8));
    }

    #[test]
    fn batch_size_zero_yields_none() {
        assert!(compute_free_window(H(7), H(100), 0, vec![]).is_none());
    }

    #[test]
    fn peer_at_or_below_tip_none() {
        assert!(compute_free_window(H(10), H(10), 5, vec![]).is_none());
        assert!(compute_free_window(H(10), H(9), 5, vec![]).is_none());
    }

    #[test]
    fn skips_exact_cover_at_a() {
        let out = compute_free_window(H(9), H(100), 4, vec![range(10, 12)]).unwrap();
        assert_eq!(*out.start(), H(13));
        assert_eq!(*out.end(), H(16));
    }

    #[test]
    fn stops_before_next_blocking_range() {
        let out = compute_free_window(H(9), H(100), 10, vec![range(15, 20)]).unwrap();
        assert_eq!(*out.start(), H(10));
        assert_eq!(*out.end(), H(14));
    }

    #[test]
    fn jumps_over_overlapping_blocks() {
        let out = compute_free_window(
            H(9),
            H(100),
            3,
            vec![range(10, 12), range(11, 13), range(13, 14)],
        )
        .unwrap();
        assert_eq!(*out.start(), H(15));
        assert_eq!(*out.end(), H(17));
    }

    #[test]
    fn peer_cap_hits_inside_growth() {
        let out = compute_free_window(H(0), H(5), 100, vec![]).unwrap();
        assert_eq!(*out.start(), H(1));
        assert_eq!(*out.end(), H(5));
    }

    #[test]
    fn hole_between_blocks_fills_up_to_batch_limit() {
        let out = compute_free_window(H(0), H(100), 4, vec![range(1, 3), range(10, 99)]).unwrap();
        assert_eq!(*out.start(), H(4));
        assert_eq!(*out.end(), H(7));
    }

    #[test]
    fn hole_smaller_than_batch_stops_at_block() {
        let out = compute_free_window(H(0), H(100), 10, vec![range(1, 3), range(9, 20)]).unwrap();
        assert_eq!(*out.start(), H(4));
        assert_eq!(*out.end(), H(8));
    }

    // helper to normalize `Vec<RangeInclusive<H>>` for easy equality checks
    fn as_pairs(v: &[RangeInclusive<H>]) -> Vec<(u64, u64)> {
        let mut pairs: Vec<_> = v
            .iter()
            .map(|r| (r.start().as_u64(), r.end().as_u64()))
            .collect();
        pairs.sort_unstable();
        pairs
    }

    #[test]
    fn not_found_returns_err() {
        let mut v = vec![range(5, 7), range(9, 12)];
        let err = excise_height(&mut v, H(8)).unwrap_err();
        assert!(err.contains("cannot find range"), "got: {}", err);
        assert_eq!(as_pairs(&v), vec![(5, 7), (9, 12)]);
    }

    #[test]
    fn singleton_range_removed() {
        let mut v = vec![range(5, 5)];
        excise_height(&mut v, H(5)).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn singleton_range_removed_with_other_ranges() {
        let mut v = vec![range(1, 3), range(5, 5), range(9, 11)];
        excise_height(&mut v, H(5)).unwrap();
        assert_eq!(as_pairs(&v), vec![(1, 3), (9, 11)]);
    }

    #[test]
    fn height_at_left_edge_trims_left() {
        let mut v = vec![range(5, 10)];
        excise_height(&mut v, H(5)).unwrap();
        assert_eq!(as_pairs(&v), vec![(6, 10)]);
    }

    #[test]
    fn height_at_right_edge_trims_right() {
        let mut v = vec![range(5, 10)];
        excise_height(&mut v, H(10)).unwrap();
        assert_eq!(as_pairs(&v), vec![(5, 9)]);
    }

    #[test]
    fn height_inside_splits_into_two() {
        let mut v = vec![range(5, 10)];
        excise_height(&mut v, H(7)).unwrap();
        assert_eq!(as_pairs(&v), vec![(5, 6), (8, 10)]);
    }

    #[test]
    fn multiple_ranges_only_first_matching_is_changed() {
        let mut v = vec![range(1, 3), range(5, 7), range(9, 11)];
        excise_height(&mut v, H(6)).unwrap();
        assert_eq!(as_pairs(&v), vec![(1, 3), (5, 5), (7, 7), (9, 11)]);
    }
}
