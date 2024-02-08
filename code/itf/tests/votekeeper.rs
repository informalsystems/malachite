#[path = "votekeeper/runner.rs"]
pub mod runner;
#[path = "votekeeper/utils.rs"]
pub mod utils;

use glob::glob;
use rand::rngs::StdRng;
use rand::SeedableRng;

use malachite_itf::utils::{generate_traces, get_seed, TraceOptions};
use malachite_itf::votekeeper::State;
use malachite_test::{Address, PrivateKey};

use runner::VoteKeeperRunner;
use utils::ADDRESSES;

#[test]
fn test_itf() {
    let temp_dir =
        tempfile::TempDir::with_prefix("malachite-votekeeper-").expect("Failed to create temp dir");
    let temp_path = temp_dir.path().to_owned();

    if std::env::var("KEEP_TEMP").is_ok() {
        std::mem::forget(temp_dir);
    }

    let seed = get_seed();

    generate_traces(
        "tests/votekeeper/votekeeperTest.qnt",
        &temp_path.to_string_lossy(),
        TraceOptions {
            seed,
            ..Default::default()
        },
    );

    for json_fixture in glob(&format!("{}/*.itf.json", temp_path.display()))
        .expect("Failed to read glob pattern")
        .flatten()
    {
        println!("🚀 Running trace {json_fixture:?}");

        let json = std::fs::read_to_string(&json_fixture).unwrap();
        let trace = itf::trace_from_str::<State>(&json).unwrap();

        let mut rng = StdRng::seed_from_u64(seed);

        // build mapping from model addresses to real addresses
        let vote_keeper_runner = VoteKeeperRunner {
            address_map: ADDRESSES
                .iter()
                .map(|&name| {
                    let pk = PrivateKey::generate(&mut rng).public_key();
                    (name.into(), Address::from_public_key(&pk))
                })
                .collect(),
        };

        trace.run_on(vote_keeper_runner).unwrap();
    }
}
