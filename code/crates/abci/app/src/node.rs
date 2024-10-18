use std::path::Path;

use malachite_abci_host::context::AbciContext;
use malachite_abci_host::types::{PrivateKey, PublicKey, Validator, ValidatorSet};
use malachite_common::VotingPower;
use malachite_node::Node;
use rand::{CryptoRng, RngCore};

pub struct AbciNode;

impl Node for AbciNode {
    type Context = AbciContext;
    type PrivateKeyFile = PrivateKey;
    type Genesis = ValidatorSet;

    fn generate_private_key<R>(&self, rng: R) -> PrivateKey
    where
        R: RngCore + CryptoRng,
    {
        PrivateKey::generate(rng)
    }

    fn load_private_key_file(
        &self,
        path: impl AsRef<Path>,
    ) -> std::io::Result<Self::PrivateKeyFile> {
        let private_key = std::fs::read_to_string(path)?;
        serde_json::from_str(&private_key).map_err(|e| e.into())
    }

    fn load_private_key(&self, file: Self::PrivateKeyFile) -> PrivateKey {
        file
    }

    fn make_private_key_file(&self, private_key: PrivateKey) -> Self::PrivateKeyFile {
        private_key
    }

    fn load_genesis(&self, path: impl AsRef<Path>) -> std::io::Result<Self::Genesis> {
        let genesis = std::fs::read_to_string(path)?;
        serde_json::from_str(&genesis).map_err(|e| e.into())
    }

    fn make_genesis(&self, validators: Vec<(PublicKey, VotingPower)>) -> Self::Genesis {
        let validators = validators
            .into_iter()
            .map(|(pk, vp)| Validator::new(pk, vp));

        ValidatorSet::new(validators)
    }
}
