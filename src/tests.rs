use crate::{
    cli::{Cli, Command, ExtractCommand},
    dat::{
        dump_entries, hash_lookup_by_file_id, hash_lookup_by_mft_index,
        lookup_mft_entry_for_file_id, lookup_mft_stream_entry_from_base, read_dat_entry_from_file,
        read_dat_table, read_hash_lookup,
    },
    file_ref::{decode_file_reference, encode_file_reference},
    text::{detect_entry_kind, utf16le_strings},
};
use clap::Parser;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
};

static TEMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let id = TEMP_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("tyria-extractor-test-{}-{id}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_minimal_gw_dat(path: &Path) -> anyhow::Result<()> {
    let mut bytes = vec![0_u8; 1546];

    bytes[0..4].copy_from_slice(&[0x33, 0x41, 0x4e, 0x1a]);
    bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&512_u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&0x4ccbad70_u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&512_u64.to_le_bytes());
    bytes[24..28].copy_from_slice(&120_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&0_u32.to_le_bytes());

    bytes[512..516].copy_from_slice(&[b'M', b'f', b't', 0x1a]);
    bytes[524..528].copy_from_slice(&5_u32.to_le_bytes());

    write_mft_entry(&mut bytes, 536, 0, 32, 0, 3, 0, 0, 0);
    write_mft_entry(&mut bytes, 560, 1024, 16, 0, 3, 0, 0, 0x1234);
    write_mft_entry(&mut bytes, 584, 512, 120, 0, 3, 0, 0, 0x5678);
    write_mft_entry(&mut bytes, 608, 1536, 10, 8, 3, 2, 20, 0xdeadbeef);

    bytes[1024..1028].copy_from_slice(&123_u32.to_le_bytes());
    bytes[1028..1032].copy_from_slice(&16_u32.to_le_bytes());
    bytes[1032..1036].copy_from_slice(&456_u32.to_le_bytes());
    bytes[1036..1040].copy_from_slice(&18_u32.to_le_bytes());

    fs::write(path, bytes)?;
    Ok(())
}

fn write_hash_lookup_gw_dat(path: &Path) -> anyhow::Result<()> {
    let mut bytes = vec![0_u8; 1280];

    bytes[0..4].copy_from_slice(&[0x33, 0x41, 0x4e, 0x1a]);
    bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&512_u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&0x4ccbad70_u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&512_u64.to_le_bytes());
    bytes[24..28].copy_from_slice(&144_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&0_u32.to_le_bytes());

    bytes[512..516].copy_from_slice(&[b'M', b'f', b't', 0x1a]);
    bytes[524..528].copy_from_slice(&6_u32.to_le_bytes());

    let dds_payload = b"DDS texture".to_vec();
    let text_payload = utf16le_fixture(&["Skill Name", "Skill Description"]);
    let ascii_payload = b"Plain ASCII entry".to_vec();
    let sound_payload = vec![0xff, 0x11, 0x22, 0x33];

    write_mft_entry(
        &mut bytes,
        536,
        1056,
        dds_payload.len() as u32,
        0,
        3,
        1,
        10,
        0xaaaa0001,
    );
    write_mft_entry(&mut bytes, 560, 1024, 24, 0, 0, 0, 0, 0);
    write_mft_entry(
        &mut bytes,
        584,
        1088,
        text_payload.len() as u32,
        0,
        3,
        4,
        30,
        0xbbbb0003,
    );
    write_mft_entry(
        &mut bytes,
        608,
        1152,
        ascii_payload.len() as u32,
        0,
        3,
        5,
        40,
        0xcccc0004,
    );
    write_mft_entry(
        &mut bytes,
        632,
        1200,
        sound_payload.len() as u32,
        0,
        3,
        6,
        50,
        0xdddd0005,
    );

    for (row, (file_number, mft_index)) in [
        (0x1000_u32, 3_u32),
        (0x1001_u32, 3_u32),
        (0x2000_u32, 4_u32),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 1024 + row * 8;
        bytes[offset..offset + 4].copy_from_slice(&file_number.to_le_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&mft_index.to_le_bytes());
    }

    bytes[1056..1056 + dds_payload.len()].copy_from_slice(&dds_payload);
    bytes[1088..1088 + text_payload.len()].copy_from_slice(&text_payload);
    bytes[1152..1152 + ascii_payload.len()].copy_from_slice(&ascii_payload);
    bytes[1200..1200 + sound_payload.len()].copy_from_slice(&sound_payload);

    fs::write(path, bytes)?;
    Ok(())
}

fn write_stream_chain_gw_dat(path: &Path) -> anyhow::Result<()> {
    let mut bytes = vec![0_u8; 1152];

    bytes[0..4].copy_from_slice(&[0x33, 0x41, 0x4e, 0x1a]);
    bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&512_u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&0x4ccbad70_u32.to_le_bytes());
    bytes[16..24].copy_from_slice(&512_u64.to_le_bytes());
    bytes[24..28].copy_from_slice(&120_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&0_u32.to_le_bytes());

    bytes[512..516].copy_from_slice(&[b'M', b'f', b't', 0x1a]);
    bytes[524..528].copy_from_slice(&5_u32.to_le_bytes());

    let hash_payload = [(0x3000_u32, 3_u32)];
    let fallback_payload = b"fallback";
    let icon_payload = b"icon!!";

    write_mft_entry(&mut bytes, 536, 0, 0, 0, 0, 0, 0, 0);
    write_mft_entry(&mut bytes, 560, 1024, 8, 0, 3, 0, 0, 0x1111);
    write_mft_entry(
        &mut bytes,
        584,
        1056,
        fallback_payload.len() as u32,
        0,
        3,
        1,
        4,
        0x2222,
    );
    write_mft_entry(
        &mut bytes,
        608,
        1088,
        icon_payload.len() as u32,
        0,
        1,
        0,
        0,
        0x3333,
    );

    bytes[1024..1028].copy_from_slice(&hash_payload[0].0.to_le_bytes());
    bytes[1028..1032].copy_from_slice(&hash_payload[0].1.to_le_bytes());
    bytes[1056..1056 + fallback_payload.len()].copy_from_slice(fallback_payload);
    bytes[1088..1088 + icon_payload.len()].copy_from_slice(icon_payload);

    fs::write(path, bytes)?;
    Ok(())
}

#[expect(clippy::too_many_arguments, reason = "compact DAT fixture writer")]
fn write_mft_entry(
    bytes: &mut [u8],
    offset: usize,
    file_offset: u64,
    size: u32,
    compression: u16,
    content: u8,
    content_type: u8,
    id: u32,
    crc: u32,
) {
    bytes[offset..offset + 8].copy_from_slice(&file_offset.to_le_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&size.to_le_bytes());
    bytes[offset + 12..offset + 14].copy_from_slice(&compression.to_le_bytes());
    bytes[offset + 14] = content;
    bytes[offset + 15] = content_type;
    bytes[offset + 16..offset + 20].copy_from_slice(&id.to_le_bytes());
    bytes[offset + 20..offset + 24].copy_from_slice(&crc.to_le_bytes());
}

fn utf16le_fixture(strings: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for text in strings {
        for code in text.encode_utf16() {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }
    bytes
}

#[path = "tests/cli.rs"]
mod cli;
#[path = "tests/dat.rs"]
mod dat;
#[path = "tests/textures.rs"]
mod textures;
