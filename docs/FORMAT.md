# The `.znimok` document format, version 1.0

A `.znimok` file is one screenshot with its annotations: the untouched original picture, the
recipe applied over it (tone, quarter turns, mirror), the crop, and the marks as editable
objects. The reference implementation is `crates/znimok-format` (`read`, `peek`, `write`,
`save`). Znimok does not read Little Helpers `.lhshot` files; the format was designed afresh.

## Principles

- **Recognised by magic, not by extension.**
- **Little-endian** everywhere, written explicitly.
- **Forward compatible:** unknown blocks and unknown object fields are skipped by their
  length; unknown codes fall back to defaults; objects of an unknown kind are dropped.
- **Nothing that is default is written** — an old file and a file without new features are
  byte-identical, and new optional fields do not change the version.
- **No enum ordinals in the file:** the kind of a mark is a four-letter text tag; small
  enumerations use fixed codes listed below, never the order of a type in the source.
- **Descriptive blocks precede the pixels**, so a library can read a record by parsing only
  its head (`peek` stops at `SRC `).
- **Atomic save:** write `<path>.part`, flush it to disk, rename over the target.
- **Bounded reader:** see [Limits](#limits). A document is built only after the whole file
  parsed successfully.

## Layout

```
offset 0   8 bytes   magic  5A 4E 49 4D 4F 4B 1A 0A   ("ZNIMOK", SUB, LF)
offset 8   u16       major = 1   incompatible changes; a reader refuses a larger major
offset 10  u16       minor = 0   additions only; not checked by readers
offset 12  blocks until the end of the file:
           tag[4]    ASCII, space-padded, e.g. "SRC "
           u32       length of the value in bytes
           value
```

Primitive types: `u8`, `u16`, `u32`, `i32`, `i64`, `f32` (IEEE 754, must be finite);
`bool` = `u8` (0 false, anything else true); `str` = `u32` byte length + UTF-8, no
terminator; `rgb` = 4 bytes R, G, B, A (A is written 255 and ignored); `rect` = 4 × `i32`
x, y, w, h in pixels of the picture.

## Blocks

Written in this order. Only `SRC ` is mandatory.

| Tag | Value | Written |
|---|---|---|
| `META` | 16 bytes UUID (RFC 4122 byte order); `i64` created, Unix ms UTC — the moment of capture, re-saving does not move it; `str` name; `str` source (`screen`, `window`, `region`, `clipboard`, `file`…); `str` application version (diagnostics, readers ignore it) | always |
| `DESC` | `u8` version = 1; `str` description; `str` author; `str` copyright; `u32` n + n × `str` tags | when any is set |
| `INFO` | `u32` width, `u32` height of the **frame** (crop or whole picture); `u32` number of marks; `u8` document kind (0 = image; 1 = video, reserved for v2) | always |
| `THMB` | PNG of the composed frame (marks, crop), at most 320 × 240 | when the writer has one |
| `SRC ` | PNG of the current **original** — before tone, turns and mirror | always |
| `BANK` | `u32` n; n × (`u32` bank id, `u32` length, PNG) — pictures used by image marks | when image marks exist |
| `RCPE` | `f32` exposure EV −2…2; `f32` gamma 0.5…2; `i32` contrast −50…50; `u8` quarter turns clockwise 0…3; `bool` mirror (applied **before** the turns) | when not default (0, 1, 0, 0, false) |
| `CROP` | `rect` of the frame in the displayed picture's coordinates | when cropped |
| `SCAL` | `u32` DPI scale of the monitor of capture × 1000 (corner radii), 250…8000 | when ≠ 1000 |
| `OBJS` | `u32` n; n × `OBJ ` records in z-order, bottom first | always |
| `GRPN` | `u32` n; n × (`u32` group id, `str` name) | when groups are named |

Reserved for v2 (video, no major change): `CUTS`, `GEOM`, `MOUS`, `DEVT`, audio tracks.
Reserved: `FONT` — the bundled fonts a document uses (name and version), once fonts are
bundled (ZK-34).

Bank ids in `BANK` are local to the file. On reading, the original becomes bank 0 and the
`BANK` entries follow; image marks are renumbered accordingly. An image mark whose bank is
missing is dropped rather than failing the whole document.

## Marks — `OBJ ` records

An `OBJ ` value is a sequence of fields, each `tag[4] + u32 length + value`. Fields that are
default are not written. Unknown fields are skipped.

| Tag | Type | Meaning | Default / when written |
|---|---|---|---|
| `id  ` | `u32` | stable id inside the document (0 = assign) | always |
| `kind` | 4 ASCII | `rect` `elps` `line` `pen ` `text` `hide` `mark` `cnt ` `stmp` `img ` | always |
| `rect` | `rect` | box; for `line` the signs of w and h carry the direction from (x, y) to (x + w, y + h) | always |
| `colr` | `rgb` | main colour | #FF5A5F |
| `thck` | `i32` | thickness; marker: band height; counter/stamp: diameter | 4 |
| `alph` | `u8` | opacity 10…100 % | 100 |
| `nomn` | `bool` | no outline — rectangle/ellipse become a solid plate | false |
| `col2` | `rgb` | second colour: fill (rect, ellipse), outline (text), digit (counter) | none |
| `alp2` | `u8` | opacity of the second colour 10…100 % | 100 |
| `dash` | `u8` | 0 solid, 1 dashed, 2 dash-dot | 0 |
| `crnr` | `u8` | 0 sharp, 1 soft, 2 round | 0 |
| `crpx` | `i32` | corner radius in pixels, fixed when the corner level was chosen | 0 |
| `shdw` | `u8` | shadow 0 none, 1 light, 2 strong | 0 |
| `glow` | `u8` | glow 0 none, 1 light, 2 strong | 0 |
| `rot ` | `u16` | rotation 0…359° around the centre (not for `hide`, `mark`) | 0 |
| `grp ` | `u32` | selection group; members are adjacent in z-order | 0 |
| `name` | `str` | name in the Layers panel | none |
| `hidn` | `bool` | hidden: not drawn, not picked, not exported | false |
| `hdf ` `hdb ` | `u8` | line: head at the end / at the start — 0 none, 1 triangle, 2 chevron, 3 dot | line only |
| `hds ` | `u8` | line: head size step 0 small, 1 medium, 2 large | line only |
| `pts ` | `u32` n + n × (`i32` x, `i32` y) | pen trail, absolute picture coordinates | pen only |
| `text` `size` | `str`, `i32` | text and size in picture pixels (1…1600) | text only |
| `bold` `ital` | `bool` | | when true |
| `algn` | `u8` | 0 left, 1 centre, 2 right | when not left |
| `boxw` | `i32` | wrap width; 0 = one line, width from the text | when not 0 |
| `mode` `strn` | `u8`, `u8` | hide: 0 blur, 1 pixelate, 2 plate; strength 10…100 % | hide only |
| `cseq` `cgrp` `cstr` | `u32`, `u32`, `i32` | counter: creation sequence, numbering group, start of the group | counter only |
| `cshp` | `u8` | counter shape 0 circle, 1 rounded box, 2 pin | when not 0 |
| `stmp` | `u32` | stamp: 0…5 vector (check, cross, question, exclamation, star, warning), 100+ emoji | stamp only |
| `img ` | `u32` | image mark: bank id in `BANK` | image only |

A counter's number is not stored: it is `start + rank of seq` among the counters of its group,
so deleting one renumbers the rest.

## Limits

A reader refuses (with a "damaged" error), before allocating: files over 512 MiB; strings
over 1 MiB; more than 100 000 marks, 2²⁰ pen points in one mark, 4 096 bank pictures,
10 000 group names, 1 000 tags; pictures wider or taller than 32 767 px or over 2²⁸ pixels
(checked from the PNG header). Numeric fields are clamped to their ranges.

## Errors

| Error | Meaning |
|---|---|
| not a Znimok document | wrong magic, or shorter than 12 bytes / longer than the limit |
| newer version | `major` > 1: made by a newer Znimok |
| damaged | truncated, a record running past its container, out of limits, or no `SRC ` |

## Changing the format

- New optional block or field, new kind, new code: minor + 1 (readers do not check minor).
- Anything an old reader would misread: major + 1.
- Never reuse a tag or a code with a different meaning.
