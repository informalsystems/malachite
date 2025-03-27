use core::fmt;
use std::sync::OnceLock;

use futures::channel;
use libp2p::gossipsub;
use libp2p_broadcast as broadcast;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy)]
pub struct ChannelNames {
    pub consensus: &'static str,
    pub proposal_parts: &'static str,
    pub sync: &'static str,
}

impl Default for ChannelNames {
    fn default() -> Self {
        Self {
            consensus: "consensus_votes",
            proposal_parts: "consensus_proposals",
            sync: "sync",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    Consensus,
    ProposalParts,
    Sync,
}

impl Channel {
    pub fn all() -> &'static [Channel] {
        &[Channel::Consensus, Channel::ProposalParts, Channel::Sync]
    }

    pub fn consensus() -> &'static [Channel] {
        &[Channel::Consensus, Channel::ProposalParts]
    }

    pub fn to_gossipsub_topic(self, channel_names: ChannelNames) -> gossipsub::Sha256Topic {
        // gossipsub::IdentTopic::new(self.as_str())
        gossipsub::Sha256Topic::new(self.as_str(channel_names))
    }

    pub fn to_broadcast_topic(self, channel_names: ChannelNames) -> broadcast::Topic {
        broadcast::Topic::new(self.as_str(channel_names).as_bytes())
    }

    pub fn as_str(&self, channel_names: ChannelNames) -> &'static str {
        match self {
            Channel::Consensus => channel_names.consensus,
            Channel::ProposalParts => channel_names.proposal_parts,
            Channel::Sync => channel_names.sync,
        }
    }

    pub fn has_gossipsub_topic(
        topic_hash: &gossipsub::TopicHash,
        channel_names: ChannelNames,
    ) -> bool {
        Self::all()
            .iter()
            .any(|channel| &channel.to_gossipsub_topic(channel_names).hash() == topic_hash)
    }

    pub fn has_broadcast_topic(topic: &broadcast::Topic, channel_names: ChannelNames) -> bool {
        Self::all()
            .iter()
            .any(|channel| &channel.to_broadcast_topic(channel_names) == topic)
    }

    pub fn from_gossipsub_topic_hash(
        topic: &gossipsub::TopicHash,
        channel_names: ChannelNames,
    ) -> Option<Self> {
        if topic == &Self::Consensus.to_gossipsub_topic(channel_names).hash() {
            Some(Self::Consensus)
        } else if topic == &Self::ProposalParts.to_gossipsub_topic(channel_names).hash() {
            Some(Self::ProposalParts)
        } else if topic == &Self::Sync.to_gossipsub_topic(channel_names).hash() {
            Some(Self::Sync)
        } else {
            None
        }
    }

    pub fn from_broadcast_topic(
        topic: &broadcast::Topic,
        channel_names: ChannelNames,
    ) -> Option<Self> {
        match topic.as_ref() {
            name if name == channel_names.consensus.as_bytes() => Some(Self::Consensus),
            name if name == channel_names.proposal_parts.as_bytes() => Some(Self::ProposalParts),
            name if name == channel_names.sync.as_bytes() => Some(Self::Sync),
            _ => None,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // TODO: how to use the correct channel names?
        self.as_str(ChannelNames::default()).fmt(f)
    }
}
