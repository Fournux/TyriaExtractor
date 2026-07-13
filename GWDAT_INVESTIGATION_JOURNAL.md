# Gw.dat investigation journal

Last updated: 2026-07-12

This journal records durable discoveries, rejected interpretations, the latest capture evidence, and unresolved questions. Format details belong in the focused references:

- [Archive and text-record structures](doc/GWDAT_FORMAT.md)
- [Compression and texture decoding](doc/DECOMPRESSION.md)
- [Skill extraction](doc/SKILL_EXTRACTION.md)
- [Runtime item identity, text, and icons](doc/ITEM_EXTRACTION.md)

## Evidence policy

A conclusion is retained here only when it is supported by repeatable binary structure, client-code behavior, consistent runtime observations, or agreement between independent evidence paths. Visual resemblance, proximity, plausible counts, external names, and a single unexplained match are leads, not mappings. Ambiguous joins remain unresolved rather than being selected heuristically.

Negative results are scoped to the data and layouts examined. They reject the stated interpretation; they do not prove that no equivalent structure exists elsewhere.

## Durable parser discoveries

### Odd-aligned UTF-16LE and printable trailers

Some localized UTF-16LE strings begin at odd byte offsets. A scanner restricted to even offsets silently misses valid text; `Hypochondria` was observed at odd offsets in more than one decompressed resource.

A separate parsing error came from treating the printable 16-bit word immediately before a `0x0000, 0x0010` separator as part of the string. That produced suffixes such as `Vampiric Biteb`, `Windborne Speedn`, and `Flame Burstj`. Excluding the trailer yields the expected strings. These were independent bugs: alignment recovery does not fix trailer handling.

### Structured and compact text records

Localized resources are record streams rather than arbitrary UTF-16 runs. The record header supplies the record size, flags, type, and subtype; visible text begins after the header. Structured parsing preserves record ordinals and avoids false joins through binary gaps.

The decisive correction was that record type `0x10` is also compact symbol width 16. A type-`0x10`, subtype-0 record is plain UTF-16LE only when its flags word is zero. With nonzero flags it is a seeded compact text record. Sending those records through the plain-text path caused empty Japanese and Korean results. The compact decoder restored complete locale sets, including confirmed width-16 Japanese text, while the already confirmed width-7 path remained valid.

The full record and decoder rules are maintained in [the format reference](doc/GWDAT_FORMAT.md).

## Confirmed skill resolver milestone

The skill table stores numeric name and description string IDs, but those IDs alone do not identify the correct text resource when resource ranges overlap. Shifted inferred bases can produce many plausible but incorrect matches.

The client language file-ID array supplies the missing resource context. Following that array to the localized resources and then resolving the record ordinal produced text for all 1,329 selected player skills, with zero unresolved rows. The selected set also matched the confirmed campaign and elite distribution. This replaced campaign-specific bases, name whitelists, and count-driven guessing with a client-defined lookup path.

## Confirmed item runtime, text, and icon bridge

Runtime item identity is a pair of distinct namespaces: `model_id` and `model_file_id`. The item message also carries the encoded name, while the runtime item object can expose an encoded description. Neither the display-text ID nor `model_id` is an icon file identifier.

Observed name and description EncStrings can be reduced to the text IDs requested by the client decoder and resolved against localized DAT text resources. On the observed item corpus, the compact EncString predictor reproduced every client-emitted text-ID sequence. Missing descriptions are attributable to absent description EncStrings, not failed DAT text lookup.

The icon path is independently confirmed:

```text
runtime model_file_id
  -> DAT hash alias
  -> base MFT entry
  -> linked stream with content selector 1
  -> inline ATEX texture
  -> inventory icon
```

Live model-file IDs for the Salvage Kit, Pile of Glittering Dust, and Grim Cesta resolve as alternate DAT hash aliases of their already identified local model resources. This proves the runtime-to-archive icon bridge. It does not provide the still-missing complete static `model_id -> model_file_id` mapping.

## Disproved hypotheses

- **Hard-coded skill string bases identify the correct resource.** Overlapping ranges allow shifted bases to resolve unrelated but plausible text. The client language file-ID array supplies the required context instead.
- **An item ID, item text ID, or runtime `model_id` can be used directly as an icon hash.** Tested values resolved to unrelated sounds, material textures, or other resources. Runtime evidence keeps these namespaces separate from `model_file_id`.
- **The integer adjacent to a model-file value in snapshot entry 2 is a general item `model_id`.** Entry 2 is the archive hash lookup table. Candidate adjacent pairs do not agree with trusted runtime pairs.
- **The client image or extracted entry 2 contains an obvious packed table of trusted `model_id` and `model_file_id` pairs.** Adjacent scans in both orders over all 382 trusted pairs found no matches; wider proximity produced accidental or cross-record coincidences, not a stable record layout.
- **Visual similarity or MFT locality safely links standalone textures to items.** Item-like icons share neighborhoods with environment art, UI strips, masks, and unrelated textures. Locality is useful for inspection but cannot establish identity.
- **The icon-bearing file ID is proven to be server-only.** Client handling proves that it arrives in decoded runtime item state, but does not prove its ultimate provenance or rule out an unidentified local source.

## Current capture evidence

Two independent item captures currently provide the following coverage:

| Capture | Unique `(model_id, model_file_id)` pairs | Fully named | Fully described |
| --- | ---: | ---: | ---: |
| Older capture | 382 | 382 | 361 |
| Fresh compact capture | 322 | 322 | 301 |

Their pair sets overlap by 318. The compact capture adds 4 pairs, the older capture has 64 pairs not seen in the compact capture, and their union contains 386 pairs.

In each capture, the number of missing descriptions is exactly the number of rows lacking a description EncString: 21 in the older capture and 21 in the compact capture. No captured description EncString remains unresolved. Thus the description shortfall measures capture coverage, not decoder or localized-resource failure.

Three historically observed mappings still lack a complete current compact capture:

- `1882 -> 9528` — Wooden Buckler
- `327 -> 14368` — Reinforced Buckler
- `383 -> 112081` — Holy Staff

They remain valid historical observations but are not promoted to fully captured catalog rows.

## Audit cross-checks (2026-07-10)

- `ItemGeneral +0xb0` is a count of `u32` modifier words, not a byte count. With at most 64 words, the decoded message occupies `0xb4 + count * 4` bytes and ends no later than `0x1b4`. The Rust sniffer and legacy Py4GW helper now preserve all counted words.
- In the current 3,443-row skill table, all 168 locally indexed `_PvP` skills carry `0x00400000`; none carries the previously used `0x00100000`. The validated PvE bit is `0x00080000`. Correcting these masks keeps the extracted player corpus at the required 1,329 skills.

## Retired item diagnostics (2026-07-11)

- Unreachable item scanners and image-corpus utilities were removed from the extractor rather than kept behind a module-wide `dead_code` suppression. They were diagnostic experiments, had no CLI entry point, and were not part of the current `extract items` pipeline.
- The build-specific client addresses `0x00A15728` and `0x00A19DA8` were only provisional GMCTL table leads. Their adjacent resource fields did not establish a stable item-icon mapping and must not be treated as format constants.
- Proximity probes seeded with the observed Salvage Kit, Pile of Glittering Dust, and Grim Cesta pairs found no repeatable packed `model_id -> model_file_id` layout. The observations remain valid runtime evidence; the proximity scanner did not become an extractor algorithm.
- Standalone image-corpus aliasing, MFT locality, and sprite tiling remain unsuitable for canonical item joins. The supported icon path remains the runtime `model_file_id` hash alias followed by linked stream 1.

## Quest extraction findings (2026-07-11 to 2026-07-12)

### First-party source boundary

Decoding the supplied runtime EncStrings against the current local `Gw.dat` confirms that localized quest categories, names, NPC labels, descriptions, rewards, and objectives are DAT text records. Representative objective text IDs are `62150`, `75872`, `62138`, `62139`, `62142`, `62140`, and `62141` for quest `0x389`; `74581` for `0x38B`; and `63099`, `75877`, and `63104` for `0x393`. Category IDs `62683` and `62681` resolve to Norn and Asura in every available client language, while French record `71288` is the template `Quêtes principales : %str1%`.

Runtime EncStrings supply text IDs, 64-bit compact seeds, objective order, and completed/pending wrappers (`10741` / `10742`); runtime quest packets supply `quest_id`, state, maps, and markers. `Gw.dat` supplies the localized text, but not those runtime relations.

None of the 15 examined compact seeds occurs literally in its DAT record, the current client PE, or the raw DAT image. This proves only that those records are not self-keying and that no literal seed table was found. It does not rule out an unidentified derivation.

No complete static `quest_id -> metadata/text references` inventory has been identified in the current DAT or client image. The apparent PE table indexed through `2076` belongs to `ConstEffect.cpp::s_effect`, not quests. GWCA's `RequestQuestInfoId` also requires the quest to exist in the active quest log, so it is not an arbitrary-ID enumerator.

### Runtime packet schemas

A direct official-client capture confirmed the following fixed packet layouts. Byte counts include the 32-bit header.

| Header | Relevant content | Bytes |
| --- | --- | ---: |
| `0x0020 AGENT_SPAWNED` | `agent_id`, NPC model in `agent_type`, position/state | 116 |
| `0x0021 AGENT_DESPAWNED` | `agent_id` | 8 |
| `0x0049 QUEST_ADD` | `quest_id`, marker, `map_to`, state, category/name/NPC `[8]`, `map_from` | 80 |
| `0x004C QUEST_DESCRIPTION` | `quest_id`, description `[128]`, objectives `[128]` | 520 |
| `0x0050 QUEST_GENERAL_INFO` | `quest_id`, state, category/name/NPC `[8]`, `map_from` | 64 |
| `0x0051 QUEST_UPDATE_MARKER` | `quest_id`, marker, `map_to` | 24 |
| `0x0052 QUEST_REMOVE` | `quest_id` | 8 |
| `0x0053 QUEST_ADD_MARKER` | `quest_id`, marker, `map_to` | 24 |
| `0x0054 QUEST_UPDATE_OBJECTIVES` | `quest_id`, objectives `[128]` | 264 |
| `0x0056 NPC_UPDATE_PROPERTIES` | NPC model/file IDs, flags, profession, level, name `[8]` | 52 |
| `0x007E DIALOG_BUTTON` | icon, button text `[128]`, `dialog_id`, skill ID | 272 |
| `0x0081 DIALOG_SENDER` | `agent_id` | 8 |
| `0x0199 INSTANCE_LOAD_INFO` | player `agent_id`, map, instance metadata | 28 |

Two clean full-pipeline captures confirm installation and valid framing for all 13 families. They include nonzero `AGENT_DESPAWNED` traffic and `INSTANCE_LOAD_INFO` contexts for maps `194`, `148`, and `146`. Hook installation records the current client's field descriptors and rejects any family whose calculated size differs from the expected fixed size.

### Clean dialogue, NPC, and reward evidence

An earlier clean new-character capture contains 416 rows, including 25 `DIALOG_SENDER` and 22 quest `DIALOG_BUTTON` packets. Every quest button joined through `DIALOG_SENDER.agent_id -> AGENT_SPAWNED.agent_id -> NPC model -> NPC_UPDATE_PROPERTIES name`. Action `2` is refusal and is excluded from the consumer relation set.

The latest zero-state capture used the standalone sniffer after removal of the GWCA runtime bridge. It contains 491 quest-log rows (13 schemas, one hook status, 452 packets, and 25 `capture_health` rows) plus 1,605 item-log rows (1,580 compact `ItemGeneral` observations and 25 health rows). All 13 quest packet families are present; every loss and write-failure counter remains zero. The item log contains no `runtime_item_strings`, yet regeneration produces all 370 distinct observed `(model_id, model_file_id)` rows with names in all 11 official languages. Reward-dialog-to-`QUEST_REMOVE` lifecycles are complete for quests `0x0050`, `0x0051`, and `0x00D9`; together with the preceding clean capture, this also proves `0x0054` and `0x00DC`. Current quest regeneration produces 13 quest IDs, 20 localized observed steps in 12 distinct captured sequences, and nine NPC-role relations.

`GW::Quest::npc` is not a giver field: quest `0x0050` names the Royal Herald while its observed giver is model `1480` and its reward NPC is model `1459`; quest `0x0036` names Haversdan while models `1458` and `1505` fill those roles. The consumer therefore keeps `npc_*` text separate from observed `quest_npcs` relations.

The earlier clean log also exposed one false `0x0054` handler-argument candidate with quest ID `0x532CC66C`. Both the hook and offline decoder now reject quest IDs outside `1..=65535`. Its corrected catalog contains eight real quest IDs and 14 NPC-role relations.

Reward components are nested in `QUEST_DESCRIPTION` EncStrings. Confirmed wrapper IDs are `10728` (section), `10730` (experience), `10732` (gold), `10735` (item), and `10738` (skill). Quest `0x002E`, for example, decodes to 200 experience, 50 gold, and item text ID `9741`. Item wrappers contain a seeded text reference but no item model ID. For quest `0x003E`, exact reference `(9553, 855850257207)` joins conservatively to the uniquely observed `ITEM_GENERAL_INFO` model `(2817, 9528)`. If a reference is ambiguous, temporal resolution additionally requires a reward button followed by `QUEST_REMOVE` in the same session and exactly one matching model near that removal.

### Consumer and corpus semantics

`quests.json` is a deterministic consumer projection, not an evidence ledger. It keeps `quest_id`, every nonzero observed `origin_map_ids` value, localized static fields, `observed_steps`, distinct `observed_step_sequences`, structured rewards, and stable NPC model/role/map evidence. Raw timestamps, state flags, encoded payloads, transient agent IDs, dialogue buttons, removals, `map_to`, and markers remain in JSONL. Observed step sequences are variants, not a claim of exhaustive or canonical branch order.

`map_from` is the observed origin map. `map_to` belongs to the current `GamePos` marker and can change during one quest; quest `0x003E` emitted target maps `164`, `148`, and `164` again. Raw marker `x`/`y` values are in-map coordinates and `zplane` is an integer plane, not altitude.

The corpus is append-only and session-scoped. New rows carry `session_id`; quest schema/status rows also carry the client PE timestamp. `AGENT_DESPAWNED` and `INSTANCE_LOAD_INFO` prevent transient agent joins from leaking across despawns or map changes. `capture_health` rows expose lock drops, capacity evictions, and write failures; a missing health row is not evidence of zero loss.

The offline consumer now enforces that evidence boundary. Every session containing quest or item data must include `capture_health`; nonzero lock drops, capacity evictions, or write failures reject extraction. Quest sessions must additionally provide all 13 `quest_schema` rows, with recognized names, internally consistent field counts, packet-size-compatible descriptors, and no conflicting client PE timestamps. `extract items` and `extract quests` fail closed by default; `--allow-unverified-capture` exists only for explicit legacy-log recovery. Successful capture-backed extractions write a deterministic `capture.json` sidecar (schema version 1) containing per-session counters, schema headers, client timestamp, verification status, and issues.

No packet or client structure examined contains generic quest minimum levels, prerequisite quest IDs, or an availability expression. `DIALOG_BUTTON` proves only that an action was available to that character at that moment. Candidate conditions require controlled cross-character observations; manually curated requirements must remain in a separate site layer keyed by `quest_id`, never in reproducible extractor output without first-party evidence.

## Open questions

1. **Where is the complete static `model_id -> model_file_id` bridge, if one exists?** Runtime pairs and archive alias resolution are confirmed, but no complete offline table or derivation has been identified.
2. **What constitutes complete runtime item coverage?** The union continues to grow across captures, and there is no confirmed finite item-definition inventory against which to prove completeness.
3. **How are standalone ATEX service-item and trophy icons linked safely?** Their textures are present, but a visual label or nearby MFT position is insufficient evidence for a canonical item mapping.
4. **Does the observed compact EncString rule generalize beyond the captured item subset?** It exactly matches the current item and supplied quest evidence, but broader control-word behavior remains unproven.
5. **How can exhaustive dialog coverage be proven across every map, campaign state, profession, character, and prerequisite combination?** The fresh capture alone contains 13 quest IDs and validates three full reward lifecycles, but the complete traversal set remains unknown.
6. **Which quests have mutually exclusive or branching stages?** The consumer now preserves every distinct captured objective sequence under `observed_step_sequences` instead of flattening all references into a claimed chronology, but each quest still needs sufficient lifecycle coverage before those observed variants can be treated as exhaustive.
7. **How are reward-item quantities, stat payloads, and mutually exclusive choices represented?** Exact seeded reward-name references can use globally unique models or conservative completion-gated same-session correlation, but the reward EncString itself contains no model ID in the validated sample. Quantities, stats, and choice-group semantics still need a proven first-party structure or controlled completion captures.
