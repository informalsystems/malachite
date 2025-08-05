//! Direct re-export of the external fifo-mempool crate

// Re-export the external fifo-mempool crate (specific imports to avoid conflicts)
pub use mempool::{Mempool, MempoolActorRef, MempoolConfig, MempoolMsg};

// Type aliases for compatibility
pub type MempoolRef = ::mempool::MempoolActorRef;

// Re-export the external libp2p-network crate (specific imports to avoid conflicts)
pub use libp2p_network::network::{MempoolNetwork, MempoolNetworkActorRef};

// Type aliases for compatibility
pub type MempoolNetworkRef = ::libp2p_network::network::MempoolNetworkActorRef;
