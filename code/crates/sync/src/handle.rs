use std::ops::RangeInclusive;

use derive_where::derive_where;
use tracing::{debug, error, info, trace, warn};

use malachitebft_core_types::{CertificateError, CommitCertificate, Context, Height};

use crate::co::Co;
use crate::scoring::SyncResult;
use crate::{
    perform, BatchRequest, BatchResponse, Effect, Error, InboundRequestId, Metrics,
    OutboundRequestId, PeerId, PeerKind, RawDecidedValue, Request, Resume, State, Status,
    ValueRequest, ValueResponse,
};

#[derive_where(Debug)]
pub enum Input<Ctx: Context> {
    /// A tick has occurred
    Tick,

    /// A status update has been received from a peer
    Status(Status<Ctx>),

    /// Consensus just started a new height.
    /// The boolean indicates whether this was a restart or a new start.
    StartedHeight(Ctx::Height, bool),

    /// Consensus just decided on a new value
    Decided(Ctx::Height),

    /// A ValueSync request has been received from a peer
    ValueRequest(InboundRequestId, PeerId, ValueRequest<Ctx>),

    /// A (possibly empty or invalid) ValueSync response has been received
    ValueResponse(OutboundRequestId, PeerId, Option<ValueResponse<Ctx>>),

    /// A BatchSync request has been received from a peer
    BatchRequest(InboundRequestId, PeerId, BatchRequest<Ctx>),

    /// A BatchSync response has been received
    BatchResponse(OutboundRequestId, PeerId, BatchResponse<Ctx>),

    /// Got a response from the application to our `GetValue` request
    GotDecidedValue(InboundRequestId, Ctx::Height, Option<RawDecidedValue<Ctx>>),

    /// Got a response from the application to our `GetValues` request
    GotDecidedValues(
        InboundRequestId,
        RangeInclusive<Ctx::Height>,
        Vec<RawDecidedValue<Ctx>>,
    ),

    /// A request for a value timed out
    SyncRequestTimedOut(PeerId, Request<Ctx>),

    /// We received an invalid [`CommitCertificate`]
    InvalidCertificate(PeerId, CommitCertificate<Ctx>, CertificateError<Ctx>),
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

        Input::Decided(height) => on_decided(co, state, metrics, height).await,

        Input::ValueRequest(request_id, peer_id, request) => {
            on_value_request(co, state, metrics, request_id, peer_id, request).await
        }

        Input::ValueResponse(request_id, peer_id, Some(response)) => {
            on_value_response(co, state, metrics, request_id, peer_id, response).await
        }

        Input::ValueResponse(request_id, peer_id, None) => {
            on_invalid_value_response(co, state, metrics, request_id, peer_id).await
        }

        Input::BatchRequest(request_id, peer_id, request) => {
            on_batch_request(co, state, metrics, request_id, peer_id, request).await
        }

        Input::BatchResponse(request_id, peer_id, response) => {
            on_batch_response(co, state, metrics, request_id, peer_id, response).await
        }

        Input::GotDecidedValue(request_id, height, value) => {
            on_got_decided_value(co, state, metrics, request_id, height, value).await
        }

        Input::GotDecidedValues(request_id, range, values) => {
            on_got_decided_values(co, state, metrics, request_id, range, values).await
        }

        Input::SyncRequestTimedOut(peer_id, request) => {
            on_sync_request_timed_out(co, state, metrics, peer_id, request).await
        }

        Input::InvalidCertificate(peer, certificate, error) => {
            on_invalid_certificate(co, state, metrics, peer, certificate, error).await
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

    if let Some(inactive_threshold) = state.inactive_threshold {
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
    debug!(%status.peer_id, %status.tip_height, "Received peer status");

    let peer_height = status.tip_height;

    state.update_status(status);

    if !state.started {
        // Consensus has not started yet, no need to sync (yet).
        return Ok(());
    }

    if peer_height > state.tip_height {
        warn!(
            height.tip = %state.tip_height,
            height.sync = %state.sync_height,
            height.peer = %peer_height,
            "SYNC REQUIRED: Falling behind",
        );

        // We are lagging behind on one of our peers at least.
        // Request value(s) from any peer already at later height.
        request_values(co, state, metrics).await?;
    }

    Ok(())
}

pub async fn on_started_height<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    restart: bool,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let tip_height = height.decrement().unwrap_or(height);

    debug!(height.tip = %tip_height, height.sync = %height, %restart, "Starting new height");

    state.started = true;
    state.sync_height = height;
    state.tip_height = tip_height;

    // Check if there is any peer already at or above the height we just started,
    // and request value(s) from any of those peers in order to catch up.
    request_values(co, state, metrics).await?;

    Ok(())
}

pub async fn on_decided<Ctx>(
    _co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height.tip = %height, "Updating tip height");

    state.tip_height = height;
    state.remove_pending_value_request_by_height(&height);

    Ok(())
}

pub async fn on_value_request<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    peer: PeerId,
    request: ValueRequest<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request.height, %peer, "Received request for value");

    metrics.value_request_received(request.height.as_u64(), 1);

    perform!(
        co,
        Effect::GetDecidedValue(request_id, request.height, Default::default())
    );

    Ok(())
}

pub async fn on_batch_request<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    peer: PeerId,
    request: BatchRequest<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let start = request.range.start();
    let end = request.range.end();
    debug!(from_height = %start, to_height = %end, peer = %peer, "Received batch request");

    let batch_size = end.as_u64() - start.as_u64() + 1;
    metrics.value_request_received(start.as_u64(), batch_size);

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
    response: ValueResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%response.height, %request_id, %peer_id, "Received response");

    state.remove_pending_value_request_by_height(&response.height);

    let response_time = metrics.value_response_received(response.height.as_u64(), 1);

    if let Some(response_time) = response_time {
        let sync_result = response
            .value
            .as_ref()
            .map_or(SyncResult::Failure, |_| SyncResult::Success(response_time));

        state
            .peer_scorer
            .update_score_with_metrics(peer_id, sync_result, &metrics.scoring);
    }

    // We do not update the peer score if we do not know the response time.
    // This should never happen, but we need to handle it gracefully just in case.

    if response.value.is_none() {
        warn!(%response.height, %request_id, "Received invalid value response");

        // If we received an empty response, we will try to request the value from another peer.
        request_value_from_peer_except(co, state, metrics, response.height, peer_id).await?;
    }

    Ok(())
}

pub async fn on_batch_response<Ctx>(
    _co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    _request_id: OutboundRequestId,
    peer: PeerId,
    response: BatchResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let start = response.range.start().as_u64();
    let end = response.range.end().as_u64();
    let batch_size = end - start + 1;
    debug!(from = %start, to = %end, peer = %peer, "Received batch response");

    state.remove_pending_value_request_by_height_range(&response.range);

    let response_time = metrics.value_response_received(start, batch_size);

    if let Some(_response_time) = response_time {
        // TODO(SYNC): update peer score
    }

    // We do not update the peer score if we do not know the response time.
    // This should never happen, but we need to handle it gracefully just in case.

    if response.values.is_empty() {
        warn!(from = %start, to = %end, peer = %peer, "Received invalid response");

        // TODO(SYNC): request invalid values from another peer.
    }

    Ok(())
}

pub async fn on_invalid_value_response<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request_id, %peer, "Received invalid response");

    if let Some(height) = state.remove_pending_value_request_by_id(&request_id) {
        debug!(%height, %request_id, "Found which height this request was for");

        // If we have an associated height for this request, we will try again and request it from another peer.
        request_value_from_peer_except(co, state, metrics, height, peer).await?;
    }

    Ok(())
}

pub async fn on_got_decided_value<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    height: Ctx::Height,
    value: Option<RawDecidedValue<Ctx>>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let response = match value {
        None => {
            error!(%height, "Received empty value response from host");
            None
        }
        Some(value) if value.certificate.height != height => {
            error!(
                %height, value.height = %value.certificate.height,
                "Received value response for wrong height from host"
            );
            None
        }
        Some(value) => {
            info!(%height, "Received value response from host, sending it out");
            Some(value)
        }
    };

    perform!(
        co,
        Effect::SendValueResponse(
            request_id,
            ValueResponse::new(height, response),
            Default::default()
        )
    );

    metrics.value_response_sent(height.as_u64(), 1);

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
    info!(
        from_height = %range.start(),
        to_height = %range.end(),
        "Received batch response from host with {} values",
        values.len()
    );

    perform!(
        co,
        Effect::SendBatchResponse(
            request_id,
            BatchResponse::new(RangeInclusive::new(*range.start(), *range.end()), values),
            Default::default()
        )
    );

    let batch_size = range.end().as_u64() - range.start().as_u64() + 1;
    metrics.value_response_sent(range.start().as_u64(), batch_size);

    Ok(())
}

pub async fn on_sync_request_timed_out<Ctx>(
    _co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    peer_id: PeerId,
    request: Request<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    match request {
        Request::ValueRequest(value_request) => {
            let height = value_request.height;
            warn!(%peer_id, %height, "Value request timed out");

            state.remove_pending_value_request_by_height(&height);
            metrics.value_request_timed_out(height.as_u64());
        }
        Request::BatchRequest(batch_request) => {
            let mut height = *batch_request.range.start();
            warn!(%peer_id, from_height = %height, to_height = %batch_request.range.end(), "Batch request timed out");
            loop {
                state.remove_pending_value_request_by_height(&height);
                if height >= *batch_request.range.end() {
                    break;
                }
                height = height.increment();
            }
            metrics.value_request_timed_out(batch_request.range.start().as_u64());
        }
    };

    state.peer_scorer.update_score(peer_id, SyncResult::Timeout);

    Ok(())
}

async fn on_invalid_certificate<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    from: PeerId,
    certificate: CommitCertificate<Ctx>,
    error: CertificateError<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    error!(%error, %certificate.height, %certificate.round, "Received invalid certificate");
    trace!("Certificate: {certificate:#?}");

    state.peer_scorer.update_score(from, SyncResult::Failure);
    state.remove_pending_value_request_by_height(&certificate.height);

    request_value_from_peer_except(co, state, metrics, certificate.height, from).await
}

/// If there are no pending requests for the sync height,
/// and there is peer at a higher height than our sync height,
/// then sync from that peer.
async fn request_values<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let sync_height = state.sync_height;

    if state.has_pending_value_request(&sync_height) {
        warn!(height.sync = %sync_height, "Already have a pending value request for this height");
        return Ok(());
    }

    if let Some(peer) = state.random_peer_with_tip_at_or_above(sync_height) {
        request_values_from_peer(co, state, metrics, sync_height, peer).await?;
    } else {
        debug!(height.sync = %sync_height, "No peer to request sync from");
    }

    Ok(())
}

async fn request_values_from_peer<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    peer: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(height.sync = %height, %peer, "Requesting sync from peer");

    // Determine the batch size to use based on the peer's kind
    let batch_size = state
        .peers
        .get(&peer)
        .map(|peer_details| match peer_details.kind {
            PeerKind::SyncV1 => 1,
            PeerKind::SyncV2 => 100,
        })
        .unwrap_or(1);

    // Send the request
    let mut end_height = height;
    let request_id = if batch_size > 1 {
        end_height = end_height.increment_by(batch_size);
        let max_response_size = 10 * 1024 * 1024; // 10 MiB
        perform!(
            co,
            Effect::SendBatchRequest(peer, BatchRequest::new(RangeInclusive::new(height, end_height), max_response_size), Default::default()),
            Resume::ValueRequestId(id) => id,
        )
    } else {
        perform!(
            co,
            Effect::SendValueRequest(peer, ValueRequest::new(height), Default::default()),
            Resume::ValueRequestId(id) => id,
        )
    };

    metrics.value_request_sent(height.as_u64(), batch_size);

    // Store the request ID in the state
    if let Some(request_id) = request_id {
        debug!(%request_id, %peer, "Sent sync request to peer");
        state.store_pending_value_request(height, end_height, request_id);
    } else {
        warn!(height.sync = %height, %peer, "Failed to send sync request to peer");
    }

    Ok(())
}

async fn request_value_from_peer_except<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    except: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    info!(height.sync = %height, "Requesting sync from another peer");

    state.remove_pending_value_request_by_height(&height);

    if let Some(peer) = state.random_peer_with_tip_at_or_above_except(height, except) {
        request_values_from_peer(co, state, metrics, height, peer).await?;
    } else {
        error!(height.sync = %height, "No peer to request sync from");
    }

    Ok(())
}
