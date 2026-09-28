use super::*;

use znimok_core::{CounterShape, Head};

fn rich_doc() -> Document {
    let mut doc = znimok_render::reference::reference_document(320, 200);
    doc.name = "Налаштування друку — крок 3".into();
    doc.meta = Meta {
        created_ms: 1_790_558_000_123,
        source: "window".into(),
        description: "опис".into(),
        author: "Plum".into(),
        copyright: "".into(),
        tags: vec!["друк".into(), "крок".into()],
    };
    doc.recipe = Recipe {
        exposure: 0.5,
        gamma: 1.2,
        contrast: -10,
        rot_quarters: 1,
        mirror: true,
    };
    doc.crop = Some(IRect::new(10, 20, 100, 150));
    doc.shot_scale = 1500;
    doc.objects[0].group = 7;
    doc.objects[1].group = 7;
    doc.compact_groups();
    doc.group_names.insert(7, "Кнопки".into());
    doc.objects[2].hidden = true;
    doc.objects[2].name = Some("прихована".into());
    // A pen trail with heads (ZK-48) goes through the file too.
    for o in &mut doc.objects {
        if let Data::Pen {
            head_front,
            head_back,
            ..
        } = &mut o.data
        {
            *head_front = Head::Chevron;
            *head_back = Head::Dot;
        }
    }
    doc
}

fn assert_same(a: &Document, b: &Document) {
    assert_eq!(a.id, b.id);
    assert_eq!(a.name, b.name);
    assert_eq!(a.meta, b.meta);
    assert_eq!(a.recipe, b.recipe);
    assert_eq!(a.crop, b.crop);
    assert_eq!(a.shot_scale, b.shot_scale);
    assert_eq!(a.group_names, b.group_names);
    assert_eq!(a.source().width, b.source().width);
    assert_eq!(a.source().rgba, b.source().rgba);
    assert_eq!(a.objects.len(), b.objects.len());
    for (x, y) in a.objects.iter().zip(&b.objects) {
        match (&x.data, &y.data) {
            // Bank numbers are renumbered by the file; compare the pixels instead.
            (Data::Image { bank: p }, Data::Image { bank: q }) => {
                assert_eq!(a.banks[*p as usize].rgba, b.banks[*q as usize].rgba);
                let mut xx = x.clone();
                xx.data = y.data.clone();
                assert_eq!(&xx, y);
            }
            _ => assert_eq!(x, y),
        }
    }
}

#[test]
fn every_kind_round_trips() {
    let doc = rich_doc();
    let kinds: std::collections::BTreeSet<_> = doc
        .objects
        .iter()
        .map(|o| format!("{:?}", o.kind()))
        .collect();
    assert_eq!(
        kinds.len(),
        10,
        "reference scene covers all kinds: {kinds:?}"
    );
    let bytes = write(&doc, &WriteOptions::default());
    let back = read(&bytes).unwrap();
    assert_same(&doc, &back);
    // Writing the read document again gives the same bytes (deterministic writer).
    assert_eq!(write(&back, &WriteOptions::default()), bytes);
}

#[test]
fn defaults_are_not_written() {
    let mut doc = Document::from_raster("", Raster::solid(2, 1, Rgb::WHITE));
    doc.push(Object::new(IRect::new(0, 0, 1, 1), Data::Rect));
    let bytes = write(&doc, &WriteOptions::default());
    let has = |t: &[u8]| bytes.windows(4).any(|w| w == t);
    for absent in [
        b"DESC", b"RCPE", b"CROP", b"SCAL", b"GRPN", b"BANK", b"THMB", b"colr", b"thck", b"shdw",
        b"name", b"hidn",
    ] {
        assert!(
            !has(absent),
            "{} should be absent",
            String::from_utf8_lossy(absent)
        );
    }
    for present in [
        b"META", b"INFO", b"SRC ", b"OBJS", b"OBJ ", b"id  ", b"kind", b"rect",
    ] {
        assert!(has(present));
    }
    // Header.
    assert_eq!(&bytes[..8], b"ZNIMOK\x1A\n");
    assert_eq!(&bytes[8..12], &[1, 0, 0, 0]);
}

#[test]
fn metadata_comes_before_pixels_and_peek_reads_only_the_head() {
    let doc = rich_doc();
    let thumb = Raster::solid(32, 20, Rgb::BLUE);
    let bytes = write(
        &doc,
        &WriteOptions {
            thumbnail: Some(thumb),
            ..Default::default()
        },
    );
    let pos = |t: &[u8]| bytes.windows(4).position(|w| w == t).unwrap();
    assert!(
        pos(b"META") < pos(b"SRC ")
            && pos(b"DESC") < pos(b"SRC ")
            && pos(b"INFO") < pos(b"SRC ")
            && pos(b"THMB") < pos(b"SRC ")
    );
    // Peek works even if everything after the pixels' header is missing.
    let cut = &bytes[..pos(b"SRC ") + 8];
    let p = peek(cut).unwrap();
    assert_eq!(p.id, Some(doc.id));
    assert_eq!(p.name, doc.name);
    assert_eq!(p.meta, doc.meta);
    assert_eq!((p.width, p.height), (100, 150));
    assert_eq!(p.object_count as usize, doc.objects.len());
    let t = decode_png(p.thumbnail_png.as_ref().unwrap(), &Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (32, 20));
}

#[test]
fn unknown_blocks_fields_and_kinds_are_skipped() {
    let mut doc = Document::from_raster("x", Raster::solid(4, 4, Rgb::WHITE));
    doc.push(Object::new(
        IRect::new(1, 1, 2, 2),
        Data::Line {
            head_front: Head::Dot,
            head_back: Head::None,
            head_size: 2,
        },
    ));
    let mut bytes = write(&doc, &WriteOptions::default());
    // A future block at the end and one before the pixels.
    let mut extra = Writer::default();
    extra.record(b"FUTR", |w| w.bytes(&[1, 2, 3, 4, 5]));
    bytes.extend_from_slice(&extra.buf);
    let src = bytes.windows(4).position(|w| w == b"SRC ").unwrap();
    bytes.splice(src..src, extra.buf.iter().copied());
    // A future field inside the object: append to the OBJ record and fix both lengths.
    let back = read(&bytes).unwrap();
    assert_eq!(back.objects.len(), 1);

    // An object of an unknown kind is dropped, the rest survives.
    let mut w = Writer::default();
    w.bytes(&MAGIC);
    w.u16(1);
    w.u16(7); // newer minor: accepted
    w.record(b"SRC ", |w| {
        w.bytes(&encode_png(
            &Raster::solid(2, 2, Rgb::BLACK),
            png::Compression::Fast,
        ))
    });
    w.record(b"OBJS", |w| {
        w.u32(2);
        w.record(b"OBJ ", |w| {
            w.record(b"kind", |w| w.bytes(b"wave"));
            w.record(b"rect", |w| (0..4).for_each(|_| w.i32(1)));
        });
        w.record(b"OBJ ", |w| {
            w.record(b"kind", |w| w.bytes(b"cnt "));
            w.record(b"rect", |w| (0..4).for_each(|_| w.i32(1)));
            w.record(b"cshp", |w| w.u8(99)); // unknown code → default shape
            w.record(b"zzzz", |w| w.u32(5)); // unknown field
        });
    });
    let d = read(&w.buf).unwrap();
    assert_eq!(d.objects.len(), 1);
    assert!(matches!(
        d.objects[0].data,
        Data::Counter {
            shape: CounterShape::Circle,
            ..
        }
    ));
}

#[test]
fn errors_are_specific() {
    assert_eq!(read(b"PNG not a doc"), Err(FormatError::NotZnimok));
    assert_eq!(read(b"ZNIMOK\x1A\n"), Err(FormatError::NotZnimok));
    let mut v = MAGIC.to_vec();
    v.extend_from_slice(&[2, 0, 0, 0]);
    assert_eq!(read(&v), Err(FormatError::TooNew { major: 2, minor: 0 }));
    let mut v = MAGIC.to_vec();
    v.extend_from_slice(&[1, 0, 0, 0]);
    assert!(matches!(read(&v), Err(FormatError::Corrupt(m)) if m.contains("SRC")));
    let doc = rich_doc();
    let bytes = write(&doc, &WriteOptions::default());
    assert!(matches!(
        read(&bytes[..bytes.len() - 3]),
        Err(FormatError::Corrupt(_))
    ));
    assert!(is_znimok(&bytes) && !is_znimok(b"ZNIMOK"));
}

#[test]
fn save_is_atomic_and_readable() {
    let dir = std::env::temp_dir().join(format!("znimok-format-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.znimok");
    let doc = rich_doc();
    save(&path, &doc, &WriteOptions::default()).unwrap();
    save(&path, &doc, &WriteOptions::default()).unwrap(); // replaces
    assert!(!dir.join("a.znimok.part").exists());
    let back = read(&std::fs::read(&path).unwrap()).unwrap();
    assert_same(&doc, &back);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stable_codes_do_not_follow_enum_order() {
    // These numbers are part of the file format and must never change.
    assert_eq!(
        (
            dash_code(Dash::DashDot),
            corners_code(Corners::Round),
            effect_code(Effect::Strong)
        ),
        (2, 2, 2)
    );
    assert_eq!(
        (
            head_code(Head::Dot),
            hide_code(HideMode::Plate),
            shape_code(CounterShape::Pin),
            align_code(Align::Right)
        ),
        (3, 2, 2, 2)
    );
    assert_eq!(&kind_tag(Kind::Counter), b"cnt ");
    for k in [
        Kind::Rect,
        Kind::Ellipse,
        Kind::Line,
        Kind::Pen,
        Kind::Text,
        Kind::Hide,
        Kind::Mark,
        Kind::Counter,
        Kind::Stamp,
        Kind::Image,
    ] {
        assert_eq!(kind_from_tag(&kind_tag(k)), Some(k));
    }
}
