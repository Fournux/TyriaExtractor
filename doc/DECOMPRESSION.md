# Guild Wars DAT Decompression and Texture Payloads

This document defines DAT-layer compression and the ATEX, ATTX, and DDS texture payload formats. Archive addressing and MFT fields are defined in [the container format](GWDAT_FORMAT.md).

## 1. DAT compression codes

The MFT entry's `compression` field determines the first payload-decoding stage.

| Code | Meaning |
|---:|---|
| `0` | The stored bytes are the resource payload. |
| `8` | The stored bytes use the Guild Wars DAT compression format. |

Texture compression, when present, is a second stage inside the decoded resource payload.

## 2. Guild Wars DAT compression

Compression code `8` is a custom Huffman Deflate-family LZ77 format. It is not an RFC 1951, zlib, or gzip stream.

The compressed payload has these framing rules:

1. Its size is a multiple of four bytes and is at least 12 bytes.
2. It is interpreted as little-endian `u32` words.
3. The first two words seed the word-oriented bitstream state.
4. The final word is the exact decoded size in bytes.
5. Decoding ends when the declared number of output bytes has been produced.

Each block carries Huffman symbol counts and code-length runs. Format-specific fixed tables decode those runs into the block's literal/length and distance trees. Literal symbols emit one byte. Length/distance symbols copy bytes from already-decoded output; overlapping source and destination ranges use normal LZ77 forward-copy semantics. A distance cannot precede the beginning of the decoded output, and a copy cannot exceed the declared output size.

## 3. ATEX and ATTX

ATEX and ATTX wrap DXT/BC block data. The payload is 32-bit aligned and begins with:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | 4 bytes | Container magic `ATEX` or `ATTX`. |
| `0x04` | 4 bytes | Texture FourCC. |
| `0x08` | `u16` | Width. |
| `0x0a` | `u16` | Height. |
| `0x0c` | `u32` | Data-range size. |
| `0x10` | `u32` | ATEX subcode bitfield. |
| `0x14` | bytes | ATEX bitstream and planar tail. |

Width and height are nonzero. The encoded data range ends at:

```text
0x0c + data_range_size
```

The supported FourCC families and their final block decoders are:

| ATEX FourCC | Block decoder | Bytes per 4x4 block |
|---|---|---:|
| `DXT1` | DXT1 / BC1 | 8 |
| `DXT2`, `DXT3`, `DXTN` | DXT3 / BC2 | 16 |
| `DXT4`, `DXT5`, `DXTA`, `DXTL` | DXT5 / BC3 | 16 |

`DXTL` uses the DXT5-family path with premultiplied color channels.

## 4. Planar ATEX layout

ATEX stores undecoded block components in planes rather than complete DXT blocks:

```text
alpha words for undecoded blocks
color-endpoint words for undecoded blocks
color-index words for undecoded blocks
```

The planes are interleaved into block order before DXT/BC decoding. For alpha-bearing formats, each completed 16-byte block contains two alpha words, one color-endpoint word, and one color-index word. A DXT1 block contains one color-endpoint word and one color-index word.

A subcode bitfield of `0` means that no subcode fills blocks. The remaining payload is still planar and is interleaved by the same rules.

## 5. ATEX subcodes

The field at `0x10` is a bitfield:

| Bit | Format family | Effect |
|---:|---|---|
| `0x1` | `DXT1` | Subcode 2 fills constant DXT1 blocks. |
| `0x2` | `DXT2`, `DXT3`, `DXTN` | Subcode 3 fills DXT3-family alpha blocks. |
| `0x4` | `DXT4`, `DXT5`, `DXTA`, `DXTL` | Subcode 4 fills DXT5-family alpha blocks. |
| `0x8` | All supported families | Subcode 5 fills color endpoint/index blocks. |

The subcodes mark the block components they fill. Planar tail words fill the unmarked alpha and color components. The completed interleaved blocks are then decoded with the family mapping above.

## 6. DDS

A direct DDS payload begins with `DDS ` and uses the standard 124-byte DDS header. Pixel data begins at offset `128`.

The required header fields are:

| Offset | Type | Meaning |
|---:|---|---|
| `0x00` | 4 bytes | Magic `DDS `. |
| `0x04` | `u32` | Header size: `124`. |
| `0x0c` | `u32` | Height; nonzero. |
| `0x10` | `u32` | Width; nonzero. |
| `0x50` | `u32` | Pixel-format flags. |
| `0x54` | 4 bytes | Pixel-format FourCC. |
| `0x58` | `u32` | RGB bit count. |
| `0x5c` | `u32` | Red mask. |
| `0x60` | `u32` | Green mask. |
| `0x64` | `u32` | Blue mask. |
| `0x68` | `u32` | Alpha mask. |

FourCC values `DXT1`, `DXT3`, and `DXT5` use the corresponding block decoders. A zero FourCC with the RGB pixel-format flag (`0x40`) uses the declared channel masks; the confirmed uncompressed bit depths are 16 through 32 bits.
