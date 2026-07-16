# Guild Wars Skill Extraction

Skill metadata, localized text, and icons are separate resources joined by identifiers in the client executable's skill table:

```text
client executable skill row
  ├─ string id -> language file array -> text resource -> text record
  ├─ standard icon file number -> archive hash lookup -> texture resource
  └─ HD icon file number -> archive hash lookup -> texture resource
```

Archive addressing, hash lookup, and text-record decoding are specified in [GWDAT_FORMAT.md](GWDAT_FORMAT.md). Texture wrappers and block decoding are specified in [DECOMPRESSION.md](DECOMPRESSION.md).

## 1. Executable tables

The client executable contains:

- a fixed-width skill metadata table; and
- one array of text-resource file references per language.

These tables provide the joins from a skill row to localized text and icon resources in `Gw.dat` or `Gw.snapshot`. Their executable addresses and the number of rows are build-specific. An address from one client build is not part of the file format and must not be assumed for another build.

## 2. Skill record

Each skill record is `0xa4` bytes. All integer fields are little-endian, and offsets are relative to the start of the record.

The skill id is the zero-based row index. Client lookup bounds-checks the id and
addresses the row as `table_base + skill_id * 0xa4`.

| Offset | Type | Meaning |
|---:|---|---|
| `0x08` | `u32` | Campaign or release-family code. |
| `0x0c` | `u32` | Skill-type code. |
| `0x10` | `u32` | Flags bitfield. |
| `0x28` | `u8` | Profession code. |
| `0x29` | `u8` | Attribute code. |
| `0x2a` | `u16` | Title-track code. |
| `0x2c` | `u32` | Linked or replacement skill id. |
| `0x30` | `u8` | Combo code. |
| `0x31` | `u8` | Target code. |
| `0x33` | `u8` | Equip/use-family code. |
| `0x34` | `u8` | Raw overcast cost; valid only when flag `0x00000001` is set, otherwise ignored. |
| `0x35` | `u8` | Encoded energy cost: `11` means 15 energy, `12` means 25, and other values are literal. |
| `0x36` | `u8` | Health cost. |
| `0x38` | `u32` | Adrenaline cost. |
| `0x3c` | `f32` | Activation time in seconds. |
| `0x40` | `f32` | Aftercast delay in seconds. |
| `0x44` | `u32` | Duration at attribute rank 0. |
| `0x48` | `u32` | Duration at attribute rank 15. |
| `0x4c` | `u32` | Recharge time in seconds. |
| `0x58` | `u32` | Skill-argument flags. |
| `0x5c` | `u32` | Scale value at attribute rank 0. |
| `0x60` | `u32` | Scale value at attribute rank 15. |
| `0x64` | `u32` | Bonus scale at attribute rank 0. |
| `0x68` | `u32` | Bonus scale at attribute rank 15. |
| `0x6c` | `f32` | Area-of-effect range. |
| `0x70` | `f32` | Constant effect value. |
| `0x8c` | `u32` | Standard-resolution icon file number. |
| `0x90` | `u32` | High-resolution icon file number in the analyzed client table. |
| `0x98` | `u32` | Name string id. |
| `0x9c` | `u32` | Concise-description string id. |
| `0xa0` | `u32` | Full-description string id. |

Confirmed flag bits at `0x10` are:

| Mask | Meaning |
|---:|---|
| `0x00000001` | The overcast-cost byte at `0x34` is valid. |
| `0x00000002` | Touch range. |
| `0x00000004` | Elite. |
| `0x00000008` | Half range. |
| `0x00010000` | Stacking. |
| `0x00020000` | Non-stacking. |
| `0x00080000` | PvE-only. |
| `0x00400000` | PvP-only. |
| `0x02000000` | Not playable. |

## 3. Localized strings

The three string fields contain language-independent ids. Each id selects one file-array slot and one record within that file:

```text
file_index   = string_id / 1024
record_index = string_id % 1024
```

For a selected language, `language_file_array[file_index]` supplies the text resource's archive file number. Resolve that file number through the archive hash table, decode the text resource, and select `record_index`. Switching language changes only the language array; the `(file_index, record_index)` pair remains unchanged.

The concise and full description ids are independent references to separately authored text records. Both use the same language lookup and text-record format. Concise/full formatting is therefore a content boundary, not a different binary encoding: decode the selected record and its text markup as specified in [GWDAT_FORMAT.md](GWDAT_FORMAT.md), and do not derive either description from the other.

## 4. Skill template corpus boundary

The table is broader than the template corpus: it is also used for weapon
modifiers and other non-player definitions. A nonzero name id or a recognized
skill type therefore does not establish membership.

The current catalog supports both non-PvP template IDs and current PvP/Codex
variant IDs:

1. Select every row whose equip/use-family is `1` and whose PvP flag
   `0x00400000` is clear. This yields 1,333 IDs. Profession `0` is valid at
   this boundary, including `2`, `3`, and `1814` through `1816`.
2. For each selected row, read the linked skill ID at `0x2c`. Add that target
   only when it carries the PvP flag, uses equip/use-family `0`, and links back
   to the original row through its own `0x2c` field. The reciprocal relation
   selects 155 current variants while rejecting stale or unrelated PvP rows.
3. Do not admit equip/use-family `2` rows. The former Nightfall text-slot
   heuristic incorrectly selected internal IDs `829`, `833`, `861`, `868`,
   and `940`.

Rows `3418` through `3421` satisfy the family-`1` boundary but carry the
client's not-playable flag `0x02000000`, profession `0`, and the localized
name `...`. They remain distinct special rows; consumers can exclude them
from player-facing choices through `flags.playable`.

Title-track codes `5` and `6` are reported under Factions. Selection and
serialization are keyed by the table ID, never by localized name: the output
contains 1,488 unique IDs and retains separate rows when names coincide.

## 5. Icons

Offsets `0x8c` and `0x90` reference the standard- and high-resolution icons in the analyzed client table. Resolve each nonzero file number with the archive hash lookup rules in [GWDAT_FORMAT.md](GWDAT_FORMAT.md). The resource may be in either `Gw.dat` or `Gw.snapshot`; a snapshot can contain streamed icons absent from the base archive.

After archive-level decompression, decode the resulting `ATEX` or `ATTX` texture according to [DECOMPRESSION.md](DECOMPRESSION.md). DAT decompression and texture decoding are separate stages.

Associate both PNG variants with the table skill ID. A complete catalog requires
the standard icon to resolve for every included row; the current 1,488-row
corpus also resolves all 1,488 high-resolution file references. The table-defined
file references eliminate any need for a per-skill icon mapping.
