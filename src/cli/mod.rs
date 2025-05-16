use args::CliArgs;
use env_logger::{Builder, Target};

mod args;
mod check;
mod pt;
mod socks;

pub async fn run() -> anyhow::Result<()> {
    let args = args::parse_cli_args();
    setup_logging(&args);

    log::info!("Parsed CLI args and initialized logger!");

    let result = match args.command {
        args::Command::Socks(args) => socks::run(args).await,
        args::Command::Pt(args) => pt::run(args).await,
        args::Command::Check(args) => check::run(args).await,
    };

    if let Err(e) = &result {
        log::error!("Error occurred: {}", e);
        log::error!("Caused by: {}", e.root_cause());
        log::info!("Exiting cleanly with error :/");
    } else {
        log::info!("Exiting cleanly with success :)");
    }

    result
}

fn setup_logging(args: &CliArgs) {
    // Set up logger.
    let mut logger = Builder::new();

    if let Some(filters) = &args.log_filter {
        // Configure with the RUST_LOG directives string from the cli arg.
        logger.parse_filters(filters.as_str())
    } else {
        // Configure just the log level.
        logger.filter_level(args.log_level.into())
    };

    // Log to stderr because PT mode uses stdout for comms with parent.
    logger.target(Target::Stderr).init();
}
