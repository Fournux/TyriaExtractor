use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::{
    dat::DatArchive,
    io_util::write_json,
    pe::{PeImage, PeSection},
    text::{
        apply_encoded_template,
        catalog::{LocalizedTextReader, resolve_localized_text_catalog_with_client},
        encoded_values_from_hex, encoded_values_from_words, encoded_words_from_hex, hex_to_bytes,
        text_references,
    },
    text_records::{self, TEXT_RECORDS_PER_FILE},
};

mod capture;
mod catalog;
mod text;

#[cfg(test)]
mod tests;

use capture::*;
pub(crate) use catalog::export_detected_items_from_packet_log_with_client_strings;
pub(crate) use text::{packet_log_text_inputs, runtime_item_text_lookup_with_compact_seeds};

#[cfg(test)]
use catalog::{RuntimeTextLookup, export_detected_items_from_packet_log};
#[cfg(test)]
use text::{
    asyncdecode_item_ids_for_test, encoded_value_spans_for_test, encoded_values_for_test,
    packet_log_decoded_text_records, packet_log_name_ids, packet_log_name_seeds,
};
