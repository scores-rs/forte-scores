//! Builds the [`Document`] from the archive's object tree.

use crate::archive::{Archive, Data, Extra, Id, View, decode_ansi};
use crate::conductor::{
    Barline, Conductor, ConductorMeasure, ConductorStaff, FlowControl, FlowKind, Meter, Tempo,
    Text, TextKind,
};
use crate::error::{ForteError, Result};
use crate::note::{Accidental, Articulation, Chord, Note, NoteId, Rest, Tuplet, TupletElement};
use crate::staff::{
    ChordRoot, ChordSymbol, Clef, ClefKind, Dynamic, DynamicMark, Element, Key, Lyric, Measure,
    Span, Staff,
};
use std::collections::HashSet;

/// A Forte document: the `Contents` stream decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The MFC file version: 8 for older Forte versions, 10 for Forte 4.
    pub version: u16,
    /// The edition that saved it ("S2-LV-FNOTE-4-FREE-2"), in version 10
    /// files.
    pub edition: Option<String>,
    /// The program version that saved it ("4.1.3"), in version 10 files.
    pub program_version: Option<String>,
    pub conductor: Conductor,
}

impl Document {
    /// Decodes a `Contents` stream.
    pub fn from_contents(contents: &[u8]) -> Result<Self> {
        let (archive, root) = Archive::read(contents)?;
        let version = u16::from_le_bytes([contents[4], contents[5]]);
        let (edition, program_version) = header_strings(contents, version);
        Ok(Self {
            version,
            edition,
            program_version,
            conductor: Builder { archive: &archive }.conductor(root)?,
        })
    }

    /// The first text of a kind in the conductor.
    pub fn text(&self, kind: TextKind) -> Option<&str> {
        self.conductor
            .staff
            .texts
            .iter()
            .find(|t| t.kind == kind)
            .map(|t| t.text.as_str())
    }

    pub fn title(&self) -> Option<&str> {
        self.text(TextKind::Title)
    }

    pub fn composer(&self) -> Option<&str> {
        self.text(TextKind::Composer)
    }
}

/// Version 10 headers name the edition and program version that saved the
/// file, as two length-prefixed strings at offset 0x20.
fn header_strings(contents: &[u8], version: u16) -> (Option<String>, Option<String>) {
    if version < 10 {
        return (None, None);
    }
    let string = |at: usize| -> Option<(String, usize)> {
        let n = *contents.get(at)? as usize;
        let bytes = contents.get(at + 1..at + 1 + n)?;
        (n > 0 && bytes.iter().all(|b| b.is_ascii_graphic()))
            .then(|| (decode_ansi(bytes), at + 1 + n))
    };
    let Some((edition, next)) = string(0x20) else {
        return (None, None);
    };
    (Some(edition), string(next).map(|(v, _)| v))
}

struct Builder<'a, 'b> {
    archive: &'b Archive<'a>,
}

fn err(message: impl Into<String>) -> ForteError {
    ForteError::Malformed(message.into())
}

/// Bit 2 of a staff field: the lower staff of a grand staff.
fn lower(staff: u16) -> bool {
    (staff >> 2) & 1 == 1
}

impl Builder<'_, '_> {
    fn class(&self, id: Id) -> &str {
        &self.archive.node(id).class
    }

    fn element(&self, id: Id) -> Option<(&[(u8, Id)], &Extra)> {
        match &self.archive.node(id).data {
            Data::Element { views, extra } => Some((views, extra)),
            _ => None,
        }
    }

    /// An element's notation view.
    fn view(&self, id: Id) -> Option<&View> {
        let (views, _) = self.element(id)?;
        let &(_, view) = views.iter().find(|(k, _)| *k == 2)?;
        match &self.archive.node(view).data {
            Data::View(view) => Some(view),
            _ => None,
        }
    }

    fn children(&self, id: Id) -> &[Id] {
        match self.element(id) {
            Some((_, Extra::Children(children))) => children,
            _ => &[],
        }
    }

    fn text(&self, id: Id) -> Option<Text> {
        match self.view(id)? {
            View::Text { kind, text } => Some(Text {
                kind: TextKind::from_code(*kind),
                text: text.clone(),
            }),
            _ => None,
        }
    }

    fn conductor(&self, root: Id) -> Result<Conductor> {
        let (views, extra) = self.element(root).ok_or_else(|| err("no conductor"))?;
        let conductor_staff = views
            .first()
            .map(|&(_, id)| id)
            .ok_or_else(|| err("no conductor staff"))?;
        let mut staff = ConductorStaff::default();
        for &child in self.children(conductor_staff) {
            match self.class(child) {
                "ECMeterEnv" => staff.meters.extend(self.meters(child)),
                "ECTempoEnv" => staff.tempos.extend(self.tempos(child)),
                "ECText" => staff.texts.extend(self.text(child)),
                "ECConductorMeasure" => staff.measures.push(self.conductor_measure(child)),
                "ECFlowControl" => {
                    if let Some(&View::Flow { time, kind }) = self.view(child) {
                        staff.flow_controls.push(FlowControl {
                            time,
                            kind: FlowKind::from_code(kind),
                        });
                    }
                }
                _ => {}
            }
        }
        staff.meters.sort_by(|a, b| a.time.total_cmp(&b.time));
        let Extra::Staves(ids) = extra else {
            return Err(err("no staves"));
        };
        let staves = ids
            .iter()
            .enumerate()
            .map(|(number, &id)| self.staff(id, number))
            .collect::<Result<Vec<_>>>()?;
        if staves.is_empty() {
            return Err(err("no staves"));
        }
        Ok(Conductor { staff, staves })
    }

    fn meters(&self, envelope: Id) -> Vec<Meter> {
        self.children(envelope)
            .iter()
            .filter_map(|&m| match *self.view(m)? {
                View::Meter {
                    time,
                    beats,
                    beat_type,
                    shown,
                } if beats > 0 && beat_type.is_power_of_two() => Some(Meter {
                    time,
                    beats,
                    beat_type,
                    shown,
                }),
                _ => None,
            })
            .collect()
    }

    fn tempos(&self, envelope: Id) -> Vec<Tempo> {
        self.children(envelope)
            .iter()
            .filter_map(|&t| match self.element(t)? {
                (_, Extra::Tempo(tempo)) => Some(Tempo {
                    time: tempo.time,
                    bpm: tempo.values[1],
                    fermata: tempo.flags & 0x10 != 0,
                    fermata_start: tempo.flags & 0x10 != 0 && tempo.kind & 0x02 != 0,
                }),
                _ => None,
            })
            .collect()
    }

    fn conductor_measure(&self, measure: Id) -> ConductorMeasure {
        let Some(View::ConductorMeasure { children }) = self.view(measure) else {
            return ConductorMeasure::default();
        };
        let barlines = children
            .iter()
            .filter_map(|&c| match *self.view(c)? {
                View::Barline { kind } => Some(Barline::from_code(kind)),
                _ => None,
            })
            .collect();
        ConductorMeasure { barlines }
    }

    fn staff(&self, id: Id, number: usize) -> Result<Staff> {
        let mut staff = Staff::default();
        for &child in self.children(id) {
            match self.class(child) {
                "ECText" => staff.texts.extend(self.text(child)),
                "ECMeasure" => {
                    let Some(View::Measure {
                        children,
                        start,
                        staves,
                    }) = self.view(child)
                    else {
                        continue;
                    };
                    let mut seen = HashSet::new();
                    let mut elements = Vec::new();
                    for &item in children {
                        self.item(item, &mut elements, &mut seen)?;
                    }
                    staff.measures.push(Measure {
                        start: *start,
                        staves: *staves,
                        elements,
                    });
                }
                "ECTie" | "ECSlur" => {
                    if let Some(View::Span(notes)) = self.view(child) {
                        let span = Span {
                            notes: notes.iter().map(|&n| NoteId(n)).collect(),
                        };
                        if self.class(child) == "ECTie" {
                            staff.ties.push(span);
                        } else {
                            staff.slurs.push(span);
                        }
                    }
                }
                _ => {}
            }
        }
        if staff.measures.is_empty() {
            return Err(err(format!("staff {} has no measures", number + 1)));
        }
        staff.dynamics = self.dynamics(id);
        Ok(staff)
    }

    /// A measure's child. `seen` keeps elements listed twice (a beam's notes
    /// are also listed on their own, a chord's notes may be too) from being
    /// read twice.
    fn item(&self, id: Id, out: &mut Vec<Element>, seen: &mut HashSet<Id>) -> Result<()> {
        if !seen.insert(id) {
            return Ok(());
        }
        let Some(view) = self.view(id) else {
            return Ok(());
        };
        let element = match view {
            View::Note { .. } => Element::Note(self.note(id)?),
            View::Rest { .. } => Element::Rest(self.rest(id)?),
            View::Chord { .. } => Element::Chord(self.chord(id, seen)?),
            View::Tuplet { members, ratio } => {
                let mut elements = Vec::new();
                for &member in members {
                    if !seen.insert(member) {
                        continue;
                    }
                    elements.push(match self.view(member) {
                        Some(View::Note { .. }) => TupletElement::Note(self.note(member)?),
                        Some(View::Rest { .. }) => TupletElement::Rest(self.rest(member)?),
                        Some(View::Chord { .. }) => TupletElement::Chord(self.chord(member, seen)?),
                        _ => continue,
                    });
                }
                Element::Tuplet(Tuplet {
                    actual: ratio[0],
                    normal: ratio[1],
                    elements,
                })
            }
            View::Clef(body) => Element::Clef(Clef {
                kind: ClefKind::from_code(body[7] & 0x7f),
                lower: body[7] & 0x80 != 0,
            }),
            View::Key(body) => {
                // Codes count down from 8 (C major): 7 is one sharp.
                Element::Key(Key {
                    fifths: (8 - body[0] as i32).clamp(-7, 7) as i8,
                    lower: body[11] & 0x80 != 0,
                })
            }
            View::ChordSymbol {
                time,
                staff,
                root,
                bass,
                suffix,
            } => {
                let Some(root) = ChordRoot::from_code(*root) else {
                    return Ok(());
                };
                Element::ChordSymbol(ChordSymbol {
                    time: *time,
                    lower: lower(*staff),
                    root,
                    bass: (*bass > 0)
                        .then(|| ChordRoot::from_code(*bass))
                        .flatten()
                        .filter(|b| *b != root),
                    suffix: suffix.clone(),
                })
            }
            View::Lyric { time, staff, text } => Element::Lyric(Lyric {
                time: *time,
                lower: lower(*staff),
                text: text.clone(),
            }),
            View::Beam(notes) => Element::Beam(notes.iter().map(|&n| NoteId(n)).collect()),
            _ => return Ok(()),
        };
        out.push(element);
        Ok(())
    }

    fn note(&self, id: Id) -> Result<Note> {
        let Some(View::Note {
            body,
            children,
            time,
            staff,
        }) = self.view(id)
        else {
            return Err(err(format!("{} where a note belongs", self.class(id))));
        };
        Ok(Note {
            id: NoteId(id),
            time: *time,
            lower: lower(*staff as u16),
            midi: body[2],
            step: (body[0] >> 4) & 7,
            value: body[6],
            dots: self.dots(children),
            accidental: self
                .marks(children, "ECAccidental")
                .next()
                .map(Accidental::from_code),
            articulations: self
                .marks(children, "ECAccent")
                .map(Articulation::from_code)
                .collect(),
        })
    }

    fn rest(&self, id: Id) -> Result<Rest> {
        let Some(View::Rest {
            body,
            children,
            time,
            staff,
        }) = self.view(id)
        else {
            return Err(err(format!("{} where a rest belongs", self.class(id))));
        };
        Ok(Rest {
            time: *time,
            lower: lower(*staff as u16),
            value: body[6],
            dots: self.dots(children),
        })
    }

    fn chord(&self, id: Id, seen: &mut HashSet<Id>) -> Result<Chord> {
        let Some(View::Chord {
            body,
            notes,
            time,
            staff,
        }) = self.view(id)
        else {
            return Err(err(format!("{} where a chord belongs", self.class(id))));
        };
        let notes = notes
            .iter()
            .map(|&n| {
                seen.insert(n);
                self.note(n)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Chord {
            time: *time,
            lower: lower(*staff),
            value: body[6],
            notes,
        })
    }

    /// The values of a note's or rest's attached marks of one class.
    fn marks<'c>(&'c self, children: &'c [Id], class: &'c str) -> impl Iterator<Item = u8> + 'c {
        children
            .iter()
            .filter(move |&&c| self.class(c) == class)
            .filter_map(|&c| match *self.view(c)? {
                View::Mark(value) => Some(value),
                _ => None,
            })
    }

    /// Dots; chord members can list theirs more than once.
    fn dots(&self, children: &[Id]) -> u8 {
        self.marks(children, "ECDot").max().unwrap_or(0)
    }

    /// Dynamics are only kept with the staff's MIDI setup, in the volume
    /// controller envelope the archive reader skips: they're picked out of
    /// the skipped bytes by the shape of their notation view.
    fn dynamics(&self, staff: Id) -> Vec<Dynamic> {
        let mut out = Vec::new();
        for (owner, bytes) in self.archive.skipped() {
            if owner != Some(staff) {
                continue;
            }
            let mut i = 0;
            while let Some(at) = bytes[i..]
                .windows(4)
                .position(|w| w == [0xff, 0x19, 0x20, 0x00])
            {
                let start = i + at;
                out.extend(dynamic_view(&bytes[start + 4..]));
                i = start + 1;
            }
        }
        out
    }
}

/// A dynamic's notation view after its flags: owner, position, then
/// `u16, time: f32, staff: u16, 3, velocity: f32, 0, code`.
fn dynamic_view(b: &[u8]) -> Option<Dynamic> {
    let tag = u16::from_le_bytes([*b.get(2)?, *b.get(3)?]);
    let at = match tag {
        0xffff if b.get(8..18)? == b"MCPosition" => 18 + 6,
        t if t & 0x8000 != 0 && t != 0xffff => 4 + 6,
        _ => return None,
    };
    let f =
        |o: usize| -> Option<f32> { Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    let time = f(at + 2)?;
    let staff = u16::from_le_bytes([*b.get(at + 6)?, *b.get(at + 7)?]);
    let velocity = f(at + 9)?;
    let code = *b.get(at + 14)?;
    let plausible = *b.get(at + 8)? == 3
        && *b.get(at + 13)? == 0
        && time.is_finite()
        && (0.0..1e6).contains(&time)
        && (0.0..=127.0).contains(&velocity)
        && (1..=10).contains(&code);
    plausible.then_some(Dynamic {
        time,
        lower: lower(staff),
        mark: DynamicMark::from_code(code),
    })
}
