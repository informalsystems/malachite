use std::collections::HashMap;

use malachite_common::NilOrVal;
use malachite_itf::types::Value;
use malachite_test::{Address, PrivateKey, PublicKey, TestContext, ValueId};
use rand::{CryptoRng, RngCore};

pub const VALIDATORS: [&str; 3] = ["alice", "bob", "john"];

pub fn build_public_key_map<R>(mut rng: R) -> HashMap<String, PublicKey>
where
    R: RngCore + CryptoRng,
{
    VALIDATORS
        .iter()
        .map(|&name| {
            let pk = PrivateKey::generate(&mut rng).public_key();
            (name.to_string(), pk)
        })
        .collect()
}

pub fn build_address_map<'a>(
    public_keys: impl Iterator<Item = (&'a String, &'a PublicKey)>,
) -> HashMap<String, Address> {
    public_keys
        .map(|(name, pk)| (name.clone(), Address::from_public_key(pk)))
        .collect()
}

pub fn value_from_model(value: &Value) -> NilOrVal<ValueId> {
    match value {
        Value::Nil => NilOrVal::Nil,
        Value::Val(v) => match v.as_str() {
            "v1" => NilOrVal::Val(1.into()),
            "v2" => NilOrVal::Val(2.into()),
            "v3" => NilOrVal::Val(3.into()),
            _ => unimplemented!("unknown value {value:?}"),
        },
    }
}

pub fn check_votes(
    expected: &malachite_itf::votekeeper::VoteCount,
    actual: &malachite_vote::count::VoteCount<TestContext>,
    address_map: &HashMap<String, Address>,
) {
    // expected has `total_weight` which is not present in actual

    let expected_values_weights = &expected.values_weights;
    let actual_values_weights = &actual.values_weights;

    // should check length too

    for value in expected_values_weights.keys() {
        assert_eq!(
            actual_values_weights.get(&value_from_model(value)),
            *expected_values_weights.get(value).unwrap() as u64,
            "weight for value {value:?}"
        );
    }

    let expected_votes_addresses = &expected.votes_addresses;
    let actual_votes_addresses = &actual.validator_addresses;

    assert_eq!(
        actual_votes_addresses.len(),
        expected_votes_addresses.len(),
        "number of voted addresses"
    );

    for address in expected_votes_addresses {
        assert!(
            actual_votes_addresses.contains(address_map.get(address).unwrap()),
            "address {address:?} not voted"
        );
    }
}
