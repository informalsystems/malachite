use informalsystems_malachitebft_test::{Height, TestContext};
use malachitebft_sync::PeerKind::{SyncV1, SyncV2};
use malachitebft_sync::{PeerId, PeerInfo, PeerKind, State, Status};
use std::collections::BTreeMap;
use std::str::FromStr;

fn add_peer(
    peers: &mut BTreeMap<PeerId, PeerInfo<TestContext>>,
    peer_id: PeerId,
    kind: PeerKind,
    min: u64,
    max: u64,
) {
    peers.insert(
        peer_id,
        PeerInfo {
            kind,
            status: Status::<TestContext> {
                peer_id,
                tip_height: Height::new(max),
                history_min_height: Height::new(min),
            },
        },
    );
}

#[test]
fn filtering_peers() {
    let peer1 = PeerId::from_str("12D3KooWEPvZRT1FQXVpgXVsUsi76VV8ahjG7bZSeMY3JvkbDYk1").unwrap();
    let peer2 = PeerId::from_str("12D3KooWEPvZRT1FQXVpgXVsUsi76VV8ahjG7bZSeMY3JvkbDYk2").unwrap();
    let peer3 = PeerId::from_str("12D3KooWEPvZRT1FQXVpgXVsUsi76VV8ahjG7bZSeMY3JvkbDYk3").unwrap();
    let peer4 = PeerId::from_str("12D3KooWEPvZRT1FQXVpgXVsUsi76VV8ahjG7bZSeMY3JvkbDYk4").unwrap();

    let range1 = Height::new(3)..=Height::new(5);
    let range2 = Height::new(3)..=Height::new(10);
    let range3 = Height::new(11)..=Height::new(20);

    let mut peers = BTreeMap::new();

    // Only v2 peers
    add_peer(&mut peers, peer1, SyncV2, 1, 5);
    add_peer(&mut peers, peer2, SyncV2, 1, 10);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range1, None);
    assert_eq!(filtered_peers.len(), 2);
    assert!(filtered_peers.contains_key(&peer1));
    assert!(filtered_peers.contains_key(&peer2));
    assert_eq!(filtered_peers.get(&peer1).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer1).unwrap().end().as_u64(), 5);
    assert_eq!(filtered_peers.get(&peer2).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer2).unwrap().end().as_u64(), 5);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range1, Some(peer1));
    assert_eq!(filtered_peers.len(), 1);
    assert!(!filtered_peers.contains_key(&peer1));
    assert!(filtered_peers.contains_key(&peer2));
    assert_eq!(filtered_peers.get(&peer2).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer2).unwrap().end().as_u64(), 5);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range2, None);
    assert_eq!(filtered_peers.len(), 1);
    assert!(!filtered_peers.contains_key(&peer1));
    assert!(filtered_peers.contains_key(&peer2));
    assert_eq!(filtered_peers.get(&peer2).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer2).unwrap().end().as_u64(), 10);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range3, None);
    assert!(filtered_peers.is_empty());

    // A mix of v1 and v2 peers
    add_peer(&mut peers, peer3, SyncV1, 1, 5);
    add_peer(&mut peers, peer4, SyncV1, 1, 10);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range1, None);
    assert_eq!(filtered_peers.len(), 2);
    assert!(filtered_peers.contains_key(&peer1));
    assert!(filtered_peers.contains_key(&peer2));
    assert!(!filtered_peers.contains_key(&peer3));
    assert!(!filtered_peers.contains_key(&peer4));

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range1, Some(peer1));
    assert_eq!(filtered_peers.len(), 1);
    assert!(!filtered_peers.contains_key(&peer1));
    assert!(filtered_peers.contains_key(&peer2));
    assert!(!filtered_peers.contains_key(&peer3));
    assert!(!filtered_peers.contains_key(&peer4));

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range3, None);
    assert!(filtered_peers.is_empty());

    // Only v1 peers
    peers.remove(&peer1);
    peers.remove(&peer2);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range2, None);
    assert_eq!(filtered_peers.len(), 2);
    assert!(!filtered_peers.contains_key(&peer1));
    assert!(!filtered_peers.contains_key(&peer2));
    assert!(filtered_peers.contains_key(&peer3));
    assert!(filtered_peers.contains_key(&peer4));
    assert_eq!(filtered_peers.get(&peer3).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer3).unwrap().end().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer4).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer4).unwrap().end().as_u64(), 3);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range2, Some(peer3));
    assert_eq!(filtered_peers.len(), 1);
    assert!(!filtered_peers.contains_key(&peer1));
    assert!(!filtered_peers.contains_key(&peer2));
    assert!(!filtered_peers.contains_key(&peer3));
    assert!(filtered_peers.contains_key(&peer4));
    assert_eq!(filtered_peers.get(&peer4).unwrap().start().as_u64(), 3);
    assert_eq!(filtered_peers.get(&peer4).unwrap().end().as_u64(), 3);

    let filtered_peers = State::<TestContext>::filter_peers_by_range(&peers, &range3, None);
    assert!(filtered_peers.is_empty());
}
