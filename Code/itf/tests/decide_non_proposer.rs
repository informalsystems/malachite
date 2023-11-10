use malachite_itf::consensus::State;

const FIXTURES: &[&str] = &["DecideNonProposerTest0.itf.json"];

#[test]
fn parse_fixtures() {
    for fixture in FIXTURES {
        println!("Parsing '{fixture}'");

        let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), fixture);

        let json = std::fs::read_to_string(&path).unwrap();
        let state = itf::trace_from_str::<State>(&json).unwrap();

        dbg!(state);
    }
}
