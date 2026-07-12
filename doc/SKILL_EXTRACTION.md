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
| `0x34` | `u8` | Overcast cost. |
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
| `0x00000002` | Touch range. |
| `0x00000004` | Elite. |
| `0x00000008` | Half range. |
| `0x00010000` | Stacking. |
| `0x00020000` | Non-stacking. |
| `0x00080000` | PvE-only. |
| `0x00400000` | PvP-only. |

## 3. Localized strings

The three string fields contain language-independent ids. Each id selects one file-array slot and one record within that file:

```text
file_index   = string_id / 1024
record_index = string_id % 1024
```

For a selected language, `language_file_array[file_index]` supplies the text resource's archive file number. Resolve that file number through the archive hash table, decode the text resource, and select `record_index`. Switching language changes only the language array; the `(file_index, record_index)` pair remains unchanged.

The concise and full description ids are independent references to separately authored text records. Both use the same language lookup and text-record format. Concise/full formatting is therefore a content boundary, not a different binary encoding: decode the selected record and its text markup as specified in [GWDAT_FORMAT.md](GWDAT_FORMAT.md), and do not derive either description from the other.

## 4. Player-skill boundary

The table is broader than the set of skills a player can equip: it is also used
for weapon modifiers and other non-player definitions. A nonzero name id or a
recognized skill type therefore does not establish that a row is a player
skill.

The locally validated PvP and PvE flags are independent properties,
identified by `0x00400000` and `0x00080000` respectively. Neither flag alone
establishes membership in the player-skill corpus. Selecting the confirmed
non-PvP corpus requires the PvP bit to be clear in addition to the content
constraints below.

### Confirmed PvE corpus boundary

For the confirmed PvE player-skill corpus, the intrinsic flag tests above are
combined with these content constraints:

- Ordinary rows use equip/use-family `1`, a Core, Prophecies, Factions,
  Nightfall, or Eye of the North campaign, and profession `1` through `10`.
  Eye of the North also admits profession `0`.
- Title-track codes `5` and `6` classify a row as Factions for this boundary.
- A separate Nightfall group uses equip/use-family `2`, professions `1` through
  `10`, and non-elite rows whose names use text-file slot `26`. The sentinel
  name `REMOVE` and names already present in the ordinary group are excluded.

These constraints define the confirmed corpus; they are not general meanings
of the campaign, profession, title-track, or equip/use-family fields.

## 5. Icons

Offsets `0x8c` and `0x90` reference the standard- and high-resolution icons in the analyzed client table. Resolve each nonzero file number with the archive hash lookup rules in [GWDAT_FORMAT.md](GWDAT_FORMAT.md). The resource may be in either `Gw.dat` or `Gw.snapshot`; a snapshot can contain streamed icons absent from the base archive.

After archive-level decompression, decode the resulting `ATEX` or `ATTX` texture according to [DECOMPRESSION.md](DECOMPRESSION.md). DAT decompression and texture decoding are separate stages.
