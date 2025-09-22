use std::cmp::max;
use std::ops::RangeInclusive;

use derive_where::derive_where;
use tracing::{debug, error, info, warn};

use malachitebft_core_types::{Context, Height};

use crate::co::Co;
use crate::scoring::SyncResult;
use crate::state::QueuedRequest;
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
                return on_invalid_value_response(
                    co,
                    state,
                    metrics,
                    request_id,
                    peer_id,
                    requested_range,
                )
                .await;
            };

            let start = response.start_height;
            let end = response.end_height().unwrap_or(start);
            let range_len = end.as_u64() - start.as_u64() + 1;

            // Check if the response is valid. A valid response starts at the
            // requested start height, has at least one value, and no more than
            // the requested range.
            let is_valid = start.as_u64() == requested_range.start().as_u64()
                && start.as_u64() <= end.as_u64()
                && end.as_u64() <= requested_range.end().as_u64()
                && response.values.len() as u64 == range_len;

            if !is_valid {
                warn!(%request_id, %peer_id, "Received request for wrong range of heights: expected {}..={} ({} values), got {}..={} ({} values)",
                        requested_range.start().as_u64(), requested_range.end().as_u64(), range_len,
                        start.as_u64(), end.as_u64(), response.values.len() as u64);
                return on_invalid_value_response(
                    co,
                    state,
                    metrics,
                    request_id,
                    peer_id,
                    requested_range,
                )
                .await;
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

            // We have a pending request to be checked for the values that we were given ...
            let actual_end = requested_range
                .start()
                .increment_by(response.values.len() as u64)
                .decrement()
                .unwrap();
            state.pending_consensus_requests.insert(
                request_id.clone(),
                (RangeInclusive::new(start, actual_end), peer_id),
            );

            on_value_response(
                co,
                state,
                metrics,
                request_id,
                peer_id,
                requested_range,
                response,
            )
            .await
        }

        Input::GotDecidedValues(request_id, range, values) => {
            on_got_decided_values(co, state, metrics, request_id, range, values).await
        }

        Input::SyncRequestTimedOut(request_id, peer_id, request) => {
            on_sync_request_timed_out(co, state, metrics, request_id, peer_id, request).await
        }

        Input::InvalidValue(peer, value) => on_invalid_value(co, state, metrics, peer, value).await,

        Input::ValueProcessingError(peer, height) => {
            on_value_processing_error(co, state, metrics, peer, height).await
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

    if peer_height >= state.sync_height {
        warn!(
            height.tip = %state.tip_height,
            height.sync = %state.sync_height,
            height.peer = %peer_height,
            "SYNC REQUIRED: Falling behind"
        );

        // We are lagging behind on one of our peers at least.
        // Request values from any peer already at or above that peer's height.
        request_values(co, state, metrics).await?;
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

    // The tip is the last decided value.
    state.tip_height = height.decrement().unwrap_or_default();

    // Garbage collect fully-validated requests.
    state.remove_fully_validated_requests();

    if start_type.is_restart() {
        // Consensus is retrying the height, so we should sync starting from it.
        state.sync_height = height;
        // Clear pending requests, as we are restarting the height.
        state.pending_consensus_requests.clear();
        // FIXME: do we need to clear up inflight requests
    } else {
        // If consensus is voting on a height that is currently being synced from a peer, do not update the sync height.
        state.sync_height = max(state.sync_height, height);
    }

    // Trigger potential requests if possible.
    request_values(co, state, metrics).await?;

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

    // Garbage collect fully-validated requests.
    state.remove_fully_validated_requests();

    // The next height to sync should always be higher than the tip.
    if state.sync_height == state.tip_height {
        state.sync_height = state.sync_height.increment();
    }

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

pub async fn on_value_response<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer_id: PeerId,
    requested_range: RangeInclusive<Ctx::Height>,
    response: ValueResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let start = response.start_height;
    debug!(start = %start, num_values = %response.values.len(), %peer_id, "Received response from peer");

    if let Some(response_time) = metrics.value_response_received(start.as_u64()) {
        state.peer_scorer.update_score_with_metrics(
            peer_id,
            SyncResult::Success(response_time),
            &metrics.scoring,
        );
    }

    let range_len = requested_range.end().as_u64() - requested_range.start().as_u64() + 1;

    if response.values.len() < range_len as usize {
        // NOTE: We cannot simply call `re_request_values_from_peer_except` here.
        // Although we received some values from the peer, these values have not yet been processed
        // by the consensus engine. If we called `re_request_values_from_peer_except`, we would
        // end up re-requesting the entire original range (including values we already received),
        // causing the syncing peer to repeatedly send multiple requests until the already-received
        // values are fully processed.
        // To tackle this, we first update the current pending request with the range of values
        // it provides we received, and then issue a new request with the remaining values.
        let new_start = requested_range
            .start()
            .increment_by(response.values.len() as u64);

        let end = *requested_range.end();

        if response.values.is_empty() {
            error!(%request_id, %peer_id, "Received response contains no values");
        }

        // Issue a new request to any peer, not necessarily the same one, for the remaining values
        let new_range = new_start..=end;
        state
            .queued_requests
            .insert(QueuedRequest::new(new_range, peer_id));
        // request_values_range(co, state, metrics, new_range).await?;
    }

    Ok(())
}

pub async fn on_invalid_value_response<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer_id: PeerId,
    requested_range: RangeInclusive<Ctx::Height>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request_id, %peer_id, "Received invalid response");

    // Received an invalid value from this peer and hence update the peer's score accordingly.
    state.peer_scorer.update_score(peer_id, SyncResult::Failure);

    state
        .queued_requests
        .insert(QueuedRequest::new(requested_range, peer_id));

    // let requested_range = match take_inflight_if_peer_matches(state, &request_id, peer_id) {
    //     Ok(r) => r,
    //     Err(e) => {
    //         warn!("Failed taking inflight request: {e}");
    //         return Ok(());
    //     }
    // };

    // We do not trust the response, so we remove the pending request and re-request
    // the whole range from another peer.
    // re_request_values_from_peer_except(
    //     co,
    //     state,
    //     metrics,
    //     request_id,
    //     requested_range,
    //     Some(peer_id),
    // )
    // .await?;

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

pub async fn on_sync_request_timed_out<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer_id: PeerId,
    request: Request<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match request {
        Request::ValueRequest(value_request) => {
            warn!(%peer_id, range = %DisplayRange::<Ctx>(&value_request.range), "Sync request timed out");

            state.peer_scorer.update_score(peer_id, SyncResult::Timeout);

            let requested_range = match take_inflight_if_peer_matches(state, &request_id, peer_id) {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed taking inflight request: {e}");
                    return Ok(());
                }
            };

            metrics.value_request_timed_out(value_request.range.start().as_u64());

            state
                .queued_requests
                .insert(QueuedRequest::new(requested_range, peer_id));
            //
            // re_request_values_from_peer_except(
            //     co,
            //     state,
            //     metrics,
            //     request_id,
            //     requested_range,
            //     Some(peer_id),
            // )
            // .await?;
        }
    };

    Ok(())
}

// When receiving an invalid value, re-request the whole batch from another peer.
async fn on_invalid_value<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    peer_id: PeerId,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    error!(%peer_id, %height, "Received invalid value");

    state.peer_scorer.update_score(peer_id, SyncResult::Failure);

    if let Some((request_id, stored_peer_id, range)) = state.get_request_id_by(height) {
        if stored_peer_id != peer_id {
            warn!(
                %request_id, peer.actual = %peer_id, peer.expected = %stored_peer_id,
                "Received response from different peer than expected"
            );
        } else {
            state
                .queued_requests
                .insert(QueuedRequest::new(range, peer_id));
            //
            // re_request_values_from_peer_except(
            //     co,
            //     state,
            //     metrics,
            //     request_id,
            //     range,
            //     Some(peer_id),
            // )
            // .await?;
        }
    } else {
        error!(%peer_id, %height, "Received height of invalid value for unknown request");
    }

    Ok(())
}

async fn on_value_processing_error<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    peer_id: PeerId,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    error!(%peer_id, %height, "Error while processing value");

    // NOTE: We do not update the peer score here, as this is an internal error
    //       and not a failure from the peer's side.

    if let Some((request_id, _, range)) = state.get_request_id_by(height) {
        re_request_values_from_peer_except(co, state, metrics, request_id, range, None).await?;
    } else {
        error!(%peer_id, %height, "Received height of invalid value for unknown request");
    }

    Ok(())
}

fn current_amount_of_values<Ctx>(state: &State<Ctx>) -> u64
where
    Ctx: Context,
{
    let mut values = 0;

    for (_, (range, _)) in state.inflight_requests.iter() {
        let requested_values = range.end().as_u64() - range.start().as_u64() + 1;
        values += requested_values;
    }

    for (_, (range, _)) in state.pending_consensus_requests.iter() {
        let requested_values = range.end().as_u64() - range.start().as_u64() + 1;
        values += requested_values;
    }

    values
}

/// Request multiple batches of values in parallel.
async fn request_values<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
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

    // The maximum number of values we can be waiting for at any point in time.
    // Should be equal to the capacity of the `sync_input_queue`.
    let max_values = state.config.batch_size as u64 * state.config.parallel_requests;

    'outer: loop {
        let current_amount_of_values = current_amount_of_values(state);

        // 1. Gather any requests if we can still send them through without exceeding `max_values`.
        let mut to_actually_send = Vec::new();
        for pending_request in state.queued_requests.iter() {
            let range = &pending_request.range;
            let except_peer_id = &pending_request.peer_id;

            // can I issue this request at this point in time???
            let asking_for_how_many_values = range.end().as_u64() - range.start().as_u64() + 1;
            if current_amount_of_values + asking_for_how_many_values > max_values {
                if to_actually_send.is_empty() {
                    // nothing was added
                    break 'outer;
                }
                break;
            }

            to_actually_send.push(pending_request.clone());
        }

        // 2. Send all the requests.
        for request_to_send in to_actually_send.iter() {
            // FIXME: should not touch the sync height ...

            println!("I AM HERE ...");

            request_values_from_peer(
                &co,
                state,
                metrics,
                request_to_send.range.clone(),
                request_to_send.peer_id,
            )
            .await?;

            // remove from the rquests to be send
            state.queued_requests.remove(request_to_send);
        }

        // 3. Check whether we can gather more requests to send.
        if state.queued_requests.is_empty() {
            // Build the next range of heights to request from a peer.
            let start_height = state.sync_height;
            let batch_size = max(1, state.config.batch_size as u64);
            let end_height = start_height.increment_by(batch_size - 1);
            let range = start_height..=end_height;

            // Get a random peer that can provide the values in the range.
            match state.random_peer_with(&range) {
                Some((peer_id, range)) => {
                    // update sync height ...
                    state.sync_height = max(state.sync_height, range.end().increment());
                    state
                        .queued_requests
                        .insert(QueuedRequest::new(range, peer_id));
                }
                None => {
                    debug!("No peer to request sync from");
                    // No connected peer reached this height yet, we can stop syncing here.
                    break 'outer;
                }
            };
        }
    }

    Ok(())
}

/// Request values for this specific range from a peer.
/// Should only be used when re-requesting a partial range of values from a peer.
async fn request_values_range<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    range: RangeInclusive<Ctx::Height>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    // NOTE: We do not perform a `max_parallel_requests` check and return here in contrast to what is done, for
    // example, in `request_values`. This is because `request_values_range` is only called for retrieving
    // partial responses, which means the original request is not on the wire anymore. Nevertheless,
    // we log here because seeing this log frequently implies that we keep getting partial responses
    // from peers and hints to potential reconfiguration.
    let max_parallel_requests = state.max_parallel_requests();
    if state.pending_consensus_requests.len() as u64 >= max_parallel_requests {
        info!(
            %max_parallel_requests,
            pending_consensus_requests = %state.pending_consensus_requests.len(),
            "Maximum number of pending requests reached when re-requesting a partial range of values"
        );
    };

    // Get a random peer that can provide the values in the range.
    let Some((peer, range)) = state.random_peer_with(&range) else {
        // No connected peer reached this height yet, we can stop syncing here.
        debug!(range = %DisplayRange::<Ctx>(&range), "No peer to request sync from");
        return Ok(());
    };

    request_values_from_peer(&co, state, metrics, range, peer).await?;

    Ok(())
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

    // Store inflight request and move the sync height.
    debug!(%request_id, range = %DisplayRange::<Ctx>(&range), %peer, "Sent sync request to peer");
    // state.sync_height = max(state.sync_height, range.end().increment());
    state.inflight_requests.insert(request_id, (range, peer));

    Ok(())
}

/// Remove the pending request and re-request the batch from another peer.
/// If `except_peer_id` is provided, the request will be re-sent to a different peer than the one that sent the original request.
async fn re_request_values_from_peer_except<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    range: RangeInclusive<Ctx::Height>,
    except_peer_id: Option<PeerId>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(%request_id, except_peer_id = ?except_peer_id, "Re-requesting values from peer");

    // It is possible that a prefix or the whole range of values has been validated via consensus.
    // Then, request only the missing values.
    let range = state.trim_validated_heights(&range);

    if range.is_empty() {
        warn!(
            %request_id,
            "All values in range {} have been validated, skipping re-request",
            DisplayRange::<Ctx>(&range)
        );

        return Ok(());
    }

    let peer_id = state
        .random_peer_with_except(&range, except_peer_id)
        .map(|(id, _)| id)
        .or(except_peer_id)
        .ok_or_else(|| {
            error!(
                range.sync = %DisplayRange::<Ctx>(&range),
                "No peer to request sync from; no fallback with `except_peer_id` either"
            );
            Error::PeerNotFound()
        })?;

    request_values_from_peer(&co, state, metrics, range, peer_id).await?;

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum InflightError {
    #[error("received response from unknown request id")]
    UnknownRequestId {
        request_id: OutboundRequestId,
        peer_id: PeerId,
    },

    #[error(
        "received response from wrong peer (expected {expected_peer_id}, got {actual_peer_id})"
    )]
    WrongPeer {
        request_id: OutboundRequestId,
        expected_peer_id: PeerId,
        actual_peer_id: PeerId,
    },
}

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
            request_id: request_id.clone(),
            peer_id,
        });
    };

    if stored_peer_id != peer_id {
        // This peer sent a response for a request id that is not related to the peer, and
        // hence we update the score of the peer accordingly.
        state.peer_scorer.update_score(peer_id, SyncResult::Failure);
        return Err(InflightError::WrongPeer {
            request_id: request_id.clone(),
            expected_peer_id: stored_peer_id,
            actual_peer_id: peer_id,
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
