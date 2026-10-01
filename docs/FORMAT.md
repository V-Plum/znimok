# The `.znimok` document format, version 1.1

A `.znimok` file is one screenshot with its annotations: the untouched original picture, the
recipe applied over it (tone, quarter turns, mirror), the crop, and the marks as editable
objects — or, since 1.1, one screen recording: the same document built on the first frame (the
*poster*), plus the encoded video stream, the edit list, the audio tracks and the logs of the
recording (see [Video documents](#video-documents)). The reference implementation is
`crates/znimok-format` (`read`, `read_any`, `open`, `peek`, `write`, `write_video_to`, `save`,
`save_video`). Znimok does not read Little Helpers `.lhshot`, `.lhvideo` or `.mp4.lhmeta` files;
the format was designed afresh.

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
offset 10  u16       minor       additions only; not checked by readers (see below)
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
| `DESC` | `u8` version = 2; `str` description; `str` author; `str` copyright; `u32` n + n × `str` tags; `u8` flags (bit 0: pinned in the library, ZK-178) — version 1 ends after the tags, and a version-1 reader ignores the flags | when any is set |
| `INFO` | `u32` width, `u32` height of what export produces — the **frame** (crop or whole picture), for a resized video the export size; `u32` number of marks; `u8` document kind: 0 = image, 1 = video (1.1); an unknown kind reads as an image | always |
| `VINF` | video only, see [Video documents](#video-documents) | video |
| `THMB` | PNG of the composed frame (marks, crop), at most 320 × 240 | when the writer has one |
| `SRC ` | PNG of the current **original** — before tone, turns and mirror; for a video, the poster (first frame at the video's size) | always |
| `BANK` | `u32` n; n × (`u32` bank id, `u32` length, PNG) — pictures used by image marks | when image marks exist |
| `RCPE` | `f32` exposure EV −2…2; `f32` gamma 0.5…2; `i32` contrast −50…50; `u8` quarter turns clockwise 0…3; `bool` mirror (applied **before** the turns) | image or video (tone only), when not default (0, 1, 0, 0, false) |
| `CROP` | `rect` of the frame in the displayed picture's coordinates | image, when cropped |
| `SCAL` | `u32` DPI scale of the monitor of capture × 1000 (corner radii), 250…8000 | when ≠ 1000 |
| `OBJS` | `u32` n; n × `OBJ ` records in z-order, bottom first | always |
| `GRPN` | `u32` n; n × (`u32` group id, `str` name) | when groups are named |
| `GEOM` `CUTS` `AUDI` `MOUS` `DEVT` | video only, see [Video documents](#video-documents) | video, when not default |
| `MP4 ` | a chunk of the encoded video stream | video, last |

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
| `vspn` | `u32`, `u32` | video: shown in frames `[from, to)` of the stream; ignored when empty | shown for the whole video |
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

## Video documents

Added in 1.1 (ZK-96). A video document is an ordinary document whose `INFO` kind is 1: the
poster in `SRC ` is the first frame at the video's size, and the marks, groups and banks are
those of a screenshot, in the poster's (= the video's) pixels. So everything that handles
screenshots — the library, thumbnails, Quick Look, the editor's geometry — works on a video
unchanged, and a reader that only knows images still shows the poster with its marks. On top
come the video blocks:

| Tag | Value | Written |
|---|---|---|
| `VINF` | stream parameters, see below | always, right after `INFO` |
| `GEOM` | frame and export size, see below | when cropped or resized |
| `CUTS` | edit list, see below | when anything is cut or trimmed |
| `AUDI` | audio tracks, see below | when the stream has audio |
| `MOUS` | mouse log, see below | when the recording has one |
| `DEVT` | browser log, see below | when the recording has one |
| `MP4 ` | raw bytes of the encoded stream, in chunks | always, last |

A video document's `RCPE` carries its tone only (ZK-188: exposure, gamma, contrast, applied to the
frames in the player and on export; a writer writes 0 turns and no mirror, a reader ignores them),
and it has no `CROP` (the frame is in `GEOM`). It needs `VINF`, a poster of exactly the `VINF` size and a non-empty stream; any
of them missing or wrong is "damaged". Blocks are written in this order: `META`, `DESC`, `INFO`,
`VINF`, `THMB`, `SRC `, `BANK`, `SCAL`, `OBJS`, `GRPN`, `GEOM`, `CUTS`, `AUDI`, `MOUS`, `DEVT`,
`MP4 `… — so `peek` learns the kind and the duration from the head of the file.

### The stream: embedded, last, in chunks

The encoded video (MP4: H.264, optionally HEVC, AAC audio tracks) is stored **inside** the
document, as the value of `MP4 ` blocks at the end of the file. Their values, concatenated in
file order, are the MP4 byte for byte; the writer makes chunks of at most 2³⁰ bytes (a block
length is a `u32`), a reader accepts any sizes, including empty chunks and other blocks between
them. The reader never loads the stream: it locates the chunks, seeks over them
(`open`/`read_from`) and returns their file ranges; a player reads them straight from the file
through a byte stream over those ranges (`PayloadReader` → Media Foundation `IMFByteStream`,
AVFoundation resource loader), with no copy.

Why embedded rather than a reference to an `.mp4` next to the document:

- **One file is one record.** The library, cloud folders and synchronisation (PLAN decision 14)
  see one file; a rename, a move, a sync conflict or "send this file" cannot separate a project
  from its video. A sibling file is exactly what breaks when a user tidies a folder — Little
  Helpers needed a hidden `.lhmeta` next to each MP4 and a cache key to notice when they drifted.
- **One save is atomic.** The usual `.part` → rename covers the stream and the edits together;
  there is no window in which the document points to a video that is not there yet.
- **The owner's rule**: the video project lives in blocks of the `.znimok` container
  (clarification of 27.09.2026); no Little Helpers compatibility to keep.
- **Nothing is lost by it:** people without Znimok get an exported MP4, as with screenshots
  (PLAN decision 12); the stream in the file is the untouched recording, the edits stay editable.

The cost is that saving the edits of a project rewrites the file, the stream included (a copy at
disk speed; atomic save rewrites the file anyway). The stream comes last so the blocks the
editor changes sit before it and a future in-place edit (truncate after the last block before
`MP4 `, append) remains possible without changing the format.

### `VINF` — stream parameters

| Offset | Type | Meaning |
|---|---|---|
| 0 | `u32` | width of the encoded frames, px (1…32 767) — also the poster's size |
| 4 | `u32` | height, px (1…32 767) |
| 8 | `u32` | frame rate × 1000 (1…1 000 000; 30 000, 60 000, 29 970) |
| 12 | `u32` | frames in the stream, N (1…2³¹−1); frame numbers run over `[0, N)` |
| 16 | `i64` | duration, 100 ns (≥ 0) |
| 24 | tag[4] | video codec: `avc1` H.264, `hvc1` HEVC, `av01` AV1; unknown tags are kept |
| 28 | `u8` | flags, optional (absent = 0): bit 0 — the file carries a browser log (`DEVT`), so its icon can say so from the head of the file (ZK-150); other bits reserved, written 0, ignored |

Out-of-range values make the document damaged.

### `GEOM` — frame and export size

| Offset | Type | Meaning |
|---|---|---|
| 0 | `u32` | export width, px; 0 with a height of 0 = no resize (the frame's size) |
| 4 | `u32` | export height, px |
| 8 | `rect` | the frame (crop) in video pixels — **only when cropped** (block of 24 bytes; 8 otherwise) |

The frame is clamped to the video; export sizes to 1…32 767 (the exporter makes them even).
The reader puts the frame into the document's crop, where the editor keeps it for screenshots.

### `CUTS` — edit list

| Offset | Type | Meaning |
|---|---|---|
| 0 | `u32` | n parts (≤ 100 000) |
| 4 | n × (`u32` a, `u32` b, `bool` off) | part `[a, b)` in frame numbers; `off` = cut out |
| 4 + 9n | `u32` | in: first frame kept by the trim handles |
| 8 + 9n | `u32` | out: end of the trim, exclusive |

Parts cover `[0, N)` in order without gaps, each non-empty; `in < out ≤ N`. The trim handles do
not split parts. Export keeps the parts not cut, within `[in, out)`, adjacent ones merged. An
edit list that does not fit the stream is dropped (the video opens uncut) rather than failing
the document. Not written when nothing is cut (one part `[0, N)`, in 0, out N).

### `AUDI` — audio tracks

`u32` n (≤ 16), then n `TRK ` records. Record `i` describes the `i`-th audio track of the MP4, in
track order. Each source is recorded as its own AAC track and the tracks are mixed only at
export (PLAN decision 16: the user can mute the microphone or lower the system sound after the
fact; on macOS the sources even run on different clocks) — the mix sums the tracks that are not
muted, with their volume and shift, through the soft limiter. A `TRK ` value is a sequence of
fields like an `OBJ `; unknown fields are skipped:

| Tag | Type | Meaning | Default / when written |
|---|---|---|---|
| `srce` | `u8` | source: 0 system sound (loopback), 1 microphone; unknown → 0 | always |
| `labl` | `str` | device name as the user saw it | when not empty |
| `volm` | `u8` | volume at export, % (0…200, clamped) | 100 |
| `mute` | `bool` | left out of the mix | false |
| `offs` | `i32` | shift against the video at export, ms (±60 000, clamped; positive = later) | 0 |
| `peak` | `u8` step, then bytes | loudness for the timeline (ZK-189): the step in ms (10), then a byte per step — the peak of that stretch, `√peak · 255`; another step is ignored | when measured (recordings since 0.0.2) |

### `MOUS` — mouse log

| Offset | Type | Meaning |
|---|---|---|
| 0 | `u32` | n records (≤ 2²²) |
| 4 | `u32` | size of a record, bytes (13…256; this version writes 13) |
| 8 | n × record | |

A record:

| Offset | Type | Meaning |
|---|---|---|
| 0 | `i32` | video time, ms (pauses already removed) |
| 4 | `i32` | x in pixels of the video frame |
| 8 | `i32` | y |
| 12 | `u8` | low 4 bits: 0 left, 1 right, 2 middle, 15 cursor position (no button); bit 4: pressed (for 15: left button held) |
| 13… | | fields of a newer minor version — skipped by the record size |

Both presses and releases are written, and — when recorded — cursor position samples. A record
with an unknown button code is skipped (a click of an unknown button must not become a left
click).

### `DEVT` — browser log

| Offset | Type | Meaning |
|---|---|---|
| 0 | `u8` | version = 1 (not checked) |
| 1 | `i64` | wall clock of the first frame, Unix ms UTC |
| 9 | `u32` | n events (≤ 200 000) |
| 13 | n × (`i32` ms, `str` JSON) | video time of the event (pauses removed); the extension's JSON as it came |

Events are in time order. The JSON is opaque to the format (it is UTF-8, nothing more is
checked).

### One extension

Screenshots and videos share the extension `.znimok` (PLAN decision 28, reviewed at the video
stage): the kind is inside the file, and readers recognise documents by magic anyway. Should two
extensions be chosen, only `extension_for(kind)` and the file associations change — not the
format.

## Limits

A reader refuses (with a "damaged" error), before allocating: files over 512 MiB — for a video,
everything but the `MP4 ` chunks, whose size is not limited; strings over 1 MiB; more than
100 000 marks, 2²⁰ pen points in one mark, 4 096 bank pictures, 10 000 group names, 1 000 tags;
pictures wider or taller than 32 767 px or over 2²⁸ pixels (checked from the PNG header); more
than 100 000 parts in an edit list, 16 audio tracks, 2²² mouse records, 200 000 browser events,
65 536 stream chunks; a count whose records cannot fit in the rest of their block. Numeric
fields are clamped to their ranges.

## Errors

| Error | Meaning |
|---|---|
| not a Znimok document | wrong magic, or shorter than 12 bytes / longer than the limit |
| newer version | `major` > 1: made by a newer Znimok |
| damaged | truncated, a record running past its container, out of limits, or no `SRC `; for a video, no `VINF`, a poster of another size or no stream |

## Changing the format

- New optional block or field, new kind, new code: minor + 1 (readers do not check minor).
- A writer stamps the **lowest** minor that describes the file: screenshots are still written as
  1.0 (byte-identical to older writers), video documents as 1.1.

| Version | Change |
|---|---|
| 1.0 | screenshots |
| 1.1 | video documents: `INFO` kind 1, `VINF`, `GEOM`, `CUTS`, `AUDI`, `MOUS`, `DEVT`, `MP4 `, the `vspn` field of a mark (ZK-96) |
- Anything an old reader would misread: major + 1.
- Never reuse a tag or a code with a different meaning.
