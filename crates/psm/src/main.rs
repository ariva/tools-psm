mod analysis;
mod cli;
mod collect;
mod commands;
mod config;
mod model;
mod output;
mod store;

/// Exit codes: 0 success, 1 reserved for thresholds, 2 usage error or runtime failure.
fn main() {
    let cli = cli::parse();
    if let Err(e) = commands::run(cli) {
        eprintln!("psm: {e:#}");
        std::process::exit(2);
    }
}
