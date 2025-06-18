use std::{ops::RangeInclusive, sync::Arc};

use bytes::Bytes;
use derive_where::derive_where;
use displaydoc::Display;
use libp2p::request_response;
use serde::{Deserialize, Serialize};

use malachitebft_core_types::{CommitCertificate, Context, Height};
pub use malachitebft_peer::PeerId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
#[displaydoc("{0}")]
pub struct InboundRequestId(Arc<str>);

impl InboundRequestId {
    pub fn new(id: impl ToString) -> Self {
        Self(Arc::from(id.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
#[displaydoc("{0}")]
pub struct OutboundRequestId(Arc<str>);

impl OutboundRequestId {
    pub fn new(id: impl ToString) -> Self {
        Self(Arc::from(id.to_string()))
    }
}

pub type ResponseChannel = request_response::ResponseChannel<RawResponse>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PeerKind {
    SyncV1,
    SyncV2,
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct Status<Ctx: Context> {
    pub peer_id: PeerId,
    pub tip_height: Ctx::Height,
    pub history_min_height: Ctx::Height,
}

impl<Ctx: Context> Status<Ctx> {
    pub(crate) fn default(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            tip_height: Ctx::Height::ZERO,
            history_min_height: Ctx::Height::ZERO,
        }
    }
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct PeerDetails<Ctx: Context> {
    /// The kind of protocol the peer supports.
    pub kind: PeerKind,
    /// The peer's status.
    pub status: Status<Ctx>,
}

impl<Ctx: Context> PeerDetails<Ctx> {
    pub(crate) fn update_status(&mut self, status: Status<Ctx>) {
        self.status = status;
    }
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Request<Ctx: Context> {
    ValueRequest(ValueRequest<Ctx>),
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Response<Ctx: Context> {
    ValueResponse(ValueResponse<Ctx>),
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct ValueRequest<Ctx: Context> {
    pub range: RangeInclusive<Ctx::Height>,
}

impl<Ctx: Context> ValueRequest<Ctx> {
    pub fn new(range: RangeInclusive<Ctx::Height>) -> Self {
        Self { range }
    }
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct ValueResponse<Ctx: Context> {
    pub range: RangeInclusive<Ctx::Height>,
    pub values: Vec<RawDecidedValue<Ctx>>,
}

impl<Ctx: Context> ValueResponse<Ctx> {
    pub fn new(range: RangeInclusive<Ctx::Height>, values: Vec<RawDecidedValue<Ctx>>) -> Self {
        Self { range, values }
    }
}

#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct RawDecidedValue<Ctx: Context> {
    pub value_bytes: Bytes,
    pub certificate: CommitCertificate<Ctx>,
}

impl<Ctx: Context> RawDecidedValue<Ctx> {
    pub fn new(value_bytes: Bytes, certificate: CommitCertificate<Ctx>) -> Self {
        Self {
            value_bytes,
            certificate,
        }
    }
}

#[derive(Clone, Debug)]
pub enum RawMessage {
    Request {
        request_id: request_response::InboundRequestId,
        peer: PeerId,
        body: Bytes,
    },
    Response {
        request_id: request_response::OutboundRequestId,
        peer: PeerId,
        body: Bytes,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRequest(pub Bytes);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawResponse(pub Bytes);
