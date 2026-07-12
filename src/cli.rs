use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const DEFAULT_GW_SNAPSHOT: &str =
    "/home/fournux/.local/share/Steam/steamapps/common/Guild Wars/Gw.snapshot";

#[derive(Debug, Parser)]
#[command(name = "gwdb-extractor")]
#[command(about = "Guild Wars client extraction helpers")]
pub(crate) struct Cli {
    #[arg(long, value_enum)]
    pub(crate) extract: Option<ExtractTarget>,
    #[arg(long, default_value = DEFAULT_GW_SNAPSHOT)]
    pub(crate) snapshot: PathBuf,
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ExtractTarget {
    Skills,
    Items,
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
        #[arg(long, default_value = DEFAULT_GW_SNAPSHOT)]
        snapshot: PathBuf,
    },
    Items {
        #[arg(long, default_value = DEFAULT_GW_SNAPSHOT)]
        snapshot: PathBuf,
        #[arg(long)]
        packet_log: Option<PathBuf>,
        #[arg(long)]
        skip_icons: bool,
        #[arg(long)]
        use_client_strings: bool,
    },
    Quests {
        #[arg(long, default_value = DEFAULT_GW_SNAPSHOT)]
        snapshot: PathBuf,
        #[arg(long)]
        packet_log: PathBuf,
        /// Compact ItemGeneral capture used to resolve reward item model IDs.
        #[arg(long)]
        item_log: Option<PathBuf>,
    },
}
