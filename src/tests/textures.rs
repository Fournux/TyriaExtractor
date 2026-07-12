use super::*;

fn compact_dxt3_atex_with_subcode3_alpha(alpha_nibble: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"ATEXDXT3");
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());

    let control_word = (alpha_nibble << 28) | (0b11_u32 << 20);
    bytes.extend_from_slice(&control_word.to_le_bytes());
    bytes.extend_from_slice(&0x07e0_f800_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

fn minimal_dxt1_dds() -> Vec<u8> {
    let mut bytes = vec![0_u8; 128];
    bytes[0..4].copy_from_slice(b"DDS ");
    bytes[4..8].copy_from_slice(&124_u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&4_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&4_u32.to_le_bytes());
    bytes[76..80].copy_from_slice(&32_u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&0x4_u32.to_le_bytes());
    bytes[84..88].copy_from_slice(b"DXT1");
    bytes[108..112].copy_from_slice(&0x1000_u32.to_le_bytes());
    bytes.extend_from_slice(&0xf800_u16.to_le_bytes());
    bytes.extend_from_slice(&0x07e0_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

#[test]
fn atex_header_parser_handles_reference_dxt_variants() -> anyhow::Result<()> {
    let header = crate::atex::parse_header(b"ATEXDXTA\x40\x00\x20\x00")?;
    assert_eq!(header.container, crate::atex::AtexContainer::Atex);
    assert_eq!(header.format.as_fourcc(), "DXTA");
    assert_eq!(header.format.dds_fourcc(), "DXT5");
    assert_eq!(header.width, 64);
    assert_eq!(header.height, 32);

    let header = crate::atex::parse_header(b"ATTXDXTL\x00\x01\x00\x01")?;
    assert_eq!(header.container, crate::atex::AtexContainer::Attx);
    assert_eq!(header.format.as_fourcc(), "DXTL");
    assert_eq!(header.format.dds_fourcc(), "DXT5");
    assert_eq!(header.width, 256);
    assert_eq!(header.height, 256);

    assert!(crate::atex::parse_header(b"ATEXBAD!\x40\x00\x20\x00").is_err());
    assert!(crate::atex::parse_header(b"ATEXDXT1").is_err());
    Ok(())
}

#[test]
fn compact_dxt3_atex_subcode3_decodes_uniform_alpha_and_color() -> anyhow::Result<()> {
    let atex = compact_dxt3_atex_with_subcode3_alpha(7);

    let (width, height, rgba) = crate::atex::decode_atex_rgba(&atex)?;

    assert_eq!((width, height), (4, 4));
    assert_eq!(rgba.len(), 4 * 4 * 4);
    for pixel in rgba.chunks_exact(4) {
        assert_eq!(pixel, &[255, 0, 0, 119]);
    }
    Ok(())
}

#[test]
fn ffna_inline_atex_extraction_finds_embedded_texture_payload() {
    let inline_atex = compact_dxt3_atex_with_subcode3_alpha(15);
    let mut ffna = b"ffna\x01".to_vec();
    ffna.extend_from_slice(&0x0faa_u32.to_le_bytes());
    ffna.extend_from_slice(&(4_u32 + inline_atex.len() as u32).to_le_bytes());
    ffna.extend_from_slice(b"pad!");
    let expected_offset = ffna.len();
    ffna.extend_from_slice(&inline_atex);

    let (offset, payload, header) = crate::items::find_inline_atex_payload(&ffna)
        .expect("FFNA-like stream must expose its embedded ATEX payload");

    assert_eq!(offset, expected_offset);
    assert_eq!(payload, inline_atex.as_slice());
    assert_eq!(header.container, crate::atex::AtexContainer::Atex);
    assert_eq!(header.format.as_fourcc(), "DXT3");
    assert_eq!((header.width, header.height), (4, 4));
}

#[test]
fn model_file_icon_export_records_ffna_inline_atex_metadata_and_png() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let out_dir = temp_dir.path().join("model_file_icons");
    let inline_atex = compact_dxt3_atex_with_subcode3_alpha(15);
    let mut ffna = b"ffna\x01".to_vec();
    ffna.extend_from_slice(&0x0faa_u32.to_le_bytes());
    ffna.extend_from_slice(&(4_u32 + inline_atex.len() as u32).to_le_bytes());
    ffna.extend_from_slice(b"pad!");
    let expected_offset = ffna.len();
    ffna.extend_from_slice(&inline_atex);
    let base_entry = test_mft_entry(10);
    let image_entry = test_mft_entry(42);

    let manifest = crate::items::export_model_file_icon_payload_for_test(
        0x1234,
        "model_stream_1",
        Some(1),
        &base_entry,
        &image_entry,
        &[0xabcd],
        &[0xfeed],
        &ffna,
        &out_dir,
    )?
    .expect("FFNA stream with embedded ATEX must export a model-file icon manifest row");

    let expected_png = "4660.png";
    assert_eq!(manifest["kind"], "ffna_inline_atex");
    assert_eq!(manifest["inline_texture_offset"], expected_offset);
    assert_eq!(manifest["width"], 4);
    assert_eq!(manifest["height"], 4);
    assert_eq!(manifest["format"], "DXT3");
    assert_eq!(manifest["png"], expected_png);

    let exported = image::ImageReader::open(out_dir.join(expected_png))?.decode()?;
    assert_eq!((exported.width(), exported.height()), (4, 4));

    Ok(())
}

fn test_mft_entry(index: u32) -> crate::models::MftEntry {
    crate::models::MftEntry {
        index,
        offset: 0,
        size: 0,
        compression: 0,
        content: 0,
        content_type: 0,
        id: 0,
        crc: 0,
    }
}

#[test]
fn mft_file_id_lookup_handles_contiguous_and_sparse_entry_indexes() {
    let hash_to_mft = BTreeMap::from([(0x1000, 2), (0x2000, 42)]);
    let contiguous_entries = vec![test_mft_entry(1), test_mft_entry(2)];
    let sparse_entries = vec![test_mft_entry(42)];

    assert_eq!(
        lookup_mft_entry_for_file_id(0x1000, &hash_to_mft, &contiguous_entries)
            .map(|entry| entry.index),
        Some(2)
    );
    assert_eq!(
        lookup_mft_entry_for_file_id(0x2000, &hash_to_mft, &sparse_entries)
            .map(|entry| entry.index),
        Some(42)
    );
}

#[test]
fn model_file_icon_export_records_stream1_atex_metadata_and_model_filename() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let out_dir = temp_dir.path().join("model_file_icons");
    let atex = compact_dxt3_atex_with_subcode3_alpha(15);
    let base_entry = test_mft_entry(10);
    let image_entry = test_mft_entry(42);

    let manifest = crate::items::export_model_file_icon_payload_for_test(
        0x1234,
        "model_stream_1",
        Some(1),
        &base_entry,
        &image_entry,
        &[0xabcd],
        &[0xfeed],
        &atex,
        &out_dir,
    )?
    .expect("stream-1 ATEX payload must export a model-file icon manifest row");

    let expected_png = "4660.png";
    assert_eq!(manifest["model_file_id"], 0x1234);
    assert_eq!(manifest["source"], "model_stream_1");
    assert_eq!(manifest["stream_id"], 1);
    assert_eq!(manifest["base_mft_entry_index"], 10);
    assert_eq!(manifest["base_hashes"], serde_json::json!([0xabcd]));
    assert_eq!(manifest["base_relative_path"], "000/000010.bin");
    assert_eq!(manifest["image_mft_entry_index"], 42);
    assert_eq!(manifest["image_hashes"], serde_json::json!([0xfeed]));
    assert_eq!(manifest["image_relative_path"], "000/000042.bin");
    assert_eq!(manifest["kind"], "atex");
    assert_eq!(manifest["width"], 4);
    assert_eq!(manifest["height"], 4);
    assert_eq!(manifest["format"], "DXT3");
    assert_eq!(manifest["png"], expected_png);
    assert!(!expected_png.contains('/'));
    assert!(!Path::new(expected_png).is_absolute());

    let exported = image::ImageReader::open(out_dir.join(expected_png))?.decode()?;
    assert_eq!((exported.width(), exported.height()), (4, 4));

    Ok(())
}

#[test]
fn model_file_icon_export_default_skips_direct_candidate_without_stream1() {
    let entries = [test_mft_entry(10)];

    let candidate = crate::items::model_file_icon_candidate_for_test(&entries[0], false, &entries);

    assert_eq!(candidate, None);
}

#[test]
fn model_file_icon_export_include_direct_uses_direct_candidate_without_stream1() {
    let entries = [test_mft_entry(10)];

    let candidate = crate::items::model_file_icon_candidate_for_test(&entries[0], true, &entries);

    assert_eq!(candidate, Some(("direct_file", None, 10)));
}

#[test]
fn model_file_icon_export_prefers_stream1_candidate_when_direct_is_enabled() {
    let mut entries = [test_mft_entry(10), test_mft_entry(42)];
    entries[0].id = 42;
    entries[1].content = 1;

    let candidate = crate::items::model_file_icon_candidate_for_test(&entries[0], true, &entries);

    assert_eq!(candidate, Some(("model_stream_1", Some(1), 42)));
}

#[test]
fn model_file_icon_export_records_direct_atex_metadata_for_include_direct() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let out_dir = temp_dir.path().join("model_file_icons");
    let atex = compact_dxt3_atex_with_subcode3_alpha(15);
    let base_entry = test_mft_entry(10);

    let manifest = crate::items::export_model_file_icon_payload_for_test(
        0x1234,
        "direct_file",
        None,
        &base_entry,
        &base_entry,
        &[0xabcd],
        &[0xabcd],
        &atex,
        &out_dir,
    )?
    .expect("include-direct diagnostic ATEX payload must export a manifest row");

    let expected_png = "4660.png";
    assert_eq!(manifest["model_file_id"], 0x1234);
    assert_eq!(manifest["source"], "direct_file");
    assert!(manifest["stream_id"].is_null());
    assert_eq!(manifest["base_mft_entry_index"], 10);
    assert_eq!(manifest["base_hashes"], serde_json::json!([0xabcd]));
    assert_eq!(manifest["base_relative_path"], "000/000010.bin");
    assert_eq!(manifest["image_mft_entry_index"], 10);
    assert_eq!(manifest["image_hashes"], serde_json::json!([0xabcd]));
    assert_eq!(manifest["image_relative_path"], "000/000010.bin");
    assert_eq!(manifest["kind"], "atex");
    assert_eq!(manifest["width"], 4);
    assert_eq!(manifest["height"], 4);
    assert_eq!(manifest["format"], "DXT3");
    assert_eq!(manifest["png"], expected_png);

    let exported = image::ImageReader::open(out_dir.join(expected_png))?.decode()?;
    assert_eq!((exported.width(), exported.height()), (4, 4));

    Ok(())
}

#[test]
fn model_file_icon_export_records_direct_dds_metadata_and_pixels() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let out_dir = temp_dir.path().join("model_file_icons");
    let dds = minimal_dxt1_dds();
    let base_entry = test_mft_entry(10);

    let manifest = crate::items::export_model_file_icon_payload_for_test(
        0x1234,
        "direct_file",
        None,
        &base_entry,
        &base_entry,
        &[0xabcd],
        &[0xabcd],
        &dds,
        &out_dir,
    )?
    .expect("direct DDS payload must export a model-file icon manifest row");

    let expected_png = "4660.png";
    assert_eq!(manifest["kind"], "dds");
    assert_eq!(manifest["width"], 4);
    assert_eq!(manifest["height"], 4);
    assert_eq!(manifest["format"], "DXT1");
    assert_eq!(manifest["png"], expected_png);

    let exported = image::ImageReader::open(out_dir.join(expected_png))?
        .decode()?
        .to_rgba8();
    assert_eq!((exported.width(), exported.height()), (4, 4));
    assert_eq!(exported.get_pixel(0, 0).0, [255, 0, 0, 255]);

    Ok(())
}

#[test]
fn model_file_icon_export_skips_unsupported_direct_payload_without_png() -> anyhow::Result<()> {
    let temp_dir = TestDir::new()?;
    let out_dir = temp_dir.path().join("model_file_icons");
    let base_entry = test_mft_entry(10);
    let image_entry = test_mft_entry(42);

    let manifest = crate::items::export_model_file_icon_payload_for_test(
        0x1234,
        "direct_file",
        None,
        &base_entry,
        &image_entry,
        &[0xabcd],
        &[0xfeed],
        b"not a supported image payload",
        &out_dir,
    )?;

    assert!(manifest.is_none());
    assert!(!out_dir.join("4660.png").exists());
    assert_eq!(fs::read_dir(&out_dir)?.count(), 0);

    Ok(())
}

#[test]
fn test_decompress_dxt1() -> anyhow::Result<()> {
    let atex_path = Path::new("data/raw/test_90_3438.bin");
    if atex_path.exists() {
        let bytes = fs::read(atex_path)?;
        let temp_dir = TestDir::new()?;
        let out_path = temp_dir.path().join("test_3438_hd.png");
        crate::atex::save_atex_as_png(&bytes, &out_path)?;
        assert!(out_path.exists());
        let meta = fs::metadata(out_path)?;
        assert!(meta.len() > 0);
    }
    Ok(())
}

#[test]
fn uncompressed_atex_planes_are_interleaved_before_dxt_decode() -> anyhow::Result<()> {
    let mut dxt1 = Vec::new();
    dxt1.extend_from_slice(b"ATEXDXT1");
    dxt1.extend_from_slice(&8_u16.to_le_bytes());
    dxt1.extend_from_slice(&4_u16.to_le_bytes());
    dxt1.extend_from_slice(&24_u32.to_le_bytes());
    dxt1.extend_from_slice(&0_u32.to_le_bytes());
    dxt1.extend_from_slice(&0x0000_f800_u32.to_le_bytes());
    dxt1.extend_from_slice(&0x0000_07e0_u32.to_le_bytes());
    dxt1.extend_from_slice(&0_u32.to_le_bytes());
    dxt1.extend_from_slice(&0_u32.to_le_bytes());

    let (_, _, rgba) = crate::atex::decode_atex_rgba(&dxt1)?;
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[(4 * 4)..(4 * 4 + 4)], &[0, 255, 0, 255]);

    let mut dxtl = Vec::new();
    dxtl.extend_from_slice(b"ATEXDXTL");
    dxtl.extend_from_slice(&8_u16.to_le_bytes());
    dxtl.extend_from_slice(&4_u16.to_le_bytes());
    dxtl.extend_from_slice(&40_u32.to_le_bytes());
    dxtl.extend_from_slice(&0_u32.to_le_bytes());
    dxtl.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
    dxtl.extend_from_slice(&0_u32.to_le_bytes());
    dxtl.extend_from_slice(&0x0000_00ff_u32.to_le_bytes());
    dxtl.extend_from_slice(&0_u32.to_le_bytes());
    dxtl.extend_from_slice(&0x0000_f800_u32.to_le_bytes());
    dxtl.extend_from_slice(&0x0000_07e0_u32.to_le_bytes());
    dxtl.extend_from_slice(&0_u32.to_le_bytes());
    dxtl.extend_from_slice(&0_u32.to_le_bytes());

    let (_, _, rgba) = crate::atex::decode_atex_rgba(&dxtl)?;
    assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&rgba[(4 * 4)..(4 * 4 + 4)], &[0, 255, 0, 255]);

    Ok(())
}
