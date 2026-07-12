use anyhow::Context;
use serde::Serialize;
use std::{fs, path::Path};

fn read_array<const N: usize>(bytes: &[u8], offset: usize, kind: &str) -> anyhow::Result<[u8; N]> {
    bytes
        .get(offset..)
        .and_then(|tail| tail.get(..N))
        .with_context(|| format!("{kind} at byte offset {offset} is out of bounds"))?
        .try_into()
        .with_context(|| format!("{kind} at byte offset {offset} has invalid length"))
}

pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(read_array(bytes, offset, "u16")?))
}

pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, offset, "u32")?))
}

pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, offset, "u64")?))
}

pub(crate) fn hex_u32(value: u32) -> String {
    format!("0x{value:08x}")
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes)?;
    println!("wrote {}", path.display());
    Ok(())
}

pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, bytes)?;
    println!("wrote {}", path.display());
    Ok(())
}
