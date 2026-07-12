mod atex;
mod cli;
mod dat;
mod file_ref;
mod gw_dat_decompress;
mod icon_payload;
mod io_util;
mod items;
mod models;
mod pe;
mod quests;
mod skills;
mod text;
mod text_records;
mod workflows;

#[cfg(test)]
mod tests;

use clap::Parser;

use cli::{Cli, Command, ExtractCommand, ExtractTarget};

use crate::{
    dat::{dump_entries, read_dat_entry},
    io_util::{write_bytes, write_json},
};

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(target) = cli.extract {
        match target {
            ExtractTarget::Skills => workflows::extract_skills(&cli.snapshot)?,
            ExtractTarget::Items => workflows::extract_items(&cli.snapshot, None, false, false)?,
        }
        return Ok(());
    }

    let Some(command) = cli.command else {
        anyhow::bail!(
            "missing command; use --extract <skills|items> --snapshot <path> or a subcommand"
        );
    };

    match command {
        Command::Extract { target } => match target {
            ExtractCommand::Skills { snapshot } => workflows::extract_skills(&snapshot)?,
            ExtractCommand::Items {
                snapshot,
                packet_log,
                skip_icons,
                use_client_strings,
            } => workflows::extract_items(
                &snapshot,
                packet_log.as_deref(),
                skip_icons,
                use_client_strings,
            )?,
            ExtractCommand::Quests {
                snapshot,
                packet_log,
                item_log,
            } => workflows::extract_quests(&snapshot, &packet_log, item_log.as_deref())?,
        },
        Command::DumpEntries {
            gw_dat,
            out_dir,
            limit,
        } => {
            let manifest = dump_entries(&gw_dat, &out_dir, limit)?;
            write_json(&manifest.cache_root.join("manifest.json"), &manifest)?
        }
        Command::ExtractEntry { gw_dat, index, out } => {
            write_bytes(&out, &read_dat_entry(&gw_dat, index)?)?
        }
    }

    Ok(())
}
