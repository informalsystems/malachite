use core::fmt;
use std::sync::OnceLock;

use futures::channel;
use libp2p::gossipsub;
use libp2p_broadcast as broadcast;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
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

static CHANNEL_NAMES: OnceLock<ChannelNames> = OnceLock::new();

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    Consensus,
    ProposalParts,
    Sync,
}

impl Channel {
    pub fn init_channel_names(channel_names: ChannelNames) -> Result<(), ChannelNames> {
        CHANNEL_NAMES.set(channel_names)
    }

    fn get_channel_names() -> &'static ChannelNames {
        CHANNEL_NAMES.get_or_init(ChannelNames::default)
    }

    pub fn all() -> &'static [Channel] {
        &[Channel::Consensus, Channel::ProposalParts, Channel::Sync]
    }

    pub fn consensus() -> &'static [Channel] {
        &[Channel::Consensus, Channel::ProposalParts]
    }

    pub fn to_gossipsub_topic(self) -> gossipsub::Sha256Topic {
        // gossipsub::IdentTopic::new(self.as_str())
        gossipsub::Sha256Topic::new(self.as_str())
    }

    pub fn to_broadcast_topic(self) -> broadcast::Topic {
        broadcast::Topic::new(self.as_str().as_bytes())
    }

    pub fn as_str(&self) -> &'static str {
        let channel_names = Self::get_channel_names();
        match self {
            Channel::Consensus => channel_names.consensus,
            Channel::ProposalParts => channel_names.proposal_parts,
            Channel::Sync => channel_names.sync,
        }
    }

    pub fn has_gossipsub_topic(topic_hash: &gossipsub::TopicHash) -> bool {
        Self::all()
            .iter()
            .any(|channel| &channel.to_gossipsub_topic().hash() == topic_hash)
    }

    pub fn has_broadcast_topic(topic: &broadcast::Topic) -> bool {
        Self::all()
            .iter()
            .any(|channel| &channel.to_broadcast_topic() == topic)
    }

    pub fn from_gossipsub_topic_hash(topic: &gossipsub::TopicHash) -> Option<Self> {
        if topic == &Self::Consensus.to_gossipsub_topic().hash() {
            Some(Self::Consensus)
        } else if topic == &Self::ProposalParts.to_gossipsub_topic().hash() {
            Some(Self::ProposalParts)
        } else if topic == &Self::Sync.to_gossipsub_topic().hash() {
            Some(Self::Sync)
        } else {
            None
        }
    }

    pub fn from_broadcast_topic(topic: &broadcast::Topic) -> Option<Self> {
        let channel_names = Self::get_channel_names();
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
        self.as_str().fmt(f)
    }
}
