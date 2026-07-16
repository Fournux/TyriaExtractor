mod atex;
mod capture;
mod cli;
mod dat;
mod file_ref;
mod gw_dat_decompress;
mod icon_payload;
mod images;
mod io_util;
mod items;
mod models;
mod npcs;
mod pe;
mod quests;
mod skills;
mod text;
mod text_records;
mod vendors;
mod workflows;

#[cfg(test)]
mod tests;

use clap::Parser;

use cli::{Cli, Command, ExtractCommand};

use crate::{
    dat::{dump_entries, read_dat_entry},
    io_util::{write_bytes, write_json},
};

pub fn run() -> anyhow::Result<()> {
    execute(Cli::parse())
}

pub(crate) fn execute(cli: Cli) -> anyhow::Result<()> {
    let Cli {
        out_dir: extraction_root,
        command,
    } = cli;
    match command {
        Command::Extract { target } => match target {
            ExtractCommand::Skills { snapshot } => {
                workflows::extract_skills(&snapshot, &extraction_root)?
            }
            ExtractCommand::Images { snapshot } => {
                workflows::extract_images(&snapshot, &extraction_root)?
            }
            ExtractCommand::Items {
                snapshot,
                packet_log,
                skip_icons,
                use_client_strings,
            } => workflows::extract_items(
                &snapshot,
                &packet_log,
                skip_icons,
                use_client_strings,
                &extraction_root,
            )?,
            ExtractCommand::Quests {
                snapshot,
                packet_log,
                item_log,
            } => workflows::extract_quests(&snapshot, &packet_log, &item_log, &extraction_root)?,
            ExtractCommand::Npcs {
                snapshot,
                packet_log,
            } => workflows::extract_npcs(&snapshot, &packet_log, &extraction_root)?,
            ExtractCommand::Vendors {
                snapshot,
                packet_log,
            } => workflows::extract_vendors(&snapshot, &packet_log, &extraction_root)?,
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
