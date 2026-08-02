use std::path::PathBuf;

use calendar::{config::Config, store::CalendarStore, tui};
use clap::Parser;
use eyre::Result;

#[derive(Debug, Parser)]
#[command(about = "A keyboard-first terminal calendar", version)]
struct Cli {
    /// Override the directory containing the vdir-style .ics files.
    #[arg(short = 'd', long)]
    calendar_dir: Option<PathBuf>,

    /// Override the YAML configuration file.
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(Config::default_path);
    let mut config = Config::load_or_create(&config_path)?;
    if let Some(calendar_dir) = cli.calendar_dir {
        config.calendar_dir = calendar_dir;
    }
    config.calendar_dir = Config::expand_home(&config.calendar_dir);

    let store = CalendarStore::new(config.calendar_dir.clone(), config.default_calendar.clone());
    tui::run(config, store)
}
