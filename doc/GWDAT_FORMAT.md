# Guild Wars DAT / Snapshot Container Format

`Gw.dat` and `Gw.snapshot` use the same little-endian archive container. Resources are addressed through a Master File Table (MFT) and a file-number hash table. Archive decoding ends at the decompressed MFT payload; the payload's own format is a separate layer.

## 1. Root header

The archive begins with a 32-byte header.

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | 4 bytes | Magic `33 41 4e 1a` (`3AN\x1a`). |
| `0x04` | `u32` | Header size: `32`. |
| `0x08` | `u32` | Sector size: `512`. |
| `0x0c` | `u32` | Opaque value. |
| `0x10` | `u64` | Absolute byte offset of the MFT. |
| `0x18` | `u32` | MFT region size in bytes. |
| `0x1c` | `u32` | Opaque flags or reserved value. |

The MFT range is `[mft_offset, mft_offset + mft_size)`. The range lies within the archive, is at least 24 bytes long, and has a size divisible by 24.

## 2. Master File Table

The MFT region consists of 24-byte rows. Row `0` is the MFT header. The remaining rows are MFT entries with indices `1` through `entry_count - 1`.

### MFT header

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | 4 bytes | Magic `4d 66 74 1a` (`Mft\x1a`). |
| `0x04` | `u32` | Opaque value. |
| `0x08` | `u32` | Opaque value. |
| `0x0c` | `u32` | Row count, including the header row. |
| `0x10` | `u32` | Opaque value. |
| `0x14` | `u32` | Opaque value. |

The row count satisfies:

```text
entry_count * 24 <= mft_size
```

### MFT entry

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u64` | Absolute payload offset. |
| `0x08` | `u32` | Stored payload size. |
| `0x0c` | `u16` | DAT compression code. |
| `0x0e` | `u8` | Content or linked-stream selector. |
| `0x0f` | `u8` | Content-type value. |
| `0x10` | `u32` | Linked/next MFT index; `0` terminates the chain. |
| `0x14` | `u32` | Opaque value. |

An entry with `size == 0` or `content == 0` has no payload for a normal resource read. The defined DAT compression codes are:

| Code | Stored payload |
|---:|---|
| `0` | Uncompressed. |
| `8` | Guild Wars DAT-compressed; see [DAT decompression](DECOMPRESSION.md). |

The payload range is `[offset, offset + size)` and lies within the archive.

## 3. File-number hash table

MFT index `2`, the second serialized MFT entry, locates the file-number hash table. Its uncompressed payload is a sequence of 8-byte rows:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u32` | File number. |
| `0x04` | `u32` | MFT index. |

The payload size is divisible by 8. Resolution is:

```text
file number -> hash row -> MFT index -> MFT entry
```

A reference may use either state of the high bit. Lookup keys are tried in this order:

```text
file_id
file_id | 0x80000000
file_id & 0x7fffffff
```

Multiple file numbers may map to the same MFT index.

## 4. Linked resource streams

A hash row may resolve to the first entry of a linked MFT chain. At each entry, the field at `0x10` supplies the next MFT index. A zero next index ends the chain.

The `content` byte selects a stream:

```text
mft_index = hash[file_id]
while mft_index != 0:
    entry = mft[mft_index]
    if entry.content == stream_id:
        return entry
    mft_index = entry.next_mft_index
```

The verified linked-stream selection rule uses `content`. `content_type` is a separate resource-family field and is not sufficient to establish stream identity.

## 5. Payload decoding

For the selected entry:

1. Validate the stored payload range.
2. Read exactly `size` bytes at `offset`.
3. For compression code `0`, those bytes are the resource payload.
4. For compression code `8`, decode them according to [DAT decompression](DECOMPRESSION.md).
5. Identify the resource format from the decoded payload.

The MFT content-type byte does not replace payload-format identification.

### Confirmed payload families

The decoded archive contains several independent resource formats. Their
payload signatures or structures, rather than the MFT `content_type`, select
the next parser:

| Signature or shape | Resource family |
|---|---|
| `MZ` | PE32 client executable image containing static client tables. |
| `;===`, `;***`, or the record stream described below | Localized text resource. |
| `ffna` | ArenaNet model, map, or multi-part resource container. |
| `ATEX` / `ATTX` | Guild Wars texture wrapper. |
| `DDS ` | DirectDraw Surface texture. |
| `AMAT` | Material resource. |
| `AMP`, `ID3`, MPEG frame sync `ff fa` / `ff fb` | Audio resource. |

Other payloads remain resource-specific binary data until their own structure
is established.

## 6. Encoded file references

Some payloads encode a file reference as two little-endian `u16` values. The file number is computed with arithmetic modulo \(2^{32}\):

```text
file_id = u32(id0) - 0x00ff00ff + u32(id1) * 0x0000ff00
```

The resulting value is resolved through the file-number hash table, including the high-bit variants above.

## 7. Text records

A text resource contains a sequential record stream, which can begin after a binary prefix. Every record begins with a six-byte header, and its `size` includes that header. All multi-byte values are little-endian.

The record index is the zero-based position in the stream. It advances for every record, including records that do not contain text. Text lookup keys use this record index rather than a text-only ordinal.

A global string id selects a language-array slot and a record within the selected text resource:

```text
file_index   = string_id / 1024
record_index = string_id % 1024
```

The language array supplies the archive file number for `file_index`.

### Plain UTF-16LE record

The plain-text tuple is exactly:

```text
type = 0x10, subtype = 0, flags = 0
```

Its layout is:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u16` | Record size, including the six-byte header. |
| `0x02` | `u16` | Flags: `0`. |
| `0x04` | `u8` | Type: `0x10`. |
| `0x05` | `u8` | Subtype: `0`. |
| `0x06` | UTF-16LE | Text payload through the end of the record. |

Text markup remains part of the decoded UTF-16 text.

### Compact record

Compact decoding reinterprets the same six-byte prefix:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | `u16` | Record size, including the six-byte header. |
| `0x02` | `u16` | Base UTF-16 code unit. |
| `0x04` | `u8` | Symbol width in bits, from `1` through `16`. |
| `0x05` | `u8` | Subtype: `0` in the verified compact text records. |
| `0x06` | bytes | Packed symbol payload through the end of the record. |

Compact decoding requires the seed associated with the record index. Seed `0` is the boundary between the two payload paths:

- `seed == 0`: unpack the payload directly;
- `seed != 0`: apply the record's RC4 seed transform, then unpack the result.

Symbols are packed least-significant bit first at the declared width. Symbol `0` terminates the string. Symbols `1` through `31` map through this UTF-16 table:

```text
0000 0030 0031 0032 0033 0034 0035 0036
0073 0074 0072 006e 0075 006d 0028 0029
005b 005d 003c 003e 0025 0023 002f 003a
002d 0027 0022 0020 002c 002e 0021 000a
```

For symbols `>= 0x20`, the UTF-16 code unit is:

```text
code_unit = base + symbol - 0x20
```

The code-unit arithmetic wraps modulo \(2^{16}\).
