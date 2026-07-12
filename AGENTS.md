# TyriaExtractor / Guild Wars Database Extractor

TyriaExtractor reverse-engineers the official Guild Wars client data pipeline to build a structured, reproducible game database from local first-party files.

Technical findings about archive formats, decompression, resource indexing, and string resolution belong in [`GWDAT_INVESTIGATION_JOURNAL.md`](GWDAT_INVESTIGATION_JOURNAL.md) and [`doc/`](doc/README.md).

## 1. Mission

Extract and preserve as much game data and as many resources as possible from the local `Gw.dat` archive and local `.snapshot` files: skills, items, localized strings, images, models, metadata, and their relationships.

The intended result is an offline extractor that reproduces the official client's behavior for locating, decoding, decompressing, and resolving resources from `Gw.dat`. The current skills output is one validation milestone, not the limit of the project.

## 2. Source Policy and Precedence

Use sources in this order:

1. **`Gw.dat` and `.snapshot` files are primary.** Determine whether each value or resource is present there before introducing a runtime dependency.
2. **The official client is the behavioral reference.** Its executable and runtime may be inspected, debugged, disassembled, or minimally instrumented to understand the exact indexing, decoding, decompression, and string-resolution behavior that the extractor must reproduce.
3. **Official client packets are a narrow fallback.** Sniff packets only for fields or mappings shown to be absent from `Gw.dat` and `.snapshot`, not merely because their archive representation is still unknown. Capture only the required packet family and fields.
4. **External data is forbidden.** Do not fetch, scrape, or import game data from websites, public APIs, wikis, or third-party repositories.

Packet captures may enrich data that genuinely does not exist in the primary files. They must not replace archive investigation or become a shortcut around reproducing official client behavior.

## 3. Runtime Instrumentation Rules

* Keep hooks and packet logging minimal: observe only the code path, packet type, or field needed for the current unresolved question.
* Prefer reproducing a client algorithm in the offline extractor over repeatedly calling the running client.
* Treat runtime observations as reverse-engineering evidence. Document the corresponding client behavior, file structure, or proven data gap.
* Do not retain unrelated traffic or add speculative hooks.

## 4. Engineering Constraints

* Keep archive I/O, decompression, indexing, string resolution, resource-specific parsing, and output generation separated by responsibility.
* Extend existing patterns before adding abstractions. Support new resource types when their extraction is implemented, not through speculative scaffolding.
* Preserve unknown raw values when they may be needed for later interpretation; do not silently invent semantics.
* Use robust bounds checks and explicit errors for malformed offsets, blocks, indexes, and compressed streams.
* Optimize byte access and allocation only where extraction volume or measurements justify it.
* Code under `references/` is outdated. Use it only as conceptual evidence; never copy or directly translate it without validating the behavior against current first-party files and the official client.

## 5. Extraction Coverage

For every supported resource, extract all available embedded identifiers, localized strings, file/model mappings, gameplay metadata, flags, and relationships. Structured outputs must be deterministic and retain enough stable identifiers to join related resources.

For skills this includes, at minimum:

* Skill name and ID
* Energy cost
* Activation time
* Recharge duration
* Elite status
* Campaign origin

## 6. Current Skills Validation Milestone

The skills pipeline is valid only when its campaign distribution matches:

| Campaign | Non-Elite | Elite | Total |
| --- | ---: | ---: | ---: |
| Core | 212 | 40 | 252 |
| Prophecies | 154 | 60 | 214 |
| Factions | 260 | 90 | 350 |
| Nightfall | 253 | 110 | 363 |
| Eye of the North | 147 | 3 | 150 |
| **Grand Total** | **1026** | **303** | **1329** |

## 7. Workflow

1. Inventory what is present in `Gw.dat` and `.snapshot`; distinguish missing data from data whose encoding is not yet understood.
2. Trace the official client behavior needed to locate and decode those resources.
3. Add the smallest possible runtime hook or packet capture only when primary-file analysis cannot answer the question.
4. Implement the discovered behavior in the offline Rust extraction pipeline.
5. Record new format and algorithm findings in the investigation journal and stable specifications in `doc/`.
6. Validate outputs with exact counts, invariants, representative resources, and client behavior where applicable.
