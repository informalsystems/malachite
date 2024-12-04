use core::marker::PhantomData;

use derive_where::derive_where;
use thiserror::Error;
use tracing::{debug, error, info, trace, warn};

use malachite_common::{CertificateError, CommitCertificate, Context, Height, Round};

use crate::co::Co;
use crate::{
    perform, BlockRequest, BlockResponse, InboundRequestId, Metrics, OutboundRequestId, PeerId,
    Request, State, Status, SyncedBlock, VoteSetRequest, VoteSetResponse,
};

#[derive_where(Debug)]
#[derive(Error)]
pub enum Error<Ctx: Context> {
    /// The coroutine was resumed with a value which
    /// does not match the expected type of resume value.
    #[error("Unexpected resume: {0:?}, expected one of: {1}")]
    UnexpectedResume(Resume<Ctx>, &'static str),
}

#[derive_where(Debug)]
pub enum Resume<Ctx: Context> {
    Continue(PhantomData<Ctx>),
}

impl<Ctx: Context> Default for Resume<Ctx> {
    fn default() -> Self {
        Self::Continue(PhantomData)
    }
}

#[derive_where(Debug)]
pub enum Effect<Ctx: Context> {
    /// Broadcast our status to our direct peers
    BroadcastStatus(Ctx::Height),

    /// Send a BlockSync request to a peer
    SendBlockRequest(PeerId, BlockRequest<Ctx>),

    /// Send a response to a BlockSync request
    SendBlockResponse(InboundRequestId, BlockResponse<Ctx>),

    /// Retrieve a block from the application
    GetBlock(InboundRequestId, Ctx::Height),

    /// Send a VoteSet request to a peer
    SendVoteSetRequest(PeerId, VoteSetRequest<Ctx>),
}

#[derive_where(Debug)]
pub enum Input<Ctx: Context> {
    /// A tick has occurred
    Tick,

    /// A status update has been received from a peer
    Status(Status<Ctx>),

    /// Consensus just started a new height
    StartHeight(Ctx::Height),

    /// Consensus just decided on a new block
    UpdateHeight(Ctx::Height),

    /// A BlockSync request has been received from a peer
    BlockRequest(InboundRequestId, PeerId, BlockRequest<Ctx>),

    /// A BlockSync response has been received
    BlockResponse(OutboundRequestId, PeerId, BlockResponse<Ctx>),

    /// Got a response from the application to our `GetBlock` request
    GotBlock(InboundRequestId, Ctx::Height, Option<SyncedBlock<Ctx>>),

    /// A request for a block or vote set timed out
    SyncRequestTimedOut(PeerId, Request<Ctx>),

    /// We received an invalid [`CommitCertificate`]
    InvalidCertificate(PeerId, CommitCertificate<Ctx>, CertificateError<Ctx>),

    /// Consensus needs a vote set for the height and round for recovery.
    GetVoteSet(Ctx::Height, Round),

    /// A VoteSet request has been received from a peer
    VoteSetRequest(InboundRequestId, PeerId, VoteSetRequest<Ctx>),

    /// Got a response from consensus for an incoming request
    GotVoteSet(InboundRequestId, Ctx::Height, Round),

    /// A VoteSet response has been received
    VoteSetResponse(OutboundRequestId, PeerId, VoteSetResponse<Ctx>),
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

        Input::StartHeight(height) => on_start_height(co, state, metrics, height).await,

        Input::UpdateHeight(height) => on_update_height(co, state, metrics, height).await,

        Input::BlockRequest(request_id, peer_id, request) => {
            on_block_request(co, state, metrics, request_id, peer_id, request).await
        }
        Input::BlockResponse(request_id, peer_id, response) => {
            on_block_response(co, state, metrics, request_id, peer_id, response).await
        }
        Input::GotBlock(request_id, height, block) => {
            on_block(co, state, metrics, request_id, height, block).await
        }
        Input::SyncRequestTimedOut(peer_id, request) => {
            on_sync_request_timed_out(co, state, metrics, peer_id, request).await
        }
        Input::InvalidCertificate(peer, certificate, error) => {
            on_invalid_certificate(co, state, metrics, peer, certificate, error).await
        }
        Input::GetVoteSet(height, round) => {
            on_get_vote_set(co, state, metrics, height, round).await
        }
        Input::VoteSetRequest(request_id, peer_id, request) => {
            on_vote_set_request(co, state, metrics, request_id, peer_id, request).await
        }

        Input::VoteSetResponse(request_id, peer_id, response) => {
            on_vote_set_response(co, state, metrics, request_id, peer_id, response).await
        }

        Input::GotVoteSet(request_id, height, round) => {
            on_vote_set(co, state, metrics, request_id, height, round).await
        }
    }
}

#[tracing::instrument(skip_all)]
pub async fn on_tick<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height = %state.tip_height, "Broadcasting status");

    perform!(co, Effect::BroadcastStatus(state.tip_height));

    Ok(())
}

#[tracing::instrument(
    skip_all,
    fields(
        sync_height = %state.sync_height,
        tip_height = %state.tip_height
    )
)]
pub async fn on_status<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    status: Status<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%status.peer_id, %status.height, "Received peer status");

    let peer_height = status.height;

    state.update_status(status);

    if peer_height > state.tip_height {
        info!(
            tip.height = %state.tip_height,
            sync.height = %state.sync_height,
            peer.height = %peer_height,
            "SYNC REQUIRED: Falling behind"
        );

        // We are lagging behind one of our peer at least,
        // request sync from any peer already at or above that peer's height.
        request_block(co, state, metrics).await?;
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_block_request<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    peer: PeerId,
    request: BlockRequest<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height = %request.height, %peer, "Received request for block");

    metrics.decided_block_request_received(request.height.as_u64());

    perform!(co, Effect::GetBlock(request_id, request.height));

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_block_response<Ctx>(
    _co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer: PeerId,
    response: BlockResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height = %response.height, %request_id, %peer, "Received response");

    metrics.decided_block_response_received(response.height.as_u64());

    Ok(())
}

pub async fn on_start_height<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%height, "Starting new height");

    state.sync_height = height;

    // Check if there is any peer already at or above the height we just started,
    // and request sync from that peer in order to catch up.
    request_block(co, state, metrics).await?;

    Ok(())
}

pub async fn on_update_height<Ctx>(
    _co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
    height: Ctx::Height,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.tip_height < height {
        debug!(%height, "Update height");

        state.tip_height = height;
        state.remove_pending_decided_block_request(height);
    }

    Ok(())
}

pub async fn on_block<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    height: Ctx::Height,
    block: Option<SyncedBlock<Ctx>>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let response = match block {
        None => {
            error!(%height, "Received empty response");
            None
        }
        Some(block) if block.certificate.height != height => {
            error!(
                %height, block.height = %block.certificate.height,
                "Received block for wrong height"
            );
            None
        }
        Some(block) => {
            debug!(%height, "Received decided block");
            Some(block)
        }
    };

    perform!(
        co,
        Effect::SendBlockResponse(request_id, BlockResponse::new(height, response))
    );

    metrics.decided_block_response_sent(height.as_u64());

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
        Request::BlockRequest(block_request) => {
            let height = block_request.height;
            warn!(%peer_id, %height, "Block request timed out");
            state.remove_pending_decided_block_request(height);
            metrics.decided_block_request_timed_out(height.as_u64());
        }
        Request::VoteSetRequest(vote_set_request) => {
            let height = vote_set_request.height;
            let round = vote_set_request.round;
            warn!(%peer_id, %height, %round, "Vote set request timed out");
            state.remove_pending_vote_set_request(height, round);
            metrics.vote_set_request_timed_out(height.as_u64(), round.as_i64());
        }
    };

    Ok(())
}

/// If there are no pending requests for the sync height,
/// and there is peer at a higher height than our sync height,
/// then sync from that peer.
async fn request_block<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let sync_height = state.sync_height;

    if state.has_pending_decided_block_request(&sync_height) {
        debug!(sync.height = %sync_height, "Already have a pending block request for this height");
        return Ok(());
    }

    if let Some(peer) = state.random_peer_with_block(sync_height) {
        request_block_from_peer(co, state, metrics, sync_height, peer).await?;
    }

    Ok(())
}

async fn request_block_from_peer<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    peer: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(sync.height = %height, %peer, "Requesting block from peer");

    perform!(
        co,
        Effect::SendBlockRequest(peer, BlockRequest::new(height))
    );

    metrics.decided_block_request_sent(height.as_u64());
    state.store_pending_decided_block_request(height, peer);

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

    info!("Requesting sync from another peer");
    state.remove_pending_decided_block_request(certificate.height);

    let Some(peer) = state.random_peer_with_block_except(certificate.height, from) else {
        error!("No other peer to request sync from");
        return Ok(());
    };

    request_block_from_peer(co, state, metrics, certificate.height, peer).await
}

pub async fn on_get_vote_set<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    round: Round,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    if state.has_pending_vote_set_request(height, round) {
        debug!(vote_set.height = %height, "Already have a pending vote set request for this height");
        return Ok(());
    }

    debug!(
        "VS4 - send vote set request to peer, number of peers {}",
        state.peers.len()
    );

    let Some(peer) = state.random_peer_for_votes() else {
        error!("No other peer to request vote set from");
        return Ok(());
    };

    request_vote_set_from_peer(co, state, metrics, height, round, peer).await?;

    Ok(())
}

async fn request_vote_set_from_peer<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    metrics: &Metrics,
    height: Ctx::Height,
    round: Round,
    peer: PeerId,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(vote_set.height = %height, vote_set.round = %round, %peer, "Requesting vote set from peer");

    perform!(
        co,
        Effect::SendVoteSetRequest(peer, VoteSetRequest::new(height, round))
    );

    metrics.vote_set_request_sent(height.as_u64(), round.as_i64());

    state.store_pending_vote_set_request(height, round, peer);

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_vote_set_request<Ctx>(
    _co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    peer: PeerId,
    request: VoteSetRequest<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request_id, %peer, height = %request.height, round = %request.round, "Received request for vote set");

    metrics.vote_set_request_received(request.height.as_u64(), request.round.as_i64());

    Ok(())
}

pub async fn on_vote_set<Ctx>(
    _co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: InboundRequestId,
    height: Ctx::Height,
    round: Round,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request_id, %height, %round, "Vote set response sent");

    metrics.vote_set_response_sent(height.as_u64(), round.as_i64());

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_vote_set_response<Ctx>(
    _co: Co<Ctx>,
    _state: &mut State<Ctx>,
    metrics: &Metrics,
    request_id: OutboundRequestId,
    peer: PeerId,
    response: VoteSetResponse<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(%request_id, %peer, height = %response.height, round = %response.round, "Received vote set response");

    metrics.vote_set_response_received(response.height.as_u64(), response.round.as_i64());

    Ok(())
}
