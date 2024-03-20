use malachite_gossip::Channel;
pub use malachite_gossip::{spawn, Config, CtrlMsg, Handle, HandleEvent, Keypair};

use super::{Msg, Network, PeerId};

#[async_trait::async_trait]
impl Network for Handle {
    async fn recv(&mut self) -> Option<(PeerId, Msg)> {
        loop {
            match Handle::recv(self).await {
                Some(HandleEvent::Message(peer_id, Channel::Consensus, data)) => {
                    let msg = Msg::from_network_bytes(&data).unwrap();
                    let peer_id = PeerId::new(peer_id.to_string());
                    return Some((peer_id, msg));
                }
                _ => continue,
            }
        }
    }

    async fn broadcast(&mut self, msg: Msg) {
        let data = msg.to_network_bytes().unwrap();
        Handle::broadcast(self, Channel::Consensus, data)
            .await
            .unwrap();
    }
}
