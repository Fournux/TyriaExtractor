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

Install the 32-bit MSVC target and build the extractor, injector, and sniffer:

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --workspace --target i686-pc-windows-msvc
```

After the official client has logged in and loaded a map, inject the DLL:

```powershell
.\target\i686-pc-windows-msvc\release\tyria_injector.exe Gw.exe .\target\i686-pc-windows-msvc\release\tyria_sniffer.dll
```

The sniffer appends item observations to `tyria_items.jsonl` beside the DLL.
Use a local `Gw.dat` or `Gw.snapshot` to generate the databases:

```powershell
cargo run --release -- extract skills --snapshot "C:\path\to\Gw.dat"
cargo run --release -- extract items --snapshot "C:\path\to\Gw.dat" --packet-log ".\target\i686-pc-windows-msvc\release\tyria_items.jsonl"
```

Generated files remain local under the Git-ignored `skills/` and `items/`
directories. Item coverage depends on which definitions the official client
received while the sniffer was active.

## Extraction status

**Legend:** ✅ Done · 🚧 In progress · ⬜ Not started

| Dataset | Status | Current result |
|---|:---:|---|
| Player skills | ✅ | Exactly 1,329 skills with structural metadata and names/descriptions in all 11 client languages |
| Skill icons | ✅ | Standard- and high-resolution PNG icons |
| Items | 🚧 | Extraction works for observed `ItemGeneral` records; exhaustive runtime coverage is not yet proven |
| Item icons | 🚧 | Inventory icons are resolved for captured `model_file_id` values |
| Quests | 🚧 | — |
| Monsters / creatures | ⬜ | — |
| NPCs | ⬜ | — |

Items remain in progress because the decoding and export pipeline works, but the
official client must still receive every relevant runtime item definition before
the catalog can be considered exhaustive.

## Data flow

```text
Gw.dat / Gw.snapshot
  -> MFT and hash lookup
  -> archive decompression
  -> executable tables, localized text records, models, and textures

official client + injected sniffer
  -> ItemGeneral runtime pairs and item EncStrings
  -> local Gw.dat text and image resolution
  -> items/items.json and inventory icons
```

The DAT remains the source of names, descriptions, and assets. Runtime capture
provides joins that are not available as a confirmed complete static table,
most notably `model_id -> model_file_id`.

The injected DLL writes `tyria_items.jsonl` by default: a minimal stream of
deduplicated item observations. A real 322-record example is kept in
[`examples/tyria_items.jsonl`](examples/tyria_items.jsonl). Setting
`TYRIA_VERBOSE_JSONL` enables the broader diagnostic stream and writes
`tyria_packets.jsonl`; it is not the normal minimal mode.

## Documentation

- [Guild Wars DAT and snapshot format](doc/GWDAT_FORMAT.md)
- [Archive decompression and texture payloads](doc/DECOMPRESSION.md)
- [Skill extraction](doc/SKILL_EXTRACTION.md)
- [Item identity, localization, and icons](doc/ITEM_EXTRACTION.md)
- [Investigation journal](GWDAT_INVESTIGATION_JOURNAL.md)

## Legal and data ownership

TyriaExtractor is an independent, unofficial project. It is not affiliated
with, endorsed by, or sponsored by ArenaNet or NCSOFT. Guild Wars, its
trademarks, game files, text, artwork, and other game content remain the
property of their respective owners.

The public source tree is intended to contain only the extractor source plus
small capture examples and test fixtures used to document and validate decoder
behavior. `Gw.dat`, `Gw.snapshot`, client executables, complete extracted
catalogs, and extracted icon collections must remain local and must not be
committed or redistributed with the project. Users must provide their own
official local client files; generated `items/` and `skills/` outputs are
ignored by Git.

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
