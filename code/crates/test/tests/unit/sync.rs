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

    struct TestCase {
        name: &'static str,
        peers: Vec<(PeerId, PeerKind, u64, u64)>,
        range: std::ops::RangeInclusive<Height>,
        exclude_peer: Option<PeerId>,
        expected_peers: Vec<PeerId>,
        expected_ranges: Vec<(u64, u64)>, // (start, end) for each expected peer
    }

    let test_cases = vec![
        TestCase {
            name: "only v2 peers, range1, no exclusion",
            peers: vec![(peer1, SyncV2, 1, 5), (peer2, SyncV2, 1, 10)],
            range: range1.clone(),
            exclude_peer: None,
            expected_peers: vec![peer1, peer2],
            expected_ranges: vec![(3, 5), (3, 5)],
        },
        TestCase {
            name: "only v2 peers, range1, exclude peer1",
            peers: vec![(peer1, SyncV2, 1, 5), (peer2, SyncV2, 1, 10)],
            range: range1.clone(),
            exclude_peer: Some(peer1),
            expected_peers: vec![peer2],
            expected_ranges: vec![(3, 5)],
        },
        TestCase {
            name: "only v2 peers, range2, no exclusion",
            peers: vec![(peer1, SyncV2, 1, 5), (peer2, SyncV2, 1, 10)],
            range: range2.clone(),
            exclude_peer: None,
            expected_peers: vec![peer2],
            expected_ranges: vec![(3, 10)],
        },
        TestCase {
            name: "only v2 peers, range3, no exclusion",
            peers: vec![(peer1, SyncV2, 1, 5), (peer2, SyncV2, 1, 10)],
            range: range3.clone(),
            exclude_peer: None,
            expected_peers: vec![],
            expected_ranges: vec![],
        },
        TestCase {
            name: "mix of v1 and v2 peers, range1, no exclusion",
            peers: vec![
                (peer1, SyncV2, 1, 5),
                (peer2, SyncV2, 1, 10),
                (peer3, SyncV1, 1, 5),
                (peer4, SyncV1, 1, 10),
            ],
            range: range1.clone(),
            exclude_peer: None,
            expected_peers: vec![peer1, peer2],
            expected_ranges: vec![(3, 5), (3, 5)],
        },
        TestCase {
            name: "mix of v1 and v2 peers, range1, exclude peer1",
            peers: vec![
                (peer1, SyncV2, 1, 5),
                (peer2, SyncV2, 1, 10),
                (peer3, SyncV1, 1, 5),
                (peer4, SyncV1, 1, 10),
            ],
            range: range1.clone(),
            exclude_peer: Some(peer1),
            expected_peers: vec![peer2],
            expected_ranges: vec![(3, 5)],
        },
        TestCase {
            name: "mix of v1 and v2 peers, range3, no exclusion",
            peers: vec![
                (peer1, SyncV2, 1, 5),
                (peer2, SyncV2, 1, 10),
                (peer3, SyncV1, 1, 5),
                (peer4, SyncV1, 1, 10),
            ],
            range: range3.clone(),
            exclude_peer: None,
            expected_peers: vec![],
            expected_ranges: vec![],
        },
        TestCase {
            name: "only v1 peers, range2, no exclusion",
            peers: vec![(peer3, SyncV1, 1, 5), (peer4, SyncV1, 1, 10)],
            range: range2.clone(),
            exclude_peer: None,
            expected_peers: vec![peer3, peer4],
            expected_ranges: vec![(3, 3), (3, 3)],
        },
        TestCase {
            name: "only v1 peers, range2, exclude peer3",
            peers: vec![(peer3, SyncV1, 1, 5), (peer4, SyncV1, 1, 10)],
            range: range2.clone(),
            exclude_peer: Some(peer3),
            expected_peers: vec![peer4],
            expected_ranges: vec![(3, 3)],
        },
        TestCase {
            name: "only v1 peers, range3, no exclusion",
            peers: vec![(peer3, SyncV1, 1, 5), (peer4, SyncV1, 1, 10)],
            range: range3.clone(),
            exclude_peer: None,
            expected_peers: vec![],
            expected_ranges: vec![],
        },
    ];

    for test_case in test_cases {
        let mut peers = BTreeMap::new();

        // Setup peers for this test case
        for (peer_id, kind, min, max) in &test_case.peers {
            add_peer(&mut peers, *peer_id, *kind, *min, *max);
        }

        let filtered_peers = State::<TestContext>::filter_peers_by_range(
            &peers,
            &test_case.range,
            test_case.exclude_peer,
        );

        // Verify expected number of peers
        assert_eq!(
            filtered_peers.len(),
            test_case.expected_peers.len(),
            "Test case '{}': expected {} peers, got {}",
            test_case.name,
            test_case.expected_peers.len(),
            filtered_peers.len()
        );

        // Verify each expected peer is present with correct range
        for (i, expected_peer) in test_case.expected_peers.iter().enumerate() {
            assert!(
                filtered_peers.contains_key(expected_peer),
                "Test case '{}': expected peer {:?} not found",
                test_case.name,
                expected_peer
            );

            let peer_range = filtered_peers.get(expected_peer).unwrap();
            let (expected_start, expected_end) = test_case.expected_ranges[i];

            assert_eq!(
                peer_range.start().as_u64(),
                expected_start,
                "Test case '{}': peer {:?} has wrong start, expected {}, got {}",
                test_case.name,
                expected_peer,
                expected_start,
                peer_range.start().as_u64()
            );

            assert_eq!(
                peer_range.end().as_u64(),
                expected_end,
                "Test case '{}': peer {:?} has wrong end, expected {}, got {}",
                test_case.name,
                expected_peer,
                expected_end,
                peer_range.end().as_u64()
            );
        }
    }
}
