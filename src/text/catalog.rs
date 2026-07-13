use anyhow::Context;
use std::{collections::BTreeMap, path::Path};

use crate::{
    dat::DatArchive,
    pe::PeImage,
    text_records::{
        self, CLIENT_LANGUAGE_CODES, CLIENT_TEXT_FILE_ID_TABLE_VA, CLIENT_TEXT_FILES_PER_LANGUAGE,
        TEXT_RECORDS_PER_FILE,
    },
};

#[derive(Debug, Default)]
pub(crate) struct LocalizedTextCatalog {
    pub(crate) by_text_id: BTreeMap<u32, BTreeMap<String, String>>,
}

pub(crate) struct LocalizedTextReader<'a> {
    archive: &'a mut DatArchive,
    localized_file_ids: Vec<(&'static str, Vec<Option<u32>>)>,
    cache: BTreeMap<u32, BTreeMap<u32, String>>,
    compact_seeds: &'a BTreeMap<u32, u64>,
    decoded_records: &'a BTreeMap<Vec<u8>, String>,
}

impl<'a> LocalizedTextReader<'a> {
    pub(crate) fn new(
        archive: &'a mut DatArchive,
        pe_data: &[u8],
        pe: &PeImage,
        compact_seeds: &'a BTreeMap<u32, u64>,
        decoded_records: &'a BTreeMap<Vec<u8>, String>,
    ) -> anyhow::Result<Self> {
        let localized_file_ids = CLIENT_LANGUAGE_CODES
            .iter()
            .map(|(language_index, code)| {
                pe.language_file_ids(
                    pe_data,
                    CLIENT_TEXT_FILE_ID_TABLE_VA,
                    CLIENT_TEXT_FILES_PER_LANGUAGE,
                    *language_index,
                )
                .map(|ids| (*code, ids))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            archive,
            localized_file_ids,
            cache: BTreeMap::new(),
            compact_seeds,
            decoded_records,
        })
    }

    pub(crate) fn file_id(&self, language_code: &str, file_index: usize) -> Option<u32> {
        self.localized_file_ids
            .iter()
            .find(|(code, _)| *code == language_code)
            .and_then(|(_, file_ids)| file_ids.get(file_index))
            .copied()
            .flatten()
    }

    pub(crate) fn read_resource_file(&mut self, file_id: u32) -> anyhow::Result<Option<Vec<u8>>> {
        self.archive.read_file_id(file_id)
    }

    pub(crate) fn text(&mut self, text_id: u32) -> anyhow::Result<BTreeMap<String, String>> {
        let file_index = (text_id / TEXT_RECORDS_PER_FILE) as usize;
        let record_index = text_id % TEXT_RECORDS_PER_FILE;
        self.localized_record(file_index, record_index)
    }

    pub(crate) fn localized_record(
        &mut self,
        file_index: usize,
        record_index: u32,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let language_files = self
            .localized_file_ids
            .iter()
            .map(|(code, file_ids)| (*code, file_ids.get(file_index).copied().flatten()))
            .collect::<Vec<_>>();
        let mut localized = BTreeMap::new();
        for (code, file_id) in language_files {
            let Some(file_id) = file_id else {
                continue;
            };
            if let Some(text) = self.record(file_id, file_index, record_index)?
                && !text.is_empty()
            {
                localized.insert(code.to_string(), text);
            }
        }
        Ok(localized)
    }

    fn record(
        &mut self,
        file_id: u32,
        file_index: usize,
        record_index: u32,
    ) -> anyhow::Result<Option<String>> {
        if !self.cache.contains_key(&file_id) {
            let Some(entry_bytes) = self.archive.read_file_id(file_id)? else {
                self.cache.insert(file_id, BTreeMap::new());
                return Ok(None);
            };
            let compact_seeds = self
                .compact_seeds
                .iter()
                .filter_map(|(&text_id, &seed)| {
                    (text_id / TEXT_RECORDS_PER_FILE == file_index as u32)
                        .then_some((text_id % TEXT_RECORDS_PER_FILE, seed))
                })
                .collect::<BTreeMap<_, _>>();
            let records = text_records::parse_text_record_map_with_decoded_records_and_seeds(
                &entry_bytes,
                self.decoded_records,
                &compact_seeds,
            )
            .with_context(|| format!("parsing text records from DAT file {file_id}"))?;
            self.cache.insert(file_id, records);
        }
        Ok(self
            .cache
            .get(&file_id)
            .and_then(|records| records.get(&record_index))
            .cloned())
    }
}

pub(crate) fn resolve_localized_text_catalog(
    gw_dat_path: &Path,
    text_ids: impl IntoIterator<Item = u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> anyhow::Result<LocalizedTextCatalog> {
    let mut archive = DatArchive::open(gw_dat_path)?;
    let pe_data = archive.client_pe_data()?;
    let pe = PeImage::parse(&pe_data)?;
    resolve_localized_text_catalog_with_client(
        &mut archive,
        &pe_data,
        &pe,
        text_ids,
        compact_seeds,
        decoded_records,
    )
}

pub(crate) fn resolve_localized_text_catalog_with_client(
    archive: &mut DatArchive,
    pe_data: &[u8],
    pe: &PeImage,
    text_ids: impl IntoIterator<Item = u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> anyhow::Result<LocalizedTextCatalog> {
    let mut reader =
        LocalizedTextReader::new(archive, pe_data, pe, compact_seeds, decoded_records)?;
    let mut by_text_id = BTreeMap::new();
    for text_id in text_ids {
        let localized = reader.text(text_id)?;
        if !localized.is_empty() {
            by_text_id.insert(text_id, localized);
        }
    }
    Ok(LocalizedTextCatalog { by_text_id })
}
