//! Example application using channels

mod node;
mod state;

use eyre::eyre;
use malachite_test_cli::args::{Args, Commands};
use malachite_test_cli::config::load_config;
use malachite_test_cli::{logging, runtime};
use node::App;
use tracing::{error, info, trace};

/// main function parses the command-line arguments, loads configuration, initializes logging
/// and runs the application object.
fn main() -> color_eyre::Result<()> {
    color_eyre::install().expect("Failed to install global error handler");

    // Load command-line arguments and possible configuration file.
    let args = Args::new();

    // Load configuration file if it exists. Some commands do not require a configuration file.
    let opt_config_file_path = args
        .get_config_file_path()
        .map_err(|error| eyre!("Failed to get configuration file path: {:?}", error));
    let opt_config = opt_config_file_path.and_then(|path| {
        load_config(&path, None)
            .map_err(|error| eyre!("Failed to load configuration file: {:?}", error))
    });

    // Override logging configuration (if exists) with optional command-line parameters.
    let mut logging = opt_config.as_ref().map(|c| c.logging).unwrap_or_default();
    if let Some(log_level) = args.log_level {
        logging.log_level = log_level;
    }
    if let Some(log_format) = args.log_format {
        logging.log_format = log_format;
    }

    // This is a drop guard responsible for flushing any remaining logs when the program terminates.
    // It must be assigned to a binding that is not _, as _ will result in the guard being dropped immediately.
    let _guard = logging::init(logging.log_level, logging.log_format);

    trace!("Command-line parameters: {args:?}");

    // Create the application object.
    let node = &App {
        home_dir: args.get_home_dir()?,
        config: Default::default(), // placeholder, because `init` and `testnet` has no valid configuration file.
        genesis_file: args.get_genesis_file_path()?,
        private_key_file: args.get_priv_validator_key_file_path()?,
        start_height: Default::default(), // placeholder, because start_height is only valid in StartCmd.
    };

    // Parse the input command.
    match &args.command {
        Commands::Start(cmd) => {
            // Build configuration from valid configuration file and command-line parameters.
            let mut config = opt_config
                .map_err(|error| error!(%error, "Failed to load configuration."))
                .unwrap();
            config.logging = logging;
            let runtime = config.runtime;
            let metrics = if config.metrics.enabled {
                Some(config.metrics.clone())
            } else {
                None
            };

            info!(
                file = %args.get_config_file_path().unwrap_or_default().display(),
                "Loaded configuration",
            );
            trace!(?config, "Configuration");

            // Redefine the node with the valid configuration.
            let node = &App {
                home_dir: args.get_home_dir()?,
                config,
                genesis_file: args.get_genesis_file_path()?,
                private_key_file: args.get_priv_validator_key_file_path()?,
                start_height: cmd.start_height,
            };

            // Define the runtime. If you are not interested in a custom runtime configuration,
            // you can use the #[async_trait] attribute on the main function.
            let rt = runtime::build_runtime(runtime)?;
            rt.block_on(cmd.run(node, metrics))
                .map_err(|error| eyre!("Failed to run start command {:?}", error))
        }
        Commands::Init(cmd) => cmd
            .run(
                node,
                &args.get_config_file_path()?,
                &args.get_genesis_file_path()?,
                &args.get_priv_validator_key_file_path()?,
                logging,
            )
            .map_err(|error| eyre!("Failed to run init command {:?}", error)),
        Commands::Testnet(cmd) => cmd
            .run(node, &args.get_home_dir()?, logging)
            .map_err(|error| eyre!("Failed to run testnet command {:?}", error)),
        Commands::DistributedTestnet(cmd) => cmd
            .run(node, &args.get_home_dir()?, logging)
            .map_err(|error| eyre!("Failed to run distributed testnet command {:?}", error)),
    }
}