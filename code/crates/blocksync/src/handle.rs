use core::marker::PhantomData;

use derive_where::derive_where;
use displaydoc::Display;
use tracing::{debug, error, info, warn};

use malachite_common::{Context, Proposal};

use crate::co::Co;
use crate::perform;
use crate::{InboundRequestId, Metrics, PeerId, Request, Response, State, Status, SyncedBlock};

#[derive_where(Debug)]
#[derive(Display)]
pub enum Error<Ctx: Context> {
    /// The coroutine was resumed with a value which
    /// does not match the expected type of resume value.
    #[displaydoc("Unexpected resume: {0:?}, expected one of: {1}")]
    UnexpectedResume(Resume<Ctx>, &'static str),
}

impl<Ctx: Context> core::error::Error for Error<Ctx> {}

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
    /// Publish our status to the network
    PublishStatus(Ctx::Height),

    /// Send a BlockSync request to a peer
    SendRequest(PeerId, Request<Ctx>),

    /// Send a response to a BlockSync request
    SendResponse(InboundRequestId, Response<Ctx>),

    /// Retrieve a block from the application
    GetBlock(InboundRequestId, Ctx::Height),
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
    Decided(Ctx::Height),

    /// A BlockSync request has been received from a peer
    Request(InboundRequestId, PeerId, Request<Ctx>),

    /// Got a response from the application to our `GetBlock` request
    GotBlock(InboundRequestId, Ctx::Height, Option<SyncedBlock<Ctx>>),

    /// A request timed out
    RequestTimedOut(PeerId, Request<Ctx>),
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
        Input::Decided(height) => on_decided(co, state, metrics, height).await,
        Input::Request(request_id, peer_id, request) => {
            on_request(co, state, metrics, request_id, peer_id, request).await
        }
        Input::GotBlock(request_id, height, block) => {
            on_block(co, state, metrics, request_id, height, block).await
        }
        Input::RequestTimedOut(peer_id, request) => {
            on_request_timed_out(co, state, metrics, peer_id, request).await
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
    debug!(height = %state.tip_height, "Publishing status");

    perform!(co, Effect::PublishStatus(state.tip_height));

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
        request_sync(co, state, metrics).await?;
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_request<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    _metrics: &Metrics,
    request_id: InboundRequestId,
    peer: PeerId,
    request: Request<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    debug!(height = %request.height, %peer, "Received request for block");

    perform!(co, Effect::GetBlock(request_id, request.height));

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
    request_sync(co, state, metrics).await?;

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
    debug!(%height, "Decided on a block");

    state.tip_height = height;
    state.remove_pending_request(height);

    Ok(())
}

pub async fn on_block<Ctx>(
    co: Co<Ctx>,
    _state: &mut State<Ctx>,
    _metrics: &Metrics,
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
        Some(block) if block.proposal.height() != height => {
            error!(
                %height, block.height = %block.proposal.height(),
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
        Effect::SendResponse(request_id, Response::new(response))
    );

    Ok(())
}

pub async fn on_request_timed_out<Ctx>(
    _co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
    peer_id: PeerId,
    request: Request<Ctx>,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    warn!(%peer_id, %request.height, "Request timed out");

    state.remove_pending_request(request.height);

    Ok(())
}

/// If there are no pending requests for the sync height,
/// and there is peer at a higher height than our sync height,
/// then sync from that peer.
async fn request_sync<Ctx>(
    co: Co<Ctx>,
    state: &mut State<Ctx>,
    _metrics: &Metrics,
) -> Result<(), Error<Ctx>>
where
    Ctx: Context,
{
    let sync_height = state.sync_height;

    if state.has_pending_request(&sync_height) {
        debug!(sync.height = %sync_height, "Already have a pending request for this height");
        return Ok(());
    }

    if let Some(peer) = state.random_peer_at_or_above(sync_height) {
        debug!(sync.height = %sync_height, %peer, "Requesting block from peer");

        perform!(co, Effect::SendRequest(peer, Request::new(sync_height)));

        state.store_pending_request(sync_height, peer);
    }

    Ok(())
}