#![recursion_limit = "128"]

mod server;
use server::Config;

fn main() {
    let config = Config::load();
    let mut builder = env_logger::Builder::new();
    match (config.verbosity, config.silent) {
        (1, true) => builder.filter(Some("nickmass_com"), log::LevelFilter::Error),
        (2, true) => builder.filter(Some("nickmass_com"), log::LevelFilter::Warn),
        (_, true) => builder.filter_level(log::LevelFilter::Off),
        (0, _) | (1, _) => builder.filter(Some("nickmass_com"), log::LevelFilter::Info),
        (2, _) => builder.filter(Some("nickmass_com"), log::LevelFilter::Debug),
        (3, _) => builder.filter(Some("nickmass_com"), log::LevelFilter::Trace),
        _ => builder.filter(None, log::LevelFilter::Trace),
    };
    builder.write_style(env_logger::WriteStyle::Auto).init();
    server::run(config);
}
