use core::fmt;

use libp2p::gossipsub;
use libp2p_broadcast as broadcast;
use serde::{Deserialize, Serialize};

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

    pub fn to_gossipsub_topic(self) -> gossipsub::Sha256Topic {
        // gossipsub::IdentTopic::new(self.as_str())
        gossipsub::Sha256Topic::new(self.as_str())
    }

    pub fn to_broadcast_topic(self) -> broadcast::Topic {
        broadcast::Topic::new(self.as_str().as_bytes())
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Consensus => "consensus_votes",
            Channel::ProposalParts => "consensus_proposals",
            Channel::Sync => "sync",
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
        match topic.as_str() {
            "consensus_votes" => Some(Channel::Consensus),
            "consensus_proposals" => Some(Channel::ProposalParts),
            "sync" => Some(Channel::Sync),
            _ => None,
        }
    }

    pub fn from_broadcast_topic(topic: &broadcast::Topic) -> Option<Self> {
        match topic.as_ref() {
            b"consensus_votes" => Some(Channel::Consensus),
            b"consensus_proposals" => Some(Channel::ProposalParts),
            b"sync" => Some(Channel::Sync),
            _ => None,
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_str().fmt(f)
    }
}
