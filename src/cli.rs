use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "gwdb-extractor")]
#[command(about = "Guild Wars client extraction helpers")]
pub(crate) struct Cli {
    /// Root directory receiving skills/, items/, and quests/.
    #[arg(long, global = true, default_value = ".")]
    pub(crate) out_dir: PathBuf,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Extract {
        #[command(subcommand)]
        target: ExtractCommand,
    },
    DumpEntries {
        #[arg(long)]
        gw_dat: PathBuf,
        #[arg(long, default_value = "data/cache/gwdat")]
        out_dir: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
    },
    ExtractEntry {
        #[arg(long)]
        gw_dat: PathBuf,
        #[arg(long)]
        index: u32,
        #[arg(long, default_value = "data/raw/dat-entry.local.bin")]
        out: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ExtractCommand {
    Skills {
        #[arg(long)]
        snapshot: PathBuf,
    },
    Items {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        packet_log: Option<PathBuf>,
        #[arg(long, requires = "packet_log")]
        skip_icons: bool,
        #[arg(long, requires = "packet_log")]
        use_client_strings: bool,
        /// Accept legacy captures without verified health metadata.
        #[arg(long, requires = "packet_log")]
        allow_unverified_capture: bool,
    },
    Quests {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        packet_log: PathBuf,
        /// Compact ItemGeneral capture used to resolve reward item model IDs.
        #[arg(long)]
        item_log: Option<PathBuf>,
        /// Accept legacy captures without verified health/schema metadata.
        #[arg(long)]
        allow_unverified_capture: bool,
    },
}
