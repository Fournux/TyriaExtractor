# Gw.dat investigation journal

Last updated: 2026-07-15

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

The client language file-ID array supplies the missing resource context. Following that array to the localized resources and then resolving the record ordinal produces text in all 11 client languages for every row in the corrected 1,488-ID template corpus. This replaced campaign-specific bases, name whitelists, and count-driven guessing with a client-defined lookup path.

The overcast byte at skill-row offset `0x34` is conditional: it is a cost only when bit `0x00000001` of the `0x10` flags field is set. Applying that condition to the corrected corpus leaves 24 overcast skills (14 at 5 and 10 at 10) and writes zero for the other 1,464 rows.

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

## Audit cross-checks (2026-07-10, corrected 2026-07-15)

- `ItemGeneral +0xb0` is a count of `u32` modifier words, not a byte count. With at most 64 words, the decoded message occupies `0xb4 + count * 4` bytes and ends no later than `0x1b4`. The Rust sniffer and legacy Py4GW helper now preserve all counted words.
- The current 3,443-row skill table contains 177 rows carrying the validated PvP bit `0x00400000`. Exactly 155 equip/use-family-`0` rows form reciprocal `0x2c` links with a non-PvP family-`1` base row; those are the current PvP variants. Twenty-one unreferenced family-`0` PvP rows and one family-`1` PvP row are outside this boundary. The validated PvE bit remains `0x00080000`.
- The corrected corpus is 1,333 non-PvP family-`1` rows plus those 155 variants. It excludes the five internal family-`2` IDs `829`, `833`, `861`, `868`, and `940`; includes IDs `2`, `3`, `1814`, `1815`, and `1816`; and retains special IDs `3418` through `3421` with their client-provided not-playable flag and `...` text.

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
| `0x009B AGENT_UPDATE_NPC_NAME` | `agent_id`, display-name EncString `[32]` | 72 |
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

The corpus is append-only and session-scoped. New rows carry `session_id`;
world-packet schema/status rows also carry the client PE timestamp.
`AGENT_DESPAWNED` and `INSTANCE_LOAD_INFO` prevent transient agent joins from
leaking across despawns or map changes. The sibling `tyria_capture.jsonl`
metadata stream stores packet schemas, hook status, and capture-health
snapshots. Health is appended only when a loss or write counter changes and is
never mixed into packet JSONLs. Missing health metadata is not evidence of zero
loss.

The offline consumer enforces that evidence boundary. Every session containing
world or item data must include format-3 `capture_health`; nonzero lock drops,
capacity evictions, or write failures reject extraction. World sessions must
provide the 15 format-3 base `world_packet_schema` rows; the optional `0x009B`
schema is recognized and validated when present. All supplied schemas need
internally consistent field counts and packet-size-compatible descriptors;
client PE timestamps must not conflict, and every data row needs a unique
`capture_seq`.
There is no unverified-capture bypass. Successful capture-backed extractions
write a deterministic `capture.json` sidecar (schema version 3) containing the
capture format version, per-session counters, world-schema headers, client
timestamp, verification status, and issues.

No packet or client structure examined contains generic quest minimum levels, prerequisite quest IDs, or an availability expression. `DIALOG_BUTTON` proves only that an action was available to that character at that moment. Candidate conditions require controlled cross-character observations; manually curated requirements must remain in a separate site layer keyed by `quest_id`, never in reproducible extractor output without first-party evidence.

## NPC and collector extraction findings (2026-07-14 to 2026-07-15)

### First-party source boundary

The current clean runtime corpus contains 98 distinct `NPC_UPDATE_PROPERTIES` pairs. The decoded 52-byte message carries `npc_model_id`, `model_file_id`, two unknown scalars, raw scale, flags, profession, level, and an eight-word name EncString. Client function `0x0080EE90` copies those packet fields directly into the runtime NPC array and separately copies the encoded name; it performs no DAT or static-table lookup for the model/file relation. `NPC_UPDATE_MODEL` supplies a bounded variable composite of at most eight additional dwords.

An exhaustive scan of all 176,541 active entries in the current local `Gw.dat` completed with zero read or decompression failures. None of five representative complete `NPC_UPDATE_PROPERTIES` records, their first 32 metadata bytes, their adjacent `npc_model_id + model_file_id` pairs, or their name EncStrings occurs literally in any decompressed entry. The same 90 adjacent pairs occur neither forward nor reversed in the current `Gw.exe`; no matching pair of scalar values occurs within 256 bytes there. These negative probes rule out an obvious literal packed table, not an unidentified encoded or derived representation.

The executable's `ConstNpcBang.cpp::s_npcBang` table is eight 16-byte rows selecting 18 DAT file IDs. Those resources are FFNA type-2 model containers, not an NPC-definition inventory: their chunks are `0xFA0` geometry, `0xFA1` animation, and `0xFA5` file references. The repeated three-way file IDs are identical geometry/animation banks or build variants. Their `0xFA5` references resolve to shared dependent model resources; they contain no observed runtime NPC model IDs.

Consequently the supported source split is:

```text
runtime NPC packets -> stable NPC model/file relation, gameplay metadata, map observations
runtime merchant/window flow -> NPC service role and collector exchange relations
Gw.dat -> localized text and model/resource bytes addressed by observed file IDs
```

### First offline NPC corpus

`extract npcs` consumes the verified NPC, spawn, and instance packets from the
dedicated `tyria_npcs.jsonl` stream. The first generated `npcs/npcs.json`
contains 90 stable NPC model IDs and 39 distinct model file IDs. All 90 entries
retain model/skin IDs, packed visual adjustment, appearance, flags, primary
profession, and default level. Seventy-five models have observed map relations
across maps `146`, `148`, and `194`.

Every one of the 90 names resolves from `Gw.dat` in all 11 official client languages. Eighty-three use the previously supported seeded or tagged text-reference forms. Seven use a short direct EncString consisting of one complete encoded scalar without a seed or `0x010A` trailer; values `82779`, `72910`, `72917`, `72924`, `72959`, `72966`, and `97183` resolve respectively to M.O.X., Vekk, Pyre Fierceshot, Ogden Stonehealer, Gwen, Xandra, and Guardsman Qao Lin. This short form is accepted only when it consumes the complete NPC name EncString.

The packet's visual-adjustment dword maps exactly to the client `CharAdjustment` bytes: signed hue, saturation, and lightness followed by an unsigned scale percentage. The raw dword is retained alongside that decoded projection.

### Collector transaction flow

The current official client receives the relevant window flow around headers `0x0083` through `0x0087`. Function `0x00810DE0` appends item IDs into an accumulator. Function `0x00810E20` publishes one completed list through UI message `0x100000B8` and clears that accumulator. Function `0x00810E60` asserts that the two accumulated lists have equal counts, then publishes `transaction_type`, count, and the two aligned item-ID buffers through UI message `0x100000B9` (`kVendorItems`) before clearing both counts. `CollectorBuy` is transaction type `2`.

The aligned buffers are the authoritative exchange relation. The window owner supplies the transient agent ID, which joins through `AGENT_SPAWNED` to the stable NPC model ID and current map. Item IDs then join to captured `ITEM_GENERAL_INFO` rows for model IDs, file IDs, metadata, and localized names. A collector role or offer is not inferable from NPC flags alone, and no local static collector inventory has been identified. Capturing only this merchant/window family is therefore the justified last-resort runtime addition; unrelated traffic is unnecessary.

### Minimal collector hook and catalog join

The create parameter exposes service at `+0x00`, reward count at `+0x14`, the reward item-ID buffer at `+0x18`, and the player's trophy item ID at `+0x20`. The first live collector capture disproved the earlier interpretation of `create_param + 0x1C` as a quantity: it yielded the address-like value `950396720`. After the official constructor runs, `VnCollect + 0x0C` is the count-needed field and `VnCollect + 0x10` is the required trophy model ID. The corrected hook therefore reads both stable exchange terms from `VnCollect`.

Each `collector_offers` row preserves the transient reward item IDs and joins them immediately through the official item manager to stable model ID, model file ID, and item type. It also records the merchant agent ID and resolves the current NPC model ID through the official agent manager. `extract npcs` accepts only complete collector rows, rejects unresolved or truncated exchange terms, deduplicates identical offers, and emits them under the stable NPC model entry.

Controlled recaptures established the final identity path. The function-entry vendor-window hook was not invoked before collector creation and was removed. Server packet `0x00C4 WINDOW_OWNER` reliably supplied merchant agent `36`, which the official agent manager resolved to NPC model `1471`; the current client advertised schema `0x00C3 WINDOW_MERCHANT` but emitted no such packet for this collector. The constructor's service field at `create_param + 0x00` is therefore the authoritative `CollectorBuy = 2` discriminator, rather than a missing window transaction value.

The validated offer for NPC model `1471` on map `146` requires five items of model `429` and returns models `33` / `2556`, with model-file IDs `111929` / `112034` and item types `3` / `21`. Regenerating `npcs/npcs.json` promoted this offer under the localized `Weaponsmith` NPC entry. The capture file also contains an earlier healthy session from before headers `0x00C3` and `0x00C4` were added; its manifest remains explicitly unverified while the final session is independently verified with zero drops or write failures.

### Merchant, crafter, and skill-trainer catalogs

Current-build disassembly confirms that these inventories are finalized runtime constructor inputs rather than locally selected rows. `VnBuy` at `0x00590EA0` requires service `1`, then consumes item count `create_param + 0x14` and an item-ID buffer at `+0x18`. `VnCraft` at `0x005942D0` uses the same layout for service `3`. `VnLearnSkill` at `0x00599F80` requires service `10`, then consumes count `+0x10` and an eight-byte-per-entry buffer at `+0x14`; the second dword is preserved as `availability_flags_raw` because its complete semantics are not yet established. The three constructor signatures are unique in the current `Gw.exe`, and the injected hooks validate their function prologues before patching.

The local installation contains no `.snapshot` files. An additional exhaustive scan of all 176,541 active `Gw.dat` entries completed with zero failures and found no decompressed entry where the known collector's required model `429` is followed within 256 bytes by both reward models `33` and `2556`. This rejects an obvious co-located literal offer tuple; broad scalar coincidences elsewhere are not evidence of a mapping and do not rule out an unidentified encoded representation.

Verified live session `1784134432671` captured one trainer list (NPC model `3295`, map `194`, 112 skill IDs), two merchant lists (models `3327` / `1470`, maps `194` / `148`, 17 / 7 items), two crafter lists (models `3331` / `3324`, map `194`, 25 / 8 products), and the validated collector offer (model `1471`, map `146`). All six service rows are complete. Thirty capture-health samples report zero lock drops, capacity drops, and write failures.

`extract vendors` now produces `collectors.json`, `merchants.json`, `crafters.json`, `skill_trainers.json`, and an observed-only `coverage.json` keyed by map ID. Regenerating `npcs/npcs.json` from the same verified session yields 98 NPC models, all named in the 11 client languages. Merging the old and new compact item sessions yields 407 item models; every one of the 58 item model IDs referenced by the service catalogs joins to `items/items.json`. All 112 trainer skill IDs now join to the corrected 1,488-ID skill catalog, including `2` (`Resurrection Signet`) and `3` (`Signet of Capture`).

Clean format-3 session `1784142524723` proved that `npc_model_id` is not a
service-NPC identity. In map `146`, collector agents `36` and `63` both used NPC
model `1471`, but spawned at `(7432, 3544)` and `(-6998, -5773)` and exposed
different offers. Aggregating by model had merged them. The same model-level map
join also attributed merchant model `1470` to map `148` although the captured
catalog owner was the distinct map-`146` instance at `(-6847, -5994)`.

Vendor catalogs now key every observed service instance by
`(map_id, npc_model_id, AGENT_SPAWNED.position)`. Transient agent IDs are used
only for the session join and are not published as stable identity. The
corrected clean-session output contains two separate model-`1471` collectors,
one offer each, and coverage retains both positions instead of collapsing the
model ID.

The same session contains one `NPC_UPDATE_PROPERTIES` definition for model
`1471`; its eight-word name EncString resolves through `Gw.dat` text ID `3671`
to the generic archetype label `Weaponsmith` / `Fabricant d'armes`. Treating
that model-level label as the displayed name was disproved by the two French UI
instances: `Brownlow [Collectionneur]` and `Jacob [Collectionneur]`.

Verified session `1784146689955` captured 43
`0x009B AGENT_UPDATE_NPC_NAME` packets. The current client schema descriptors
are `[0x009B, 0x0010, 0x2017]`: header, one 32-bit `agent_id`, and 32 UTF-16
words, for a 72-byte packet. This disproves the outdated `[40]` declaration in
the local GWCA reference. The client sends each name immediately before the
corresponding `AGENT_SPAWNED`, so the consumer retains a pending name across
spawn and clears it on despawn or map change.

The verified capture has 16 world schemas, 114 replicated health rows, and zero
drops or write failures. `vendors/collectors.json` now keeps the two model-1471
instances separate and resolves their display names in all 11 client languages:
French `Brownlow [Collectionneur]` / `Jacob [Collectionneur]`, and English
`Brownlow [Collector]` / `Jacobs [Collector]`.

The same per-instance join now applies to every vendor service. The merged
catalog resolves merchant model `1470` at `(-6847, -5994)` as `Hamish
[Merchant]` / `Hamish [Marchand]` in all 11 languages. The preserved skill
trainer on map `194` came from the older session without `0x009B`, so it
intentionally remains unnamed until that instance is recaptured.

### Session capture stream separation
Format 5 gives every persisted data contract its own session file:
`tyria_items.jsonl`, `tyria_quests.jsonl`, `tyria_npcs.jsonl`,
`tyria_vendor_context.jsonl`, `tyria_collectors.jsonl`,
`tyria_merchants.jsonl`, `tyria_crafters.jsonl`, and
`tyria_skill_trainers.jsonl`. The vendor-context stream contains only shared
merchant-window/owner packets; merchant, crafter, trainer, and collector rows
each remain in their own file.

`world_packet_schema` and `world_packet` cover the packet families used for
quests, NPCs, dialogue, maps, and vendor context. Schemas, hook status, and
health are stored once in `tyria_capture.jsonl`; each data stream contains only
its own records. Extraction requires that sidecar and
`capture_format_version: 5`. There is no fallback for inline metadata or
pre-cutover captures.

The format-5 schema set contains 16 packet families, including
`AGENT_UPDATE_NPC_NAME`. Collector and vendor rows must resolve an agent name
from merged format-5 evidence for the same stable service-instance key;
catalog generation fails rather than emitting a model-level name otherwise.

Every cross-domain data row carries a session-monotonic `capture_seq`.
Multi-file extraction sorts by `(session_id, capture_seq)` before parsing, so
spawn, map load, dialogue, collector, and vendor relations retain their
observed order regardless of command-line file order. The required stream
combinations are: quests = NPC + quest; NPCs with offers = NPC + collector;
vendor catalogs = NPC + vendor context + collector + merchant + crafter +
skill-trainer.

### Complete item and active-quest capture

Format 5 makes full runtime item evidence part of the standard
`tyria_items.jsonl` contract. Each observed ItemGeneral update emits the full
decoded packet fields plus the runtime item's name and `info_string` EncStrings.
The same stream records text-reference IDs whenever the client decoder observes
an EncString; this passive decoder coverage is not assumed exhaustive.
`TYRIA_VERBOSE_JSONL` adds diagnostic packet traces only; it no longer selects
a different item dataset.

The consumer prefers a client-observed decoder sequence when its EncString
matches. Otherwise it uses the bounded AsyncDecode item-subset classifier and
accepts a direct single-reference lookup only when the localized DAT resolver
successfully resolved that captured text ID. It also preserves whether the runtime
`info_string` pointer was available. A null pointer is emitted as
`runtime_description_available: false`, which distinguishes an item with no
runtime description from a missing capture or failed DAT lookup.

Quests already active before injection do not naturally resend their full
description. After installing the quest packet hooks, the sniffer therefore
calls the client's quest-info request for each active quest ID exactly once.
The resulting `QUEST_DESCRIPTION` packets use the normal capture path; no
synthetic quest text is introduced.

Clean session `1784185564652` had zero lock drops, capacity evictions, or write
failures. Replay produced 377 observed item identities named in all 11
languages, 351 with descriptions in all 11 languages, and 26 with an explicitly
unavailable runtime description. Its 11 active quests all resolved names and
descriptions in all 11 languages.

## Exhaustive image export findings (2026-07-15)

The new `extract images` path scanned all 176,541 extractable MFT entries in the current local `Gw.dat`. It found and exported 62,267 distinct image payloads as PNG with zero archive-read or texture-decode failures. The corpus is 3.07 GiB and consists of 51,170 direct ATEX payloads, 1,657 direct ATTX payloads, 223 DDS payloads, and 9,217 ATEX payloads embedded in FFNA resources. A manifest preserves each MFT index, every owning file-ID alias, source stream fields, dimensions, format, inline offset, and PNG path.

The exhaustive pass exposed three previously unsupported first-party texture cases:

* `DXTA` is an alpha-only BC4-style format using eight bytes per 4×4 block. Its interpolated channel is exported as opaque greyscale. This accounts for 12,627 textures.
* ATTX resources may end with non-word container bytes after the encoded image. The decoder now consumes the declared data range and complete tail words rather than requiring the whole MFT payload length to be divisible by four.
* Subcode bit `0x10` on 256×256 DXT3-family textures reserves and unswizzles the two outer block rows and columns through the client subcode-1/subcode-7 path. Three ATTX textures that previously exhausted their planar color tail now decode.

Nineteen DDS resources use a zero FourCC, pixel-format flag `0x80000`, 16 bits per pixel, and masks `0x00ff` / `0xff00`. They are preserved as two-channel bump-map PNGs. The final format distribution is recorded in `images/manifest.json`; generated images remain local and Git-ignored.

### Current regenerated corpus (2026-07-16)

Regeneration against the updated local `Gw.dat` scanned 176,588 extractable MFT
entries and exported 62,314 recognized image payloads, with zero read or decode
failures. The generated corpus contains 1,488 skills with a standard and HD PNG
for every serialized ID, 377 observed item identities, 11 active quests, and 99
NPC models.

The current vendor evidence produces 2 collectors with 2 offers, 4 merchants
with 48 items, no observed crafter, and 1 skill trainer with 112 skills. These
counts describe the present local archive and capture corpus; the dated
176,541-entry / 62,267-image result above remains the evidence from the earlier
archive revision.

## Open questions

1. **Where is the complete static `model_id -> model_file_id` bridge, if one exists?** Runtime pairs and archive alias resolution are confirmed, but no complete offline table or derivation has been identified.
2. **What constitutes complete runtime item coverage?** The union continues to grow across captures, and there is no confirmed finite item-definition inventory against which to prove completeness.
3. **How are standalone ATEX service-item and trophy icons linked safely?** Their textures are present, but a visual label or nearby MFT position is insufficient evidence for a canonical item mapping.
4. **Does the bounded item EncString fallback generalize beyond the captured subset?** Format-5 captures prefer the client's exact requested text IDs, but the full control-word and composition behavior of `AsyncDecodeStr` remains unproven.
5. **How can exhaustive dialog coverage be proven across every map, campaign state, profession, character, and prerequisite combination?** The active-quest request closes the injection-timing gap, but the complete traversal set remains unknown.
6. **Which quests have mutually exclusive or branching stages?** The consumer now preserves every distinct captured objective sequence under `observed_step_sequences` instead of flattening all references into a claimed chronology, but each quest still needs sufficient lifecycle coverage before those observed variants can be treated as exhaustive.
7. **How are reward-item quantities, stat payloads, and mutually exclusive choices represented?** Exact seeded reward-name references can use globally unique models or conservative completion-gated same-session correlation, but the reward EncString itself contains no model ID in the validated sample. Quantities, stats, and choice-group semantics still need a proven first-party structure or controlled completion captures.
8. **What is the authoritative complete outpost traversal set?** `vendors/coverage.json` records observed service NPCs for maps `146`, `148`, and `194`, but the current extractor does not yet reproduce the client's area-info table well enough to enumerate every current outpost and localized map name from first-party files.
