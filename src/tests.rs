//! Decodes a synthetic document written the way Forte writes them.

use crate::FnfFile;
use crate::conductor::{Barline, TextKind};
use crate::note::NoteId;
use crate::staff::{ClefKind, Element};
use std::collections::HashMap;
use std::io::{Cursor, Write};

/// Wraps `contents` in a compound file, as Forte saves it.
fn compound_file(contents: &[u8]) -> Vec<u8> {
    let mut file = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    file.create_stream("/Contents")
        .unwrap()
        .write_all(contents)
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

/// Writes an MFC archive the way Forte does: classes and objects share
/// one index count (the header uses up the first three).
struct Writer {
    out: Vec<u8>,
    classes: HashMap<&'static str, u16>,
    count: u16,
}

impl Writer {
    fn new() -> Self {
        let mut out = b"MMMF\x0a\x00".to_vec();
        out.extend([0; 20]);
        // The document element: flags, one view, view number.
        out.extend([0xca, 0x01, 0x00]);
        Self {
            out,
            classes: HashMap::new(),
            count: 3,
        }
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.out.extend_from_slice(bytes);
        self
    }

    fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    fn f32(&mut self, v: f32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    fn zeros(&mut self, n: usize) -> &mut Self {
        self.bytes(&vec![0; n])
    }

    fn string(&mut self, s: &str) -> &mut Self {
        self.bytes(&[s.len() as u8]).bytes(s.as_bytes())
    }

    /// Starts an object of `class`, returning its index.
    fn object(&mut self, class: &'static str) -> u16 {
        match self.classes.get(class) {
            Some(&index) => {
                self.u16(0x8000 | index);
            }
            None => {
                self.classes.insert(class, self.count);
                self.count += 1;
                self.u16(0xffff).u16(2).u16(class.len() as u16);
                self.bytes(class.as_bytes());
            }
        }
        self.count += 1;
        self.count - 1
    }

    /// An element head with `views` views and no extra byte.
    fn element(&mut self, class: &'static str, views: u8) -> u16 {
        let index = self.object(class);
        self.bytes(&[0, views]);
        index
    }

    /// A view's head: flags and owner (then, for a notation view, a
    /// position).
    fn view(&mut self, k: u8, class: &'static str, svflags: u32, owner: u16) -> &mut Self {
        self.bytes(&[k]);
        self.object(class);
        self.u32(0).u32(svflags).u16(owner);
        if class.starts_with("DCsv") && class != "DCsvConductorStaff" {
            self.object("MCPosition");
            self.bytes(&[0x1a, 0x64, 0, 0, 0, 0]);
        }
        self
    }

    fn trailer(&mut self, time: f32, staff: u16) -> &mut Self {
        self.bytes(&[0]).f32(time).u16(staff)
    }

    fn barline(&mut self, kind: u8) {
        let e = self.element("ECBarline", 1);
        self.view(2, "DCsvBarline", 0x19ff, e)
            .u16(0xffff)
            .bytes(&[kind])
            .zeros(11);
    }

    fn note(&mut self, time: f32, midi: u8, step: u8, value: u8) -> u16 {
        let e = self.element("ECNote", 2);
        self.bytes(&[0]).object("DCsqNote");
        self.u32(0x15)
            .u32(1)
            .u16(e)
            .bytes(&[0x35, 0x90, midi, 0x64, 0x3d, 0x90, midi, 0]);
        let mut body = [0u8; 22];
        body[0] = step << 4;
        body[2] = midi;
        body[6] = value;
        self.view(2, "DCsvNote", 0x1050_0999, e)
            .bytes(&body)
            .bytes(&[6, 0])
            .f32(time)
            .bytes(&[0x10, 0, 0])
            .f32(1.95)
            .bytes(&[0xff; 8]);
        e
    }
}

/// A two-measure 2/4 bass part: a half-note A tied over the barline,
/// with the clef, key (one flat), tempo, title, staff name and final
/// barline the reader picks up - and a staff MIDI setup holding objects
/// the reader skips.
fn sample() -> Vec<u8> {
    let mut w = Writer::new();
    let conductor = w.object("ECConductor");
    w.bytes(&[0xca, 2, 0]);
    let staff_c = w.element("ECConductorStaff", 2);
    w.bytes(&[0]).object("DCsqConductorStaff");
    w.u32(0).u32(9).u16(staff_c).f32(1.0).zeros(52);
    w.view(2, "DCsvConductorStaff", 0x1bff, staff_c).zeros(10);
    // The conductor staff's children.
    w.u16(5);
    w.object("ECMeterEnv");
    w.bytes(&[0xca, 1, 0]);
    let meter = w.element("ECMeter", 2);
    w.bytes(&[0]).object("DCsqMeter");
    w.u32(9).u32(1).u16(meter);
    w.view(2, "DCsvMeter", 0x19ff, meter);
    let mut body = [0u8; 45];
    body[5] = 0x0e;
    body[13] = 2;
    body[14] = 4;
    w.bytes(&body).bytes(&[0]).zeros(29).u16(0);
    w.object("ECTempoEnv");
    w.bytes(&[0xca, 1, 0]);
    w.element("ECTempo", 0);
    w.f32(0.0).u16(0x0e).bytes(&[0xc3]);
    for v in [60.0, 96.0, 1.0, 1.0, 1.0] {
        w.f32(v);
    }
    w.bytes(&[1]).string("").f32(50.0);
    w.bytes(&[0]).zeros(29).u16(0);
    let text = w.element("ECText", 2);
    w.bytes(&[0]).object("DCsqText");
    w.u32(0).u32(1).u16(text);
    w.view(2, "DCsvText", 0x4019ff, text)
        .f32(0.0)
        .zeros(3)
        .bytes(&[0x08])
        .string("Sample")
        .zeros(8)
        .string("Arial");
    for kind in [0x40, 0x43] {
        let m = w.element("ECConductorMeasure", 1);
        w.view(2, "DCsvConductorMeasure", 0x19ff, m)
            .u32(0x78)
            .u16(1);
        w.barline(kind);
    }
    // Layout data between the conductor staff and the staves.
    w.zeros(48);
    let _ = conductor;

    let staff = w.element("ECStaff", 2);
    w.bytes(&[0]).object("DCsqStaff");
    w.u32(9).u32(0x72).u16(staff).f32(1.0).zeros(52);
    // MIDI setup the reader skips, with two objects of its own.
    w.u16(1).object("LCsqTranspose");
    w.zeros(3).object("CMidiEnvelopeMgr");
    w.zeros(40).f32(1.0);
    w.view(2, "DCsvStaff", 0x1bff, staff);
    w.bytes(&[0x64, 0, 0, 0, 0, 5, 0]).u16(1).u16(2);
    let name = w.element("ECText", 2);
    w.bytes(&[0]).object("DCsqText");
    w.u32(0).u32(1).u16(name);
    w.view(2, "DCsvText", 0x4019ff, name)
        .f32(0.0)
        .zeros(3)
        .bytes(&[0x03])
        .string("Bass")
        .zeros(8)
        .string("");
    let mut notes = Vec::new();
    for (i, start) in [0.0f32, 2.0].into_iter().enumerate() {
        let m = w.element("ECMeasure", 1);
        w.view(2, "DCsvMeasure", 0x19ab, m)
            .u16(1)
            .bytes(&[0, 5, 5, 5, 0x69, 0, 0, 0]);
        w.u16(if i == 0 { 3 } else { 1 });
        if i == 0 {
            let clef = w.element("ECClef", 1);
            w.view(2, "DCsvClef", 0x1019ff, clef)
                .trailer(0.0, 0x10)
                .bytes(&[3]);
            let key = w.element("ECKey", 1);
            w.view(2, "DCsvKey", 0x0bff, key)
                .bytes(&[9])
                .zeros(7)
                .bytes(&[0, 0x10, 0, 0x08]);
        }
        notes.push(w.note(start, 45, 5, 2));
        w.trailer(start, 0x1e);
    }
    let tie = w.element("ECTie", 1);
    w.view(2, "DCsvTie", 0x4019ff, tie);
    w.u16(2);
    for note in &notes {
        w.u16(*note);
    }
    w.trailer(0.0, 0x10).bytes(&[2]);
    w.u16(0);
    w.out
}

#[test]
fn reads_a_sample_document() {
    let doc = FnfFile::from_bytes(&compound_file(&sample()))
        .unwrap()
        .document;
    assert_eq!(doc.version, 10);
    assert_eq!(doc.title(), Some("Sample"));
    let conductor = &doc.conductor;
    let meter = conductor.staff.meters[0];
    assert_eq!((meter.time, meter.beats, meter.beat_type), (0.0, 2, 4));
    assert_eq!(conductor.staff.tempos[0].bpm, 96.0);
    assert!(!conductor.staff.tempos[0].fermata);
    let closing: Vec<_> = conductor
        .staff
        .measures
        .iter()
        .map(|m| m.closing_barline())
        .collect();
    assert_eq!(closing, [Some(Barline::Regular), Some(Barline::Final)]);

    assert_eq!(conductor.staves.len(), 1);
    let staff = &conductor.staves[0];
    assert_eq!(staff.name(), Some("Bass"));
    assert_eq!(staff.texts[0].kind, TextKind::StaffName);
    assert_eq!(staff.measures.len(), 2);
    assert_eq!(staff.measures[1].start, 2.0);
    let first = &staff.measures[0].elements;
    assert!(matches!(first[0], Element::Clef(c) if c.kind == ClefKind::Bass && !c.lower));
    assert!(matches!(first[1], Element::Key(k) if k.fifths == -1));
    let Element::Note(note) = &first[2] else {
        panic!("expected a note, got {:?}", first[2]);
    };
    assert_eq!((note.midi, note.step, note.value), (45, 5, 2));
    assert_eq!(note.spelling(), Some((5, 0, 2)));
    let tied: Vec<NoteId> = staff.ties[0].notes.clone();
    assert_eq!(tied[0], note.id);
}

#[test]
fn reads_a_bare_contents_stream() {
    let doc = FnfFile::from_bytes(&sample()).unwrap().document;
    assert_eq!(doc.conductor.staves[0].measures.len(), 2);
}

#[test]
fn rejects_other_files() {
    assert!(FnfFile::from_bytes(b"not a score").is_err());
    let mut truncated = sample();
    truncated.truncate(truncated.len() / 2);
    assert!(FnfFile::from_bytes(&truncated).is_err());
}

#[test]
fn survives_damaged_files() {
    let data = sample();
    for len in 0..data.len() {
        let _ = FnfFile::from_bytes(&data[..len]);
    }
    for i in (0..data.len()).step_by(3) {
        let mut damaged = data.clone();
        damaged[i] ^= 0x5a;
        let _ = FnfFile::from_bytes(&damaged);
    }
}
