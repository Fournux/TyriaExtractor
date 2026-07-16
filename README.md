<div align="center">
  <img src="./logo.png" alt="TyriaExtractor" width="600" style="margin-bottom:1px solid red;">
  <p><strong>A Rust toolkit for building a complete Guild Wars game database.</strong></p>
  <p>
    <img alt="Rust 1.97.0" src="https://img.shields.io/badge/Rust-1.97.0-000000?logo=rust&amp;logoColor=white">
    <img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue">
  </p>
</div>

## Vision

TyriaExtractor is a collection of extraction and reverse-engineering tools for
turning Guild Wars client data into a comprehensive, machine-readable database.
The goal extends beyond skills and items to monsters, NPCs, quests, and other
game resources useful to players, community applications, build planners,
archival projects, and technical research.

Game data comes from the user's official local client:

- static resources, executable tables, localized text, models, and textures from
  `Gw.dat` or `Gw.snapshot`;
- relationships supplied only at runtime, such as the
  `model_id -> model_file_id` item mapping, from official server-to-client
  messages observed by the injected sniffer;
- no wiki, website, third-party database, or remote API is used as a data source.

## Quick start (Windows)

Install the 32-bit MSVC target, then build the native extractor and the 32-bit
injector/sniffer pair:

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release -p tyria-extractor-rs
cargo build --release --target i686-pc-windows-msvc -p tyria_injector -p tyria_sniffer
```

After the official client has logged in and loaded a map, inject the DLL:

```powershell
.\target\i686-pc-windows-msvc\release\tyria_injector.exe Gw.exe .\target\i686-pc-windows-msvc\release\tyria_sniffer.dll
```

The sniffer writes immutable, session-scoped evidence under
`captures/<session-id>/`, with one JSONL per record contract:
`tyria_items.jsonl`, `tyria_quests.jsonl`, `tyria_npcs.jsonl`,
`tyria_vendor_context.jsonl`, `tyria_collectors.jsonl`,
`tyria_merchants.jsonl`, `tyria_crafters.jsonl`, and
`tyria_skill_trainers.jsonl`, plus the shared metadata sidecar
`tyria_capture.jsonl`. Repeat `--packet-log` to merge the streams and capture
sessions required by an extractor:

```powershell
cargo run --release -- extract skills --snapshot "C:\path\to\Gw.dat"
cargo run --release -- extract images --snapshot "C:\path\to\Gw.dat"
cargo run --release -- extract items --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_items.jsonl"
cargo run --release -- extract quests --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_npcs.jsonl" --packet-log ".\captures\<session-id>\tyria_quests.jsonl" --item-log ".\captures\<session-id>\tyria_items.jsonl"
cargo run --release -- extract npcs --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_npcs.jsonl" --packet-log ".\captures\<session-id>\tyria_collectors.jsonl"
cargo run --release -- extract vendors `
  --snapshot "C:\path\to\Gw.dat" `
  --packet-log ".\captures\<session-id>\tyria_npcs.jsonl" `
  --packet-log ".\captures\<session-id>\tyria_vendor_context.jsonl" `
  --packet-log ".\captures\<session-id>\tyria_collectors.jsonl" `
  --packet-log ".\captures\<session-id>\tyria_merchants.jsonl" `
  --packet-log ".\captures\<session-id>\tyria_crafters.jsonl" `
  --packet-log ".\captures\<session-id>\tyria_skill_trainers.jsonl"
```

For recurring work, the Bun scripts select the newest numeric
`captures/<session-id>/` directory automatically and use
`C:\Program Files (x86)\Guild Wars\Gw.dat` by default:

```powershell
bun run build:capture
bun run inject
bun run regen
bun run extract:vendors
```

Set `GW_DAT` to use another archive, or `CAPTURE_DIR` to select a capture path
or session ID instead of the newest one. Run `bun run` to list every target.

All extraction commands write below `output/` by default. `--out-dir <PATH>`
overrides that common parent. Generated output remains local and Git-ignored:

```text
output/
  items/          items.json, capture.json, model_file/*.png
  skills/         skills.json, model_file/*.png, model_file_hd/*.png
  images/         manifest.json, png/*.png
  quests/         quests.json, capture.json
  npcs/           npcs.json, capture.json
  vendors/
    collectors/   collectors.json
    merchants/    merchants.json
    crafters/     crafters.json
    skill_trainers/ skill_trainers.json
    coverage.json
    capture.json
```

Capture-backed extraction requires format version `5` and rejects missing health
metadata, packet loss, write failures, incompatible world-packet schemas,
duplicate or missing `capture_seq` values, and every pre-cutover JSONL.

## Extraction status

**Legend:** ✅ Done · 🚧 In progress · ⬜ Not started

| Dataset | Status | Current result |
|---|:---:|---|
| Skill template corpus | ✅ | 1,488 distinct IDs: 1,333 non-PvP skills/special rows plus 155 linked PvP variants, with metadata and text in all 11 client languages |
| Skill icons | ✅ | Standard- and high-resolution PNG icons |
| DAT textures | ✅ | 62,314 web-ready PNGs from every decodable ATEX, ATTX, DDS, and inline FFNA texture in the current archive |
| Items | 🚧 | 377 observed identities in the current capture; all names and 351 descriptions resolve in all 11 client languages |
| Item icons | 🚧 | Verified linked stream-1 PNGs for 309 of 333 observed `model_file_id` values; unresolved mappings are not guessed |
| Quests | 🚧 | 11 observed active quests with localized fields, objective variants, rewards, and NPC role/map evidence |
| Monsters / creatures | ⬜ | — |
| NPCs | 🚧 | 99 observed models across maps 146, 148, and 194, with runtime metadata and names in all 11 client languages |
| Vendors and skill trainers | 🚧 | 2 collectors / 2 offers, 4 merchants / 48 items, 0 crafters, and 1 trainer / 112 skills in the current capture corpus |

Items remain in progress because the decoding and export pipeline works, but the
official client must still receive every relevant runtime item definition before
the catalog can be considered exhaustive.

Quests remain in progress because no complete static quest table has been
confirmed. The consumer preserves relations observed in official-client packet
captures, but exhaustive quest, branch, dialogue, and prerequisite coverage is
not yet proven.

NPC extraction consumes the dedicated NPC properties, spawn, and map-context
stream. Coverage is observational: the present corpus spans maps 146, 148, and
194 and is not an exhaustive NPC list.

Vendor extraction emits stable service-instance/item/skill joins in
`output/vendors/{collectors,merchants,crafters,skill_trainers}/`, with one
same-named JSON file per directory. A service instance is identified by
`(map_id, npc_model_id, position)` because multiple NPCs can share one model.
`output/vendors/coverage.json` lists those observed instances by map; it is a
traversal progress ledger, not a claim that every outpost has been covered.
Service entries use the per-agent `AGENT_UPDATE_NPC_NAME` EncString when their
capture observed it and resolve it from `Gw.dat` in all 11 client languages.

## Data flow

```text
Gw.dat / Gw.snapshot
  -> MFT and hash lookup
  -> archive decompression
  -> executable tables, localized text records, models, and textures

official client + injected sniffer
  -> tyria_items.jsonl: ItemGeneral runtime pairs and item EncStrings
  -> tyria_quests.jsonl: quest packets and dialogue relations
  -> tyria_npcs.jsonl: NPC properties, per-agent names, spawns, model/file relations, and maps
  -> tyria_vendor_context.jsonl: merchant-window and owner packets
  -> tyria_collectors.jsonl: collector exchanges
  -> tyria_merchants.jsonl: merchant item lists
  -> tyria_crafters.jsonl: crafter product lists
  -> tyria_skill_trainers.jsonl: skill-trainer lists
  -> local Gw.dat text and image resolution
  -> output/{items, skills, images, quests, npcs, vendors}/
```

The DAT remains the source of localized text records and assets. Runtime
capture provides joins that are not available as a confirmed complete static
table, most notably `model_id -> model_file_id`, plus each runtime item's
name and `info_string` EncStrings. Active quests are queried once after hook
installation so their `QUEST_DESCRIPTION` records are captured even when the
client already knew the quests before injection.

Each data JSONL contains only its own packet or runtime-hook records. The shared
`world_packet_schema`, hook-status, loss, and write-counter metadata lives once
in the sibling `tyria_capture.jsonl` sidecar, which extraction discovers
automatically. A monotonic `capture_seq` preserves cross-file event order when
consumers merge the required streams.
Only capture format 5 with its `tyria_capture.jsonl` sidecar is accepted. The
standard `tyria_items.jsonl` always contains complete ItemGeneral records,
decoder observations, and runtime item strings. `TYRIA_VERBOSE_JSONL` adds
diagnostic packet traces without changing that extraction contract.

## Documentation

- [Guild Wars DAT and snapshot format](doc/GWDAT_FORMAT.md)
- [Archive decompression and texture payloads](doc/DECOMPRESSION.md)
- [Skill extraction](doc/SKILL_EXTRACTION.md)
- [Item identity, localization, and icons](doc/ITEM_EXTRACTION.md)
- [NPC identity, localization, and vendor services](doc/NPC_AND_VENDOR_EXTRACTION.md)
- [Quest packets, localization, dialogue roles, and rewards](doc/QUEST_EXTRACTION.md)
- [Investigation journal](GWDAT_INVESTIGATION_JOURNAL.md)

## Legal and data ownership

TyriaExtractor is an independent, unofficial project. It is not affiliated
with, endorsed by, or sponsored by ArenaNet or NCSOFT. Guild Wars, its
trademarks, game files, text, artwork, and other game content remain the
property of their respective owners.

The public source tree is intended to contain only the extractor source plus
small synthetic fixtures used to document and validate decoder behavior.
`Gw.dat`, `Gw.snapshot`, client executables, complete runtime captures, extracted
catalogs, and extracted image collections must remain local and must not be
committed or redistributed with the project. Users must provide their own
official local client files; `captures/` and all generated `output/` artifacts
are ignored by Git.

The MIT License applies only to original TyriaExtractor software and project
material. It does not grant rights to Guild Wars data or assets extracted with
the tools. Runtime instrumentation operates on the user's local client process;
users are responsible for complying with applicable terms and laws.

## References

The investigation and tooling were informed by these community projects:

- [GWToolbox++](https://github.com/gwdevhub/GWToolboxpp) — Guild Wars client
  structures, GWCA interfaces, packet behavior, and mature runtime tooling.
- [Py4GW](https://github.com/apoguita/Py4GW) — Python client integration and
  packet-sniffing patterns used during early capture experiments.

They are used as conceptual references and cross-checks. TyriaExtractor does
not treat their legacy implementations as authoritative format definitions;
exported game data still comes from the user's official local client.

## License

Original TyriaExtractor software is licensed under the [MIT License](LICENSE).
