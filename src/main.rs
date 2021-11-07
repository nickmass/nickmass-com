use tokio::runtime::Runtime;
use tracing::metadata::LevelFilter;
use tracing_subscriber::prelude::*;

mod server;
use server::Config;

fn main() {
    let config = Config::load();

    let log_filter = tracing_subscriber::filter::Targets::new().with_default(LevelFilter::OFF);
    let log_filter = match (config.verbosity, config.silent) {
        (1, true) => log_filter
            .with_target("nickmass_com", LevelFilter::ERROR)
            .with_target("tower_http", LevelFilter::ERROR),
        (2, true) => log_filter
            .with_target("nickmass_com", LevelFilter::WARN)
            .with_target("tower_http", LevelFilter::WARN),
        (_, true) => log_filter.with_default(LevelFilter::OFF),
        (0, _) | (1, _) => log_filter
            .with_target("nickmass_com", LevelFilter::INFO)
            .with_target("tower_http", LevelFilter::INFO)
            .with_default(LevelFilter::ERROR),
        (2, _) => log_filter
            .with_target("nickmass_com", LevelFilter::DEBUG)
            .with_target("tower_http", LevelFilter::DEBUG)
            .with_default(LevelFilter::WARN),
        (3, _) => log_filter
            .with_target("nickmass_com", LevelFilter::TRACE)
            .with_target("tower_http", LevelFilter::TRACE)
            .with_default(LevelFilter::INFO),
        _ => log_filter.with_default(LevelFilter::TRACE),
    };

    tracing_subscriber::registry()
        .with(log_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("config loaded");

    let rt = Runtime::new().unwrap();
    rt.block_on(server::run(config));
}
