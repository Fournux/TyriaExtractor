use super::*;

#[test]
fn read_dat_table_reads_header_entries_and_hash_lookup() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("Gw.dat");
    write_minimal_gw_dat(&dat)?;

    let metadata = fs::metadata(&dat)?;
    let mut file = File::open(&dat)?;
    let (header, mft, entries) = read_dat_table(&mut file, &dat, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;

    assert_eq!(metadata.len(), 1546);
    assert_eq!(header.magic_hex, "33414e1a");
    assert_eq!(header.version, 0x1a);
    assert_eq!(header.header_size, 32);
    assert_eq!(header.sector_size, 512);
    assert_eq!(header.mft_offset, 512);
    assert_eq!(header.mft_size, 120);
    assert_eq!(mft.magic_hex, "4d66741a");
    assert_eq!(mft.entry_count, 5);
    assert_eq!(entries.len(), 4);
    assert_eq!(hash_lookup.len(), 2);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.content == 3 && entry.size > 0)
            .count(),
        4
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.compression == 8)
            .count(),
        1
    );

    let payload = &entries[3];
    assert_eq!(payload.index, 4);
    assert_eq!(payload.offset, 1536);
    assert_eq!(payload.size, 10);
    assert_eq!(payload.compression, 8);
    assert_eq!(payload.content_type, 2);
    assert_eq!(payload.id, 20);
    assert_eq!(payload.crc, 0xdead_beef);

    Ok(())
}

#[test]
fn read_hash_lookup_preserves_duplicate_aliases_by_mft_entry() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("Gw.dat");
    write_hash_lookup_gw_dat(&dat)?;

    let metadata = fs::metadata(&dat)?;
    let mut file = File::open(&dat)?;
    let (_, _, entries) = read_dat_table(&mut file, &dat, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;
    let hashes_by_mft = hash_lookup_by_mft_index(&hash_lookup);

    assert_eq!(entries.len(), 5);
    assert_eq!(hash_lookup.len(), 3);
    assert_eq!(hashes_by_mft.len(), 2);
    assert_eq!(hashes_by_mft.get(&3), Some(&vec![0x1000, 0x1001]));
    assert_eq!(hashes_by_mft.get(&4), Some(&vec![0x2000]));
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.content == 3 && entry.size > 0)
            .map(|entry| entry.index)
            .collect::<Vec<_>>(),
        vec![1, 3, 4, 5]
    );

    Ok(())
}

#[test]
fn dat_stream_lookup_prefers_linked_content_over_content_type_fallback() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("Gw.dat");
    write_stream_chain_gw_dat(&dat)?;

    let metadata = fs::metadata(&dat)?;
    let mut file = File::open(&dat)?;
    let (_, _, entries) = read_dat_table(&mut file, &dat, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;
    let hash_to_mft = hash_lookup_by_file_id(&hash_lookup);
    let base_entry = lookup_mft_entry_for_file_id(0x3000, &hash_to_mft, &entries)
        .expect("base file id must resolve");

    let stream_entry = lookup_mft_stream_entry_from_base(base_entry, 1, &entries).expect(
        "stream 1 should prefer the linked content=1 MFT entry over a content_type fallback",
    );
    assert_eq!(stream_entry.index, 4);
    assert_eq!(stream_entry.content, 1);

    let icon = read_dat_entry_from_file(&mut file, metadata.len(), stream_entry)?;
    assert_eq!(icon, b"icon!!");

    Ok(())
}

#[test]
fn detect_entry_kind_classifies_representative_prefixes() {
    let utf16le_text = utf16le_fixture(&["Wide Text"]);

    for (name, bytes, expected) in [
        (
            "ATEX texture",
            b"ATEXDXT1\x20\x00\x20\x00".as_slice(),
            "atex_texture",
        ),
        (
            "ATTX texture",
            b"ATTXDXTL\x00\x01\x00\x01".as_slice(),
            "attx_texture",
        ),
        ("ffna", b"ffna payload".as_slice(), "ffna"),
        ("DDS texture", b"DDS payload".as_slice(), "dds_texture"),
        ("sound", [0xff, 0xfa, 0x02, 0x03].as_slice(), "sound"),
        ("UTF-16LE text", utf16le_text.as_slice(), "text_utf16le"),
        (
            "ASCII text",
            b"Plain ASCII payload".as_slice(),
            "text_or_binary_ascii",
        ),
        (
            "unknown binary",
            [0x00, 0x01, 0x02, 0x03].as_slice(),
            "unknown",
        ),
    ] {
        assert_eq!(detect_entry_kind(bytes), expected, "{name}");
    }
}

#[test]
fn file_reference_codec_matches_legacy_formula() {
    let (id0, id1) = encode_file_reference(0x1000);

    assert_eq!((id0, id1), (0x10ff, 0x0100));
    assert_eq!(decode_file_reference(id0, id1), 0x1000);
    assert_eq!(decode_file_reference(0x0100, 0x0100), 1);
}

#[test]
fn detect_entry_kind_classifies_legacy_gw_unpacker_prefixes() {
    for (name, bytes, expected) in [
        (
            "text header",
            b";=== localized text".as_slice(),
            "text_resource",
        ),
        (
            "text header alt",
            b";*** localized text".as_slice(),
            "text_resource",
        ),
        ("AMAT material", b"AMAT payload".as_slice(), "amat_material"),
        ("AMP sound", b"AMP payload".as_slice(), "sound"),
        ("ID3 sound", b"ID3 payload".as_slice(), "sound"),
        (
            "MP3 frame sound",
            [0xff, 0xfb, 0x90, 0x64].as_slice(),
            "sound",
        ),
    ] {
        assert_eq!(detect_entry_kind(bytes), expected, "{name}");
    }
}

#[test]
fn dump_entries_includes_hashes_kind_magic_and_utf16_metadata() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("Gw.dat");
    let out_dir = temp_dir.path().join("cache");
    write_hash_lookup_gw_dat(&dat)?;

    let manifest = dump_entries(&dat, &out_dir, None)?;

    assert_eq!(manifest.header.version, 0x1a);
    assert_eq!(manifest.mft.entry_count, 6);
    let manifest_json = serde_json::to_value(&manifest)?;
    assert_eq!(
        manifest_json["header"]["magic_hex"].as_str(),
        Some("33414e1a")
    );
    assert_eq!(manifest_json["mft"]["unknown_1"].as_u64(), Some(0));
    assert_eq!(manifest.active_entries, 4);
    assert_eq!(manifest.decompressed_entries, 0);
    assert_eq!(manifest.failed_entries, 0);
    assert_eq!(manifest.dumped_entries, 4);
    assert_eq!(manifest.entries.len(), 4);
    assert!(manifest.entries.iter().all(|entry| entry.compression == 0));

    let text_entry = manifest
        .entries
        .iter()
        .find(|entry| entry.index == 3)
        .expect("dump must include the UTF-16LE active entry");
    assert_eq!(text_entry.hashes, vec![0x1000, 0x1001]);
    assert_eq!(text_entry.magic_hex, "53006b0069006c00");
    assert_eq!(text_entry.kind, "text_utf16le");
    assert_eq!(text_entry.utf16le_string_count, 2);
    assert!(
        manifest
            .cache_root
            .join("entries")
            .join(&text_entry.relative_path)
            .is_file()
    );

    Ok(())
}

#[test]
fn file_id_lookup_resolves_hash_aliases_to_archive_entries() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("Gw.dat");
    write_hash_lookup_gw_dat(&dat)?;

    let metadata = fs::metadata(&dat)?;
    let mut file = File::open(&dat)?;
    let (_, _, entries) = read_dat_table(&mut file, &dat, metadata.len())?;
    let hash_lookup = read_hash_lookup(&mut file, metadata.len(), &entries)?;
    let hash_to_mft = hash_lookup_by_file_id(&hash_lookup);
    let hashes_by_mft = hash_lookup_by_mft_index(&hash_lookup);

    let text_entry = lookup_mft_entry_for_file_id(0x1000, &hash_to_mft, &entries)
        .expect("text file id must resolve");
    assert_eq!(text_entry.index, 3);
    assert_eq!(
        hashes_by_mft.get(&text_entry.index),
        Some(&vec![0x1000, 0x1001])
    );
    let text_bytes = read_dat_entry_from_file(&mut file, metadata.len(), text_entry)?;
    assert_eq!(detect_entry_kind(&text_bytes), "text_utf16le");
    assert_eq!(utf16le_strings(&text_bytes).len(), 2);

    let ascii_entry = lookup_mft_entry_for_file_id(0x2000, &hash_to_mft, &entries)
        .expect("ASCII file id must resolve");
    assert_eq!(ascii_entry.index, 4);
    let ascii_bytes = read_dat_entry_from_file(&mut file, metadata.len(), ascii_entry)?;
    assert_eq!(detect_entry_kind(&ascii_bytes), "text_or_binary_ascii");

    Ok(())
}

#[test]
fn read_dat_table_rejects_non_guild_wars_magic() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("not-gw.dat");
    fs::write(&dat, [0_u8; 32])?;
    let mut file = File::open(&dat)?;

    let err = read_dat_table(&mut file, &dat, 32).expect_err("invalid DAT magic must fail");

    assert!(err.to_string().contains("not a Guild Wars Gw.dat file"));
    Ok(())
}

#[test]
fn dat_entry_bounds_reject_offset_overflow() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let dat = temp_dir.path().join("tiny.dat");
    fs::write(&dat, [0_u8])?;
    let mut file = File::open(dat)?;
    let entry = crate::models::MftEntry {
        index: 99,
        offset: u64::MAX,
        size: 1,
        compression: 0,
        content: 3,
        content_type: 0,
        id: 0,
        crc: 0,
    };

    let err = crate::dat::read_dat_entry_from_file(&mut file, 1, &entry)
        .expect_err("overflowing MFT entry range must fail cleanly");

    assert!(err.to_string().contains("range overflow"));
    Ok(())
}
