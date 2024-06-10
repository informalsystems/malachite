use color_eyre::eyre::Result;
use tracing::debug;

use malachite_node::config::Config;
use malachite_test::{PrivateKey, ValidatorSet};

use crate::args::{Args, Commands};
use crate::cmd::keys::KeysCmd;
use crate::cmd::testnet::TestnetCmd;
use crate::logging::LogLevel;

mod args;
mod cmd;
mod logging;
mod metrics;
mod priv_key;

#[tokio::main(flavor = "current_thread")]
pub async fn main() -> Result<()> {
    let args = Args::new();

    logging::init(LogLevel::Debug, &args.debug);

    debug!("Command-line parameters: {args:?}");

    match &args.command {
        Commands::Start => start(&args).await,
        Commands::Init => init(&args),
        Commands::Keys(cmd) => keys(&args, cmd),
        Commands::Testnet(cmd) => testnet(&args, cmd),
    }
}

async fn start(args: &Args) -> Result<()> {
    let cfg: Config = args.load_config()?;
    let sk: PrivateKey = args.load_private_key()?;
    let vs: ValidatorSet = args.load_genesis()?;

    cmd::start::run(sk, cfg, vs).await
}

fn init(args: &Args) -> Result<()> {
    cmd::init::run(
        &args.get_config_file_path()?,
        &args.get_genesis_file_path()?,
        &args.get_priv_validator_key_file_path()?,
    )
}

fn keys(args: &Args, cmd: &KeysCmd) -> Result<()> {
    cmd.run(args)
}

fn testnet(args: &Args, cmd: &TestnetCmd) -> Result<()> {
    cmd.run(&args.get_home_dir()?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use clap::Parser;
    use color_eyre::eyre;

    use super::*;

    #[test]
    fn running_init_creates_config_files() -> eyre::Result<()> {
        let tmp = tempfile::tempdir()?;
        let config_dir = tmp.path().join("config");

        let args = Args::parse_from(["test", "--home", tmp.path().to_str().unwrap(), "init"]);

        init(&args)?;

        let files = fs::read_dir(&config_dir)?.flatten().collect::<Vec<_>>();

        dbg!(&files);

        assert!(has_file(&files, &config_dir.join("config.toml")));
        assert!(has_file(&files, &config_dir.join("genesis.json")));
        assert!(has_file(
            &files,
            &config_dir.join("priv_validator_key.json")
        ));

        Ok(())
    }

    #[test]
    fn running_testnet_creates_all_configs() -> eyre::Result<()> {
        let tmp = tempfile::tempdir()?;

        let args = Args::parse_from([
            "test",
            "--home",
            tmp.path().to_str().unwrap(),
            "testnet",
            "--nodes",
            "3",
        ]);

        let Commands::Testnet(ref testnet_args) = args.command else {
            panic!("not testnet command");
        };

        testnet(&args, testnet_args)?;

        let files = fs::read_dir(&tmp)?.flatten().collect::<Vec<_>>();

        assert_eq!(files.len(), 3);

        assert!(has_file(&files, &tmp.path().join("0")));
        assert!(has_file(&files, &tmp.path().join("1")));
        assert!(has_file(&files, &tmp.path().join("2")));

        for node in 0..3 {
            let node_dir = tmp.path().join(node.to_string()).join("config");
            let files = fs::read_dir(&node_dir)?.flatten().collect::<Vec<_>>();

            assert!(has_file(&files, &node_dir.join("config.toml")));
            assert!(has_file(&files, &node_dir.join("genesis.json")));
            assert!(has_file(&files, &node_dir.join("priv_validator_key.json")));
        }

        Ok(())
    }

    fn has_file(files: &[fs::DirEntry], path: &PathBuf) -> bool {
        files.iter().any(|f| &f.path() == path)
    }
}
