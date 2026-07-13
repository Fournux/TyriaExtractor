use anyhow::{Context, bail};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use crate::{
    gw_dat_decompress,
    io_util::{hex_u32, read_u16, read_u32, read_u64},
    models::*,
    text::{detect_entry_kind, utf16le_strings},
};

const CLIENT_PE_FILE_ID: u32 = 4102;

pub(crate) struct DatArchive {
    path: PathBuf,
    file: File,
    file_size: u64,
    entries: Vec<MftEntry>,
    hash_lookup: Vec<HashLookupEntry>,
    hash_to_mft: BTreeMap<u32, u32>,
    hashes_by_mft: BTreeMap<u32, Vec<u32>>,
}

impl DatArchive {
    pub(crate) fn open(path: &Path) -> anyhow::Result<Self> {
        let metadata = fs::metadata(path)
            .with_context(|| format!("reading metadata for {}", path.display()))?;
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let (_, _, entries) = read_dat_table(&mut file, path, metadata.len())?;
        let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;
        let hash_to_mft = hash_lookup_by_file_id(&hash_lookup);
        let hashes_by_mft = hash_lookup_by_mft_index(&hash_lookup);
        Ok(Self {
            path: path.to_path_buf(),
            file,
            file_size: metadata.len(),
            entries,
            hash_lookup,
            hash_to_mft,
            hashes_by_mft,
        })
    }

    pub(crate) fn entries(&self) -> &[MftEntry] {
        &self.entries
    }

    pub(crate) fn hash_lookup(&self) -> &[HashLookupEntry] {
        &self.hash_lookup
    }

    pub(crate) fn hashes_by_mft(&self) -> &BTreeMap<u32, Vec<u32>> {
        &self.hashes_by_mft
    }

    pub(crate) fn entry(&self, index: u32) -> Option<MftEntry> {
        mft_entry_by_index(&self.entries, index).copied()
    }

    pub(crate) fn mft_index_for_file_id(&self, file_id: u32) -> Option<u32> {
        lookup_mft_index_for_file_id(file_id, &self.hash_to_mft)
    }

    pub(crate) fn entry_for_file_id(&self, file_id: u32) -> Option<MftEntry> {
        lookup_mft_entry_for_file_id(file_id, &self.hash_to_mft, &self.entries).copied()
    }

    pub(crate) fn read_entry(&mut self, index: u32) -> anyhow::Result<Vec<u8>> {
        let entry = self
            .entry(index)
            .with_context(|| format!("MFT entry {index} not found"))?;
        read_dat_entry_from_file(&mut self.file, self.file_size, &entry)
    }

    pub(crate) fn read_file_id(&mut self, file_id: u32) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(entry) = self.entry_for_file_id(file_id) else {
            return Ok(None);
        };
        read_dat_entry_from_file(&mut self.file, self.file_size, &entry).map(Some)
    }

    pub(crate) fn client_pe_data(&mut self) -> anyhow::Result<Vec<u8>> {
        read_client_pe_data(
            &self.path,
            &mut self.file,
            self.file_size,
            &self.hash_to_mft,
            &self.entries,
        )
    }
}

pub(crate) fn dump_entries(
    path: &Path,
    out_dir: &Path,
    limit: Option<usize>,
) -> anyhow::Result<DatCacheManifest> {
    let metadata =
        fs::metadata(path).with_context(|| format!("reading metadata for {}", path.display()))?;
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let (header, mft, entries) = read_dat_table(&mut file, path, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;
    let hash_lookup_by_mft = hash_lookup_by_mft_index(&hash_lookup);
    let cache_key = format!("{}-{}-{}", metadata.len(), header.crc_hex, mft.entry_count);
    let cache_root = out_dir.join(&cache_key);
    let entries_root = cache_root.join("entries");

    let mut cache_entries = Vec::new();
    let mut failures = Vec::new();
    let mut active_entries = 0;
    let mut dumped_entries = 0;
    let mut decompressed_entries = 0;
    let mut failed_entries = 0;

    for entry in entries
        .iter()
        .filter(|entry| entry.content == 3 && entry.size > 0)
    {
        if limit.is_some_and(|limit| dumped_entries >= limit) {
            break;
        }
        active_entries += 1;

        match read_dat_entry_from_file(&mut file, metadata.len(), entry) {
            Ok(bytes) => {
                if entry.compression == 8 {
                    decompressed_entries += 1;
                }

                let relative_path = cache_entry_relative_path(entry.index);
                let out_path = entries_root.join(&relative_path);
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out_path, &bytes)?;
                dumped_entries += 1;

                cache_entries.push(DatCacheEntry {
                    index: entry.index,
                    offset: entry.offset,
                    compressed_size: entry.size,
                    decompressed_size: bytes.len(),
                    compression: entry.compression,
                    content: entry.content,
                    content_type: entry.content_type,
                    id: entry.id,
                    crc_hex: hex_u32(entry.crc),
                    hashes: hash_lookup_by_mft
                        .get(&entry.index)
                        .cloned()
                        .unwrap_or_default(),
                    magic_hex: hex::encode(&bytes[..bytes.len().min(8)]),
                    kind: detect_entry_kind(&bytes).to_string(),
                    utf16le_string_count: utf16le_strings(&bytes).len(),
                    relative_path,
                });
            }
            Err(error) => {
                failed_entries += 1;
                failures.push(DatCacheFailure {
                    index: entry.index,
                    error: format!("{error:#}"),
                });
            }
        }
    }

    Ok(DatCacheManifest {
        schema_version: 1,
        gw_dat_cache_key: cache_key,
        cache_root,
        header,
        mft,
        active_entries,
        dumped_entries,
        decompressed_entries,
        failed_entries,
        failures,
        entries: cache_entries,
    })
}

pub(crate) fn cache_entry_relative_path(index: u32) -> PathBuf {
    PathBuf::from(format!("{:03}/{:06}.bin", index / 1000, index))
}

pub(crate) fn read_hash_lookup(
    file: &mut File,
    file_size: u64,
    entries: &[MftEntry],
) -> anyhow::Result<Vec<HashLookupEntry>> {
    let Some(hash_entry) = entries.get(1) else {
        return Ok(Vec::new());
    };
    if hash_entry.offset == 0 || hash_entry.size == 0 {
        return Ok(Vec::new());
    }
    let hash_end = hash_entry
        .offset
        .checked_add(u64::from(hash_entry.size))
        .context("hash lookup table range overflow")?;
    if hash_end > file_size {
        bail!("hash lookup table exceeds file size");
    }
    if hash_entry.size % 8 != 0 {
        bail!(
            "hash lookup table size {} is not divisible by 8",
            hash_entry.size
        );
    }

    file.seek(SeekFrom::Start(hash_entry.offset))?;
    let mut buffer = vec![0_u8; hash_entry.size as usize];
    file.read_exact(&mut buffer)
        .context("reading MFT hash lookup table")?;

    buffer
        .chunks_exact(8)
        .map(|chunk| {
            Ok(HashLookupEntry {
                file_number: read_u32(chunk, 0)?,
                mft_index: read_u32(chunk, 4)?,
            })
        })
        .collect()
}

pub(crate) fn hash_lookup_by_mft_index(hash_lookup: &[HashLookupEntry]) -> BTreeMap<u32, Vec<u32>> {
    let mut by_mft = BTreeMap::<u32, Vec<u32>>::new();
    for entry in hash_lookup {
        by_mft
            .entry(entry.mft_index)
            .or_default()
            .push(entry.file_number);
    }
    by_mft
}

pub(crate) fn hash_lookup_by_file_id(hash_lookup: &[HashLookupEntry]) -> BTreeMap<u32, u32> {
    hash_lookup
        .iter()
        .map(|entry| (entry.file_number, entry.mft_index))
        .collect()
}

pub(crate) fn mft_entry_by_index(entries: &[MftEntry], index: u32) -> Option<&MftEntry> {
    if let Some(entry) = index
        .checked_sub(1)
        .and_then(|offset| usize::try_from(offset).ok())
        .and_then(|offset| entries.get(offset))
        && entry.index == index
    {
        return Some(entry);
    }

    entries.iter().find(|entry| entry.index == index)
}

pub(crate) fn lookup_mft_index_for_file_id(
    file_id: u32,
    hash_to_mft: &BTreeMap<u32, u32>,
) -> Option<u32> {
    hash_to_mft
        .get(&file_id)
        .or_else(|| hash_to_mft.get(&(file_id | 0x8000_0000)))
        .or_else(|| hash_to_mft.get(&(file_id & 0x7fff_ffff)))
        .copied()
}

pub(crate) fn lookup_mft_entry_for_file_id<'a>(
    file_id: u32,
    hash_to_mft: &BTreeMap<u32, u32>,
    entries: &'a [MftEntry],
) -> Option<&'a MftEntry> {
    let mft_index = lookup_mft_index_for_file_id(file_id, hash_to_mft)?;
    mft_entry_by_index(entries, mft_index)
}

pub(crate) fn lookup_mft_stream_entry_from_base<'a>(
    base_entry: &'a MftEntry,
    stream_id: u8,
    entries: &'a [MftEntry],
) -> Option<&'a MftEntry> {
    let mut entry = base_entry;
    let mut fallback = None;
    for _ in 0..256 {
        if stream_id != 0 && entry.content == stream_id {
            return Some(entry);
        }
        if entry.content_type == stream_id {
            fallback.get_or_insert(entry);
        }
        if entry.id == 0 {
            return fallback;
        }
        entry = mft_entry_by_index(entries, entry.id)?;
    }
    fallback
}

pub(crate) fn read_client_pe_data(
    gw_dat_path: &Path,
    file: &mut File,
    file_size: u64,
    hash_to_mft: &BTreeMap<u32, u32>,
    entries: &[MftEntry],
) -> anyhow::Result<Vec<u8>> {
    if let Some(pe_mft_index) = lookup_mft_index_for_file_id(CLIENT_PE_FILE_ID, hash_to_mft) {
        let pe_entry =
            mft_entry_by_index(entries, pe_mft_index).context("PE entry not found in MFT")?;
        return read_dat_entry_from_file(file, file_size, pe_entry);
    }

    let parent = gw_dat_path.parent().unwrap_or_else(|| Path::new("."));
    let gw_exe_path = parent.join("Gw.exe");
    if gw_exe_path.exists() {
        return fs::read(&gw_exe_path)
            .with_context(|| format!("reading fallback PE from {}", gw_exe_path.display()));
    }

    let fallback_path = parent.join("Gw.dat");
    if !fallback_path.exists() {
        bail!(
            "PE entry index not found in hash lookup, and no fallback Gw.exe or Gw.dat found in {}",
            parent.display()
        );
    }

    let fallback_metadata = fs::metadata(&fallback_path)
        .with_context(|| format!("reading metadata for {}", fallback_path.display()))?;
    let mut fallback_file = File::open(&fallback_path)
        .with_context(|| format!("opening {}", fallback_path.display()))?;
    let (_, _, fallback_entries) =
        read_dat_table(&mut fallback_file, &fallback_path, fallback_metadata.len())?;
    let fallback_hash_lookup = read_hash_lookup(
        &mut fallback_file,
        fallback_metadata.len(),
        &fallback_entries,
    )?;
    let fallback_hash_to_mft = hash_lookup_by_file_id(&fallback_hash_lookup);
    let fallback_pe_mft_index =
        lookup_mft_index_for_file_id(CLIENT_PE_FILE_ID, &fallback_hash_to_mft)
            .context("PE entry index not found in fallback Gw.dat hash lookup")?;
    let fallback_pe_entry = mft_entry_by_index(&fallback_entries, fallback_pe_mft_index)
        .context("PE entry not found in fallback Gw.dat MFT")?;
    read_dat_entry_from_file(
        &mut fallback_file,
        fallback_metadata.len(),
        fallback_pe_entry,
    )
}

pub(crate) fn parse_mft_entry(index: u32, bytes: &[u8; 24]) -> anyhow::Result<MftEntry> {
    Ok(MftEntry {
        index,
        offset: read_u64(bytes, 0)?,
        size: read_u32(bytes, 8)?,
        compression: read_u16(bytes, 12)?,
        content: bytes[14],
        content_type: bytes[15],
        id: read_u32(bytes, 16)?,
        crc: read_u32(bytes, 20)?,
    })
}

pub(crate) fn read_dat_entry(path: &Path, index: u32) -> anyhow::Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("reading metadata for {}", path.display()))?;
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let (_, _, entries) = read_dat_table(&mut file, path, metadata.len())?;
    let entry = mft_entry_by_index(&entries, index)
        .with_context(|| format!("MFT entry {index} not found"))?;

    read_dat_entry_from_file(&mut file, metadata.len(), entry)
}

pub(crate) fn read_dat_entry_from_file(
    file: &mut File,
    file_size: u64,
    entry: &MftEntry,
) -> anyhow::Result<Vec<u8>> {
    if entry.size == 0 || entry.content == 0 {
        bail!("MFT entry {} has no extractable content", entry.index);
    }
    let entry_end = entry
        .offset
        .checked_add(u64::from(entry.size))
        .with_context(|| format!("MFT entry {} range overflow", entry.index))?;
    if entry_end > file_size {
        bail!("MFT entry {} exceeds Gw.dat file size", entry.index);
    }

    file.seek(SeekFrom::Start(entry.offset))?;
    let mut bytes = vec![0_u8; entry.size as usize];
    file.read_exact(&mut bytes)
        .with_context(|| format!("reading MFT entry {} payload", entry.index))?;

    match entry.compression {
        0 => Ok(bytes),
        8 => gw_dat_decompress::decompress_gw_dat(&bytes)
            .with_context(|| format!("decompressing MFT entry {}", entry.index)),

        other => bail!(
            "unsupported MFT entry {} compression type {other}",
            entry.index
        ),
    }
}

pub(crate) fn read_dat_table(
    file: &mut File,
    path: &Path,
    file_size: u64,
) -> anyhow::Result<(GwDatHeader, MftHeader, Vec<MftEntry>)> {
    file.seek(SeekFrom::Start(0))?;
    let mut root = [0_u8; 32];
    file.read_exact(&mut root)
        .with_context(|| format!("reading Gw.dat root block from {}", path.display()))?;

    if root[..4] != [0x33, 0x41, 0x4e, 0x1a] {
        bail!("{} is not a Guild Wars Gw.dat file", path.display());
    }

    let header = GwDatHeader {
        magic_hex: hex::encode(&root[..4]),
        version: root[3],
        header_size: read_u32(&root, 4)?,
        sector_size: read_u32(&root, 8)?,
        crc_hex: hex_u32(read_u32(&root, 12)?),
        mft_offset: read_u64(&root, 16)?,
        mft_size: read_u32(&root, 24)?,
        flags: read_u32(&root, 28)?,
    };
    validate_dat_header(&header, file_size)?;

    file.seek(SeekFrom::Start(header.mft_offset))?;
    let mut mft_header_bytes = [0_u8; 24];
    file.read_exact(&mut mft_header_bytes)
        .context("reading MFT header")?;

    if mft_header_bytes[..4] != [b'M', b'f', b't', 0x1a] {
        bail!("invalid MFT magic {}", hex::encode(&mft_header_bytes[..4]));
    }

    let mft = MftHeader {
        magic_hex: hex::encode(&mft_header_bytes[..4]),
        unknown_1: read_u32(&mft_header_bytes, 4)?,
        unknown_2: read_u32(&mft_header_bytes, 8)?,
        entry_count: read_u32(&mft_header_bytes, 12)?,
        unknown_4: read_u32(&mft_header_bytes, 16)?,
        unknown_5: read_u32(&mft_header_bytes, 20)?,
    };

    let expected_table_bytes = u64::from(mft.entry_count) * 24;
    if expected_table_bytes > u64::from(header.mft_size) {
        bail!(
            "MFT entry count {} exceeds MFT size {}",
            mft.entry_count,
            header.mft_size
        );
    }

    let mut entries = Vec::with_capacity(mft.entry_count.saturating_sub(1) as usize);
    for index in 1..mft.entry_count {
        let mut bytes = [0_u8; 24];
        file.read_exact(&mut bytes)
            .with_context(|| format!("reading MFT entry {index}"))?;
        entries.push(parse_mft_entry(index, &bytes)?);
    }

    Ok((header, mft, entries))
}

pub(crate) fn validate_dat_header(header: &GwDatHeader, file_size: u64) -> anyhow::Result<()> {
    if header.header_size != 32 {
        bail!("unexpected Gw.dat header size {}", header.header_size);
    }
    if header.sector_size != 512 {
        bail!("unexpected Gw.dat sector size {}", header.sector_size);
    }
    let mft_end = header
        .mft_offset
        .checked_add(u64::from(header.mft_size))
        .context("MFT range overflow")?;
    if mft_end > file_size {
        bail!(
            "MFT range {}..{} exceeds file size {}",
            header.mft_offset,
            mft_end,
            file_size
        );
    }
    if header.mft_size < 24 || !header.mft_size.is_multiple_of(24) {
        bail!("invalid MFT size {}", header.mft_size);
    }

    Ok(())
}
