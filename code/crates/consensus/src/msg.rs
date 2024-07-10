use malachite_common::*;

use crate::mock::{GossipEvent, Multiaddr, NetworkMsg, PeerId, ReceivedProposedValue};

#[derive(Debug)]
pub enum Event<Ctx>
where
    Ctx: Context,
{
    Listening(Multiaddr),
    Message(PeerId, NetworkMsg<Ctx>),
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
}

pub enum Msg<Ctx>
where
    Ctx: Context,
{
    /// Start a new height
    StartHeight(Ctx::Height),

    /// Move to a give height
    MoveToHeight(Ctx::Height),

    /// Process a gossip event
    GossipEvent(GossipEvent<Ctx>),

    /// A timeout has elapsed
    TimeoutElapsed(Timeout),

    /// Decision has been made on a value at a given height and round
    Decided(Ctx::Height, Round, Ctx::Value),

    // The proposal builder has built a value and can be used in a new proposal consensus message
    ProposeValue(Ctx::Height, Round, Ctx::Value),

    // The proposal builder has build a new block part, needs to be signed and gossiped by consensus
    GossipBlockPart(Ctx::BlockPart),

    /// A proposal has been received
    ProposalReceived(ReceivedProposedValue<Ctx>),
}
