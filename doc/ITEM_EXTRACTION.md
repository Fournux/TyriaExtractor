# Guild Wars Item Extraction

This reference describes which item identities, localized strings, and inventory
icons can be recovered from DAT resources when runtime item evidence is
available. Archive structure, linked-file traversal, hashing, and text records
are defined in [GWDAT_FORMAT.md](GWDAT_FORMAT.md). DAT and texture decoding are
defined in [DECOMPRESSION.md](DECOMPRESSION.md).

## Runtime identity

Three identifiers have different roles:

| Identifier | Meaning |
|---|---|
| `item_id` | Runtime identity of one item instance. It can be reused and is not a stable item definition or DAT resource key. |
| `model_id` | Gameplay identity supplied for the runtime item. It is not a DAT file number. |
| `model_file_id` | DAT resource key for the item's visual resource. It is the low 31 bits of the packet's raw model-file field. |

The decoded server-to-client messages `0x0161` (item general information) and
`0x0162` (item general information with a reused `item_id`) use the same item
layout:

| Offset | Decoded field |
|---:|---|
| `+0x04` | `item_id` |
| `+0x08` | raw model-file field; `model_file_id = raw & 0x7fffffff` |
| `+0x0c` | item type |
| `+0x10` | unknown scalar |
| `+0x14` | extra id |
| `+0x18` | materials |
| `+0x1c` | unknown scalar |
| `+0x20` | interaction flags |
| `+0x24` | price |
| `+0x28` | `model_id` |
| `+0x2c` | quantity |
| `+0x30` | name EncString, a 64-word UTF-16 buffer |
| `+0xb0` | modifier count, at most 64 |
| `+0xb4` | modifier words |

The decoded message size is `0xb4 + modifier_count * 4` bytes, up to `0x1b4`
bytes. After the message is applied, the runtime item retains `model_file_id`
at `+0x1c` and `model_id` at `+0x2c`; these runtime-structure offsets are not
packet offsets.

No complete static `model_id -> model_file_id` table is confirmed. DAT resources
can supply the bytes addressed by a known `model_file_id`, but `model_id` alone
does not identify a DAT entry, an icon, a name, or a description. A complete
semantic catalog consequently requires runtime pairs or another independently
verified mapping source.

## Name EncString

The name at ItemGeneral `+0x30` is language-independent. It is an encoded
instruction stream, not rendered text and not simply a `model_id`-derived string
id. Its words are little-endian `u16` values terminated by a zero word.

The integer encoding used by the observed item EncStrings is:

```text
value = 0
repeat:
    word = next u16
    digit = (word & 0x7fff) - 0x0100
    value = value + digit
    if word & 0x8000:
        value = value * 0x7f00
        continue
    emit value
```

A zero word terminates the EncString. Words below `0x0100` can be literal or
structural separators and are not integer starts.

Decoded integers have distinct roles:

- a **text id** selects a localized DAT text record;
- a **template or control value** determines composition and is not itself
  necessarily emitted as text;
- a **seed value** supplies the seed required by a compact encrypted text
  record and is not a text id.

Integer decoding alone does not classify those roles. The verified classifier
is bounded to the observed item-name and item-description subset. It must not be
treated as a complete specification of the client's general `AsyncDecodeStr`
language.

### Verified item-subset text-reference classifier

For item name and description EncStrings in the verified subset, classify each
decoded integer together with the word range that encoded it:

```text
if value == 2:
    emit it only for the literal sequence 0x0002, 0x0102, 0x0002
else if value > 0xffffffff:
    keep it as a seed; do not emit it as a text id
else if the integer occupies one encoded word:
    emit it when the raw word is >= 0x08d4 and is not a control word
else:
    emit it unless it is one of 37404, 56261, or 69415
```

The single-word controls excluded by that rule are:

```text
0x0a30 0x0a31 0x0a33 0x0a34 0x0a35 0x0a3a 0x0a3b 0x0a3c
0x0a3d 0x0a3e 0x0a3f 0x0a40 0x0a42 0x0a43 0x0a7e 0x0a80
0x0a81 0x0a84 0x0a85 0x0a86 0x0a87 0x0a88 0x0a89 0x0a8a
0x0a8b 0x0aa4 0x0aa7 0x0aa8 0x0aa9 0x0aac 0x0aaf 0x0abb
0x0abc
```

This classifier reproduces the client-emitted text-reference sequences for the
item EncStrings used to establish it. Its explicit scope is why it is not a
general `AsyncDecodeStr` grammar.
A runtime observation at the client decoder boundary can record the exact text
reference IDs requested for each item EncString. Prefer that client-emitted
sequence when available; the bounded classifier above remains a fallback for
EncStrings not observed at the decoder boundary.
A direct single-reference result from that fallback is accepted only when the
localized DAT resolver successfully resolved the captured text ID; an arbitrary
same-numbered local record is not sufficient provenance.

For a name in that verified item subset, the first emitted text reference
selects the base name or name template. Later emitted text references supply
localized string arguments for composed names. A value larger than `u32`, when
paired with a text id in the EncString, is a compact-record seed rather than a
lookup id. Control values are not looked up as text. If template arguments
remain unresolved, the rendered name is not established and must not be
guessed.

Each emitted text id uses the common localized-resource split:

```text
file_index   = text_id / 1024
record_index = text_id % 1024
resource_id  = language_file_array[file_index]
```

`resource_id` resolves through the DAT hash table; `record_index` selects the
record in that language's text resource. Switching languages changes only the
file array, not the EncString or text id.

To resolve a name, decode the item's text references, select the corresponding
text file from the requested language's file table, and decode the referenced
record. Repeating that lookup with each language table yields multilingual
names from the same language-independent EncString. The text-file indexing and
record formats are described in
[GWDAT_FORMAT.md](GWDAT_FORMAT.md#7-text-records).

## Descriptions

ItemGeneral does not contain the description EncString. The verified source is
the runtime item's `info_string` after the item update has populated that
object. In the analyzed 32-bit runtime item layout, the `info_string` pointer is
at runtime-item offset `+0x30`. This is a different structure from the
ItemGeneral message, whose own `+0x30` field is the name EncString.

When the description EncString is available, its text references resolve
through the same per-language DAT text files as the name, so one runtime
description EncString can yield descriptions in every available language.

DAT text records may be templates rather than final rendered strings. Static
text substitutions referenced by the EncString can be resolved, but dynamic
numeric placeholders require the corresponding runtime values and formatting
semantics. If those values are absent, DAT plus the EncString recovers only the
localized template; replacing an unresolved numeric placeholder with a guessed
number would be incorrect.

A runtime observation must distinguish a null `info_string` pointer from an
available EncString. A captured null pointer proves that the observed runtime
item exposed no description; it is not a failed DAT lookup. A localized text
record alone still does not establish that it belongs to an item.

## Inventory icons

The verified model-backed icon path is:

```text
masked model_file_id
  -> DAT hash lookup
  -> base MFT entry
  -> linked content entry whose stream selector is 1
  -> ATEX, ATTX, DDS, or an inline ATEX/ATTX texture
  -> decoded icon
```

Use the linked entry's `content == 1` value as the stream selector. The archive
and linked-entry rules are specified in
[GWDAT_FORMAT.md](GWDAT_FORMAT.md#4-linked-resource-streams); payload and texture
decoding are specified in [DECOMPRESSION.md](DECOMPRESSION.md).

The DAT hash table can contain several file-number aliases for the same base MFT
entry. A runtime `model_file_id` may be any one of those aliases. Preserve all
aliases when constructing a reverse index; equivalent aliases identify the
same resource bytes but remain distinct valid lookup keys.

Not every item icon can be assigned by the model-backed path alone:

- **Direct textures:** a direct ATEX, ATTX, or DDS file is an item icon only
  when a verified item structure identifies that field as an inventory-texture
  file id. Texture type or visual appearance is insufficient because the same
  resource families also contain non-item images.
- **Composite items:** the runtime source `model_file_id` can require a
  secondary composite-record selection before the displayed piece's file id is
  known. The selected id follows the same hash and stream-1 path, but no generic
  complete source-id-to-selected-id join is confirmed.

Therefore the safe joins are:

```text
runtime item -> masked model_file_id -> hash alias -> linked stream 1 icon
runtime item -> verified direct inventory-texture field -> texture
```

The following are not safe joins:

```text
model_id -> DAT hash lookup
text id -> DAT hash lookup
unclassified texture -> item by appearance or archive proximity
composite source id -> guessed component id
```
