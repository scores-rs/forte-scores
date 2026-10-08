//! Walks the MFC object archive in a `.fnf` file's `Contents` stream.
//!
//! Forte is an MFC application and saves its document with `CArchive`:
//! objects are written depth-first, each introduced by a 16-bit tag that
//! either defines a new class (`0xFFFF`, schema and name follow), names an
//! already defined class (`0x8000 | index`) or refers back to an object
//! written earlier (`index`). Class definitions and objects share one index
//! space, numbered in the order they first appear. Each class then writes
//! its own fields with no length prefix, so every class this reader meets
//! has to be decoded field by field - the layouts below were worked out from
//! sample files and are only as complete as those files are.
//!
//! The document is a tree of "EC" objects (score elements: staves,
//! measures, notes, ...), each owning "views": a `DCsq*` object with its
//! playback data (view 0) and a `DCsv*` object with its notation data
//! (view 2). Every view starts with the index of the element it belongs to,
//! which this reader checks as it goes.
//!
//! Two parts of the file hold data the reader doesn't need and whose layout
//! varies too much to decode: a conductor staff's page and font setup, and
//! each staff's MIDI setup (patches, controller envelopes). Both are skipped
//! by searching for the next record that can be recognized. Skipping objects
//! throws off the index count, so after a skip the count is corrected from
//! the next view's owner index (see [`Archive::resync`]). The skipped bytes
//! are kept for [`Archive::skipped`], where dynamics are recovered from.

use crate::error::{ForteError, Result};

/// An object's position in [`Archive::nodes`].
pub(crate) type Id = usize;

/// The MFC file version in the stream header that changed some layouts.
const VERSION_10: u16 = 10;

/// A decoded object.
#[derive(Debug, Clone)]
pub struct Node {
    pub class: String,
    pub data: Data,
}

/// The fields this reader keeps, per kind of object.
#[derive(Debug, Clone, Default)]
pub enum Data {
    #[default]
    None,
    /// A score element: its views by view number, plus anything the
    /// element class writes itself.
    Element { views: Vec<(u8, Id)>, extra: Extra },
    /// A playback view (`DCsq*`); only notes' MIDI events aren't empty.
    Sequence,
    /// A notation view (`DCsv*`).
    View(View),
    /// A list of objects: a beam's notes.
    List(Vec<Id>),
}

/// What an element class writes after its views.
#[derive(Debug, Clone, Default)]
pub enum Extra {
    #[default]
    None,
    /// Child elements (conductor staff, staff, envelopes).
    Children(Vec<Id>),
    /// The conductor's staves (the conductor staff comes first, in `views`).
    Staves(Vec<Id>),
    Tempo(Tempo),
}

#[derive(Debug, Clone)]
pub struct Tempo {
    /// Quarter notes from the start.
    pub time: f32,
    pub values: [f32; 5],
    /// Bit 1: a fermata's slowdown starts here (in a fermata change).
    pub kind: u8,
    /// `0x18` marks the tempo changes Forte inserts to play a fermata.
    pub flags: u8,
}

/// The notation view's fields, per class.
#[derive(Debug, Clone, Default)]
pub enum View {
    #[default]
    Other,
    Note {
        body: [u8; 22],
        children: Vec<Id>,
        time: f32,
        staff: u8,
    },
    Rest {
        body: [u8; 18],
        children: Vec<Id>,
        time: f32,
        staff: u8,
    },
    Chord {
        body: [u8; 18],
        notes: Vec<Id>,
        time: f32,
        staff: u16,
    },
    Tuplet {
        members: Vec<Id>,
        ratio: [u8; 3],
    },
    /// A dot (the number of dots), accidental (its kind) or articulation
    /// (its kind) on a note.
    Mark(u8),
    Key([u8; 12]),
    Clef([u8; 8]),
    Meter {
        time: f32,
        beats: u8,
        beat_type: u8,
        /// The time signature shown, when it differs (a pickup measure).
        shown: Option<(u8, u8)>,
    },
    Text {
        kind: u8,
        text: String,
    },
    ChordSymbol {
        time: f32,
        staff: u16,
        root: u8,
        bass: u8,
        suffix: String,
    },
    Barline {
        kind: u8,
    },
    Flow {
        time: f32,
        kind: u8,
    },
    Measure {
        children: Vec<Id>,
        start: f32,
        /// 2 for a grand staff.
        staves: u16,
    },
    /// The notes a beam joins.
    Beam(Vec<Id>),
    ConductorMeasure {
        children: Vec<Id>,
    },
    /// A tie or slur: the notes it connects.
    Span(Vec<Id>),
    Lyric {
        time: f32,
        staff: u16,
        text: String,
    },
}

#[derive(Debug, Clone)]
enum Entry {
    /// Indices the header uses up.
    Reserved,
    Class(String),
    Object(Id),
    /// Objects passed over by a skip.
    Skipped,
}

/// Malformed-input guard: no sample nests anywhere near this deep.
const MAX_DEPTH: usize = 64;

pub struct Archive<'a> {
    data: &'a [u8],
    pos: usize,
    version: u16,
    map: Vec<Entry>,
    pub nodes: Vec<Node>,
    /// Owner indices of the elements whose views are being read.
    parents: Vec<usize>,
    /// The nodes of the elements whose views are being read.
    owners: Vec<Id>,
    /// Where the index count stands since the last skip, until resynced.
    skip_at: Option<usize>,
    /// Skipped byte ranges, with the element they belong to.
    skipped: Vec<(Option<Id>, usize, usize)>,
    depth: usize,
}

fn err(message: impl Into<String>) -> ForteError {
    ForteError::Malformed(message.into())
}

impl<'a> Archive<'a> {
    /// Reads the whole document; the root is the conductor element.
    pub fn read(data: &'a [u8]) -> Result<(Archive<'a>, Id)> {
        if !data.starts_with(b"MMMF") || data.len() < 6 {
            return Err(ForteError::NotForte);
        }
        let version = u16::from_le_bytes([data[4], data[5]]);
        // The header (variable: newer files name the edition that saved
        // them) ends with the document's own element head, right before the
        // conductor class is defined.
        let conductor = find(data, b"\xff\xff\x02\x00\x0b\x00ECConductor", 0)
            .ok_or_else(|| err("no conductor"))?;
        let mut archive = Archive {
            data,
            pos: conductor
                .checked_sub(3)
                .ok_or_else(|| err("truncated header"))?,
            version,
            // The document object and its class come before the conductor.
            map: vec![Entry::Reserved; 3],
            nodes: Vec::new(),
            parents: vec![0],
            owners: Vec::new(),
            skip_at: None,
            skipped: Vec::new(),
            depth: 0,
        };
        // flags, view count, view number.
        let [_, count, _] = archive.array()?;
        if count == 0 {
            return Err(err("empty document"));
        }
        let root = archive.object()?.ok_or_else(|| err("no conductor"))?;
        Ok((archive, root))
    }

    /// The byte ranges skipped over (a staff's MIDI setup, page setup),
    /// with the element they were skipped in.
    pub fn skipped(&self) -> impl Iterator<Item = (Option<Id>, &'a [u8])> + '_ {
        self.skipped
            .iter()
            .map(|&(id, a, b)| (id, &self.data[a..b]))
    }

    pub fn node(&self, id: Id) -> &Node {
        &self.nodes[id]
    }

    // --- primitives -----------------------------------------------------

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or_else(|| err(format!("truncated at {:#x}", self.pos)))?;
        self.pos += n;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    fn peek_u16(&self) -> Option<u16> {
        self.data
            .get(self.pos..self.pos + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    /// A string with a one-byte length, in the Windows ANSI code page.
    fn string(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        Ok(decode_ansi(self.take(n)?))
    }

    // --- objects --------------------------------------------------------

    /// Reads an object: `None` for a null reference, otherwise the new or
    /// referenced object.
    fn object(&mut self) -> Result<Option<Id>> {
        let start = self.pos;
        let tag = self.u16()?;
        // A class reference or an object reference, by index.
        let (is_class, index) = match tag {
            0 => return Ok(None),
            // Big tag: a 32-bit index, the class flag in its top bit.
            0x7fff => {
                let big = self.u32()?;
                (big & 0x8000_0000 != 0, (big & 0x7fff_ffff) as usize)
            }
            0xffff => (true, usize::MAX),
            t => (t & 0x8000 != 0, (t & 0x7fff) as usize),
        };
        let class = if index == usize::MAX {
            let _schema = self.u16()?;
            let n = self.u16()? as usize;
            let name = String::from_utf8_lossy(self.take(n)?).into_owned();
            self.map.push(Entry::Class(name.clone()));
            name
        } else if is_class {
            match self.map.get(index) {
                Some(Entry::Class(name)) => name.clone(),
                _ => return Err(err(format!("bad class tag {tag:#x} at {start:#x}"))),
            }
        } else {
            return match self.map.get(index) {
                Some(Entry::Object(id)) => Ok(Some(*id)),
                _ => Err(err(format!("bad reference {index:#x} at {start:#x}"))),
            };
        };
        let id = self.nodes.len();
        let index = self.map.len();
        self.map.push(Entry::Object(id));
        self.nodes.push(Node {
            class: class.clone(),
            data: Data::None,
        });
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(err("objects nested too deeply"));
        }
        let data = self.object_body(&class, index)?;
        self.depth -= 1;
        self.nodes[id].data = data;
        Ok(Some(id))
    }

    fn object_body(&mut self, class: &str, index: usize) -> Result<Data> {
        if class.starts_with("EC") {
            return self.element(class, index);
        }
        if class.starts_with("DCsq") {
            return self.sequence(class);
        }
        if class.starts_with("DCsv") {
            return self.notation(class);
        }
        match class {
            "MCPosition" => self.skip(6)?,
            "MCPositionAndOffsets" => self.skip(14)?,
            "MCMeasurePositionAndWidth" => {
                self.skip(1)?;
                let n = self.u16()? as usize;
                self.skip(4 * n + 5)?;
            }
            "MCSplitPosition4" => {
                self.skip(1)?;
                let n = self.u16()? as usize;
                self.skip(32 * n + 5)?;
            }
            // A beam's notes (also listed in their measure) and envelope
            // event lists.
            "MCPositionBeam" => {
                self.skip(10)?;
                return Ok(Data::List(self.list16()?));
            }
            "MCEventList" => {
                self.list16()?;
            }
            "LCsqAccent" => self.skip(14)?,
            _ => return Err(err(format!("unknown class {class} at {:#x}", self.pos))),
        }
        Ok(Data::None)
    }

    /// `count: u16`, then that many objects.
    fn list16(&mut self) -> Result<Vec<Id>> {
        let n = self.u16()?;
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            if let Some(id) = self.object()? {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// A note's or rest's attached marks: `count: u8`, and if there are any,
    /// a pad byte, the objects and another pad byte.
    fn marks(&mut self) -> Result<Vec<Id>> {
        let n = self.u8()?;
        let mut out = Vec::new();
        if n > 0 {
            self.skip(1)?;
            for _ in 0..n {
                out.extend(self.object()?);
            }
            self.skip(1)?;
        }
        Ok(out)
    }

    // --- elements -------------------------------------------------------

    /// An element's views: `flags: u8, count: u8`, an extra byte if `flags`
    /// has bit 0, then `(view number, object)` pairs.
    fn views(&mut self, index: usize) -> Result<Vec<(u8, Id)>> {
        let flags = self.u8()?;
        let n = self.u8()?;
        if flags & 1 != 0 {
            self.skip(1)?;
        }
        self.parents.push(index);
        // The element's own node was the last one created before its views.
        self.owners.push(self.nodes.len() - 1);
        let mut views = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let k = self.u8()?;
            if let Some(id) = self.object()? {
                views.push((k, id));
            }
        }
        self.owners.pop();
        self.parents.pop();
        Ok(views)
    }

    fn element(&mut self, class: &str, index: usize) -> Result<Data> {
        let (views, extra) = match class {
            "ECConductor" => return self.conductor(index),
            "ECMeterEnv" | "ECTempoEnv" => (Vec::new(), Extra::Children(self.envelope()?)),
            _ => {
                let views = self.views(index)?;
                let extra = match class {
                    "ECConductorStaff" => Extra::Children(self.list16()?),
                    "ECStaff" => {
                        let mut children = Vec::new();
                        while let Some(id) = self.object()? {
                            children.push(id);
                        }
                        Extra::Children(children)
                    }
                    "ECTempo" => Extra::Tempo(self.tempo()?),
                    _ => Extra::None,
                };
                (views, extra)
            }
        };
        Ok(Data::Element { views, extra })
    }

    /// A meter or tempo envelope: its events (no view numbers), and a
    /// fixed block of envelope settings.
    fn envelope(&mut self) -> Result<Vec<Id>> {
        let _flags = self.u8()?;
        let n = self.u8()?;
        let mut events = Vec::with_capacity(n as usize);
        if n > 0 {
            self.skip(1)?;
        }
        for _ in 0..n {
            events.extend(self.object()?);
        }
        if n > 0 {
            self.skip(1)?;
        }
        self.skip(29)?;
        self.list16()?;
        Ok(events)
    }

    fn tempo(&mut self) -> Result<Tempo> {
        let time = self.f32()?;
        self.skip(2)?;
        let kind = self.u8()?;
        let mut values = [0.0; 5];
        for v in &mut values {
            *v = self.f32()?;
        }
        let flags = self.u8()?;
        let _text = self.string()?;
        let _ = self.f32()?;
        if flags & 0x10 != 0 {
            self.skip(5)?;
        }
        Ok(Tempo {
            time,
            values,
            kind,
            flags,
        })
    }

    /// The conductor: the conductor staff as its first view, then each
    /// staff. The staves aren't written as views: a block of layout data
    /// sits between them, which is skipped by finding the next staff.
    fn conductor(&mut self, index: usize) -> Result<Data> {
        let flags = self.u8()?;
        let n = self.u8()?;
        if flags & 1 != 0 {
            self.skip(1)?;
        }
        let k = self.u8()?;
        self.parents.push(index);
        self.owners.push(self.nodes.len() - 1);
        let conductor_staff = self.object()?.ok_or_else(|| err("no conductor staff"))?;
        self.owners.pop();
        self.parents.pop();
        let mut staves = Vec::new();
        for _ in 1..n {
            self.pos = self
                .find_staff()
                .ok_or_else(|| err(format!("staff {} not found", staves.len() + 1)))?;
            staves.extend(self.object()?);
        }
        Ok(Data::Element {
            views: vec![(k, conductor_staff)],
            extra: Extra::Staves(staves),
        })
    }

    fn class_index(&self, name: &str) -> Option<usize> {
        self.map
            .iter()
            .position(|e| matches!(e, Entry::Class(c) if c == name))
    }

    fn find_staff(&self) -> Option<usize> {
        let Some(staff) = self.class_index("ECStaff") else {
            return find(self.data, b"\xff\xff\x02\x00\x07\x00ECStaff", self.pos);
        };
        let tag = (0x8000 | staff as u16).to_le_bytes();
        let sequence = (0x8000 | self.class_index("DCsqStaff")? as u16).to_le_bytes();
        let mut from = self.pos;
        while let Some(i) = find(self.data, &tag, from) {
            // flags, count, [extra], view number 0, then the sequence view.
            let flags = *self.data.get(i + 2)?;
            let k = i + 4 + (flags & 1) as usize;
            if self.data.get(k) == Some(&0) && self.data.get(k + 1..k + 3) == Some(&sequence) {
                return Some(i);
            }
            from = i + 1;
        }
        None
    }

    // --- views ----------------------------------------------------------

    /// Checks a view's owner index against the element being read.
    fn owner(&mut self, owner: usize) -> Result<()> {
        self.resync(owner);
        let expected = *self.parents.last().unwrap_or(&0);
        if owner != expected {
            return Err(err(format!(
                "view at {:#x} belongs to object {owner:#x}, not {expected:#x}",
                self.pos
            )));
        }
        Ok(())
    }

    /// After a skip, the first view's owner index tells how many objects
    /// were passed over: that many indices are inserted where the skip
    /// began, shifting everything numbered since.
    fn resync(&mut self, owner: usize) {
        let Some(at) = self.skip_at.take() else {
            return;
        };
        let expected = *self.parents.last().unwrap_or(&0);
        if owner > expected {
            let delta = owner - expected;
            self.map
                .splice(at..at, std::iter::repeat_n(Entry::Skipped, delta));
            for p in &mut self.parents {
                if *p >= at {
                    *p += delta;
                }
            }
        }
    }

    fn sequence(&mut self, class: &str) -> Result<Data> {
        self.skip(4)?;
        let count = self.u32()?;
        let owner = self.u16()? as usize;
        self.owner(owner)?;
        match class {
            "DCsqNote" => self.skip(8 * count as usize)?,
            "DCsqConductorStaff" | "DCsqStaff" => {
                self.skip(4 + 52)?;
                self.skip_to_view(2)?;
            }
            "DCsqAccent" => {
                self.object()?;
            }
            _ => {}
        }
        Ok(Data::Sequence)
    }

    /// Skips opaque data up to view `k` of the element being read, found by
    /// its owner index.
    fn skip_to_view(&mut self, k: u8) -> Result<()> {
        let owner = *self.parents.last().unwrap_or(&0) as u16;
        let mut from = self.pos;
        loop {
            let o = self.data[from..]
                .iter()
                .position(|&b| b == k)
                .map(|i| from + i)
                .ok_or_else(|| err("skip: view not found"))?;
            let tag = self
                .data
                .get(o + 1..o + 3)
                .map(|b| u16::from_le_bytes([b[0], b[1]]));
            let head = match tag {
                Some(0xffff) => {
                    let n = self
                        .data
                        .get(o + 5..o + 7)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
                        .unwrap_or(usize::MAX);
                    let name = self.data.get(o + 7..o + 7 + n.min(40));
                    (n < 40 && name.is_some_and(|n| n.starts_with(b"DCs"))).then(|| o + 7 + n)
                }
                Some(t) if t & 0x8000 != 0 => match self.map.get((t & 0x7fff) as usize) {
                    Some(Entry::Class(c)) if c.starts_with("DCs") => Some(o + 3),
                    _ => None,
                },
                _ => None,
            };
            let matches = head.is_some_and(|q| {
                self.data
                    .get(q + 8..q + 10)
                    .is_some_and(|b| u16::from_le_bytes([b[0], b[1]]) == owner)
            });
            if matches {
                self.skipped
                    .push((self.owners.last().copied(), self.pos, o));
                self.pos = o;
                self.skip_at = Some(self.map.len());
                return Ok(());
            }
            from = o + 1;
        }
    }

    /// `flags`-dependent prefix of a notation view: a position object, or
    /// for the two container views an inline position.
    fn notation(&mut self, class: &str) -> Result<Data> {
        self.skip(4)?;
        let svflags = self.u32()?;
        let owner = self.u16()? as usize;
        if class == "DCsvConductorStaff" {
            // Page and font setup up to the first envelope.
            let i = find(self.data, b"\x0a\x00ECMeterEnv", self.pos)
                .ok_or_else(|| err("no meter envelope"))?;
            self.skipped
                .push((self.owners.last().copied(), self.pos, i - 6));
            self.pos = i - 6;
            return Ok(Data::View(View::Other));
        }
        if class == "DCsvStaff" {
            // Comes right after the staff's skipped MIDI setup, but belongs
            // to the staff read before it: the index count is corrected at
            // the next view instead.
            if Some(&owner) != self.parents.last() {
                return Err(err(format!(
                    "staff view at {:#x} of another staff",
                    self.pos
                )));
            }
        } else {
            self.owner(owner)?;
        }
        let position = if matches!(class, "DCsvStaff" | "DCsvChord") {
            self.skip(5)?;
            None
        } else {
            self.object()?
        };
        let view = match class {
            "DCsvNote" => {
                let body = self.array()?;
                self.skip(1)?;
                let children = self.marks()?;
                let time = self.f32()?;
                let staff = self.u8()?;
                self.skip(2 + 4 + 8)?;
                View::Note {
                    body,
                    children,
                    time,
                    staff,
                }
            }
            "DCsvRest" => {
                let body = self.array()?;
                let children = self.marks()?;
                let time = self.f32()?;
                let staff = self.u8()?;
                self.skip(1)?;
                View::Rest {
                    body,
                    children,
                    time,
                    staff,
                }
            }
            "DCsvChord" => {
                let body = self.array()?;
                let notes = self.list16()?;
                let (time, staff) = self.trailer()?;
                View::Chord {
                    body,
                    notes,
                    time,
                    staff,
                }
            }
            "DCsvTuplet" => {
                self.skip(6)?;
                let members = self.list16()?;
                self.trailer()?;
                View::Tuplet {
                    members,
                    ratio: self.array()?,
                }
            }
            "DCsvDot" | "DCsvAccidental" | "DCsvAccent" => View::Mark(self.u8()?),
            "DCsvKey" => View::Key(self.array()?),
            "DCsvClef" => View::Clef(self.array()?),
            "DCsvMirror" => {
                self.skip(7)?;
                View::Other
            }
            "DCsvBeam" => {
                self.skip(8)?;
                match position.map(|p| &self.nodes[p].data) {
                    Some(Data::List(notes)) => View::Beam(notes.clone()),
                    _ => View::Beam(Vec::new()),
                }
            }
            "DCsvMeter" => self.meter(svflags)?,
            "DCsvTempo" => {
                self.skip(1)?;
                View::Other
            }
            "DCsvText" => {
                self.skip(4 + 3)?;
                let kind = self.u8()?;
                let text = self.string()?;
                self.skip(8)?;
                self.string()?;
                View::Text { kind, text }
            }
            "DCsvLyric" => {
                self.skip(2)?;
                let (time, staff) = self.trailer()?;
                self.skip(1)?;
                let text = self.string()?;
                self.skip(8)?;
                self.string()?;
                self.skip(6)?;
                View::Lyric { time, staff, text }
            }
            "DCsvChordSymbol" => {
                let (time, staff) = self.trailer()?;
                let [root, bass, _, _] = self.array()?;
                let suffix = self.string()?;
                View::ChordSymbol {
                    time,
                    staff,
                    root,
                    bass,
                    suffix,
                }
            }
            "DCsvConductorMeasure" => {
                self.skip(4)?;
                View::ConductorMeasure {
                    children: self.list16()?,
                }
            }
            "DCsvBarline" => {
                self.skip(2)?;
                let kind = self.u8()?;
                if kind & 0x40 != 0 {
                    self.skip(1 + 4 + 2 + 4)?;
                }
                View::Barline { kind }
            }
            "DCsvFlowControl" => {
                let (time, _) = self.trailer()?;
                View::Flow {
                    time,
                    kind: self.u8()?,
                }
            }
            "DCsvBrac" => {
                self.skip(1)?;
                View::Other
            }
            "DCsvStaff" => {
                self.skip(1)?;
                if self.version >= VERSION_10 {
                    self.skip(1)?;
                }
                self.skip(4)?;
                // Further numbers until the first child element.
                while self.peek_u16().is_some_and(|t| t & 0x8000 == 0) {
                    self.skip(2)?;
                }
                View::Other
            }
            "DCsvMeasure" => {
                let staves = self.u16()?;
                self.skip(staves as usize + 1 + 6)?;
                let children = self.list16()?;
                let (start, _) = self.trailer()?;
                View::Measure {
                    children,
                    start,
                    staves,
                }
            }
            "DCsvTie" => {
                let notes = self.list16()?;
                self.trailer()?;
                self.skip(1)?;
                View::Span(notes)
            }
            "DCsvSlur" => {
                self.skip(2)?;
                let notes = self.list16()?;
                self.trailer()?;
                self.skip(1)?;
                View::Span(notes)
            }
            _ => return Err(err(format!("unknown class {class} at {:#x}", self.pos))),
        };
        Ok(Data::View(view))
    }

    /// The common tail of many views: `u8, time: f32, staff: u16` (staff
    /// number in the high nibble, bit 2 for a grand staff's lower staff).
    fn trailer(&mut self) -> Result<(f32, u16)> {
        self.skip(1)?;
        let time = self.f32()?;
        Ok((time, self.u16()?))
    }

    /// A time signature. Version 10 files drop two leading bytes; a pickup
    /// measure adds the time signature shown in front.
    fn meter(&mut self, svflags: u32) -> Result<View> {
        let pickup = svflags & 0x10_0000 != 0;
        let body = self.take(45 + if pickup { 3 } else { 0 })?;
        let mut at = if pickup { 3 } else { 0 };
        if self.version < VERSION_10 {
            at += 2;
        }
        let time = f32::from_le_bytes(body[at + 1..at + 5].try_into().unwrap());
        let fields = at + 5;
        Ok(View::Meter {
            time,
            beats: body[fields + 8],
            beat_type: body[fields + 9],
            shown: pickup.then(|| (body[0], body[1])),
        })
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// Windows-1252, which Forte writes its texts in.
pub fn decode_ansi(bytes: &[u8]) -> String {
    encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}
