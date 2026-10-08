//! A staff (`ECStaff`): its measures (`ECMeasure`) and their contents,
//! and what spans measures - ties, slurs and dynamics.
//!
//! A piano part is one Forte staff with two staves (a grand staff); each
//! element says which one it's on with its `lower` flag.

use crate::conductor::Text;
use crate::note::{Chord, Note, NoteId, Rest, Tuplet};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Staff {
    /// Its texts; the name is the [`TextKind::StaffName`] one.
    ///
    /// [`TextKind::StaffName`]: crate::conductor::TextKind::StaffName
    pub texts: Vec<Text>,
    pub measures: Vec<Measure>,
    pub ties: Vec<Span>,
    pub slurs: Vec<Span>,
    /// Kept with the staff's MIDI setup (its volume envelope) rather than
    /// its measures.
    pub dynamics: Vec<Dynamic>,
}

impl Staff {
    pub fn name(&self) -> Option<&str> {
        self.texts
            .iter()
            .find(|t| t.kind == crate::conductor::TextKind::StaffName)
            .map(|t| t.text.as_str())
    }

    /// Whether it's a grand staff.
    pub fn is_grand_staff(&self) -> bool {
        self.measures.iter().any(|m| m.staves > 1)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measure {
    /// Quarter notes from the start of the score.
    pub start: f32,
    /// 2 for a grand staff.
    pub staves: u16,
    /// In the order Forte lists them, each once (a beam's notes are only
    /// listed here, not in the beam).
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    Note(Note),
    Rest(Rest),
    Chord(Chord),
    Tuplet(Tuplet),
    Clef(Clef),
    Key(Key),
    ChordSymbol(ChordSymbol),
    Lyric(Lyric),
    /// The notes a beam joins.
    Beam(Vec<NoteId>),
}

/// A clef (`ECClef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clef {
    pub kind: ClefKind,
    pub lower: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClefKind {
    Treble,
    Bass,
    Other(u8),
}

impl ClefKind {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0 => ClefKind::Treble,
            3 => ClefKind::Bass,
            other => ClefKind::Other(other),
        }
    }
}

/// A key signature (`ECKey`). Forte only stores the accidentals, not the
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// Sharps (positive) or flats (negative).
    pub fifths: i8,
    pub lower: bool,
}

/// A chord symbol (`ECChordSymbol`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChordSymbol {
    pub time: f32,
    pub lower: bool,
    pub root: ChordRoot,
    /// A slash chord's bass.
    pub bass: Option<ChordRoot>,
    /// The quality as shown after the root ("m", "7", ...).
    pub suffix: String,
}

/// A chord root or bass: a step (C = 0) and its alteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordRoot {
    pub step: u8,
    pub alter: i8,
}

impl ChordRoot {
    /// Forte numbers roots three per step - flat, natural, sharp - from C
    /// flat = 0.
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        (code < 21).then(|| ChordRoot {
            step: code / 3,
            alter: (code % 3) as i8 - 1,
        })
    }
}

/// A lyric syllable (`ECLyric`).
#[derive(Debug, Clone, PartialEq)]
pub struct Lyric {
    pub time: f32,
    pub lower: bool,
    pub text: String,
}

/// A tie or slur (`ECTie`, `ECSlur`): the notes it connects, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub notes: Vec<NoteId>,
}

/// A dynamic mark (`ECDynamic`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dynamic {
    pub time: f32,
    pub lower: bool,
    pub mark: DynamicMark,
}

/// Forte's dynamics, softest to loudest; their playback velocities step by
/// 14 (mf 71, f 85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicMark {
    Ppp,
    Pp,
    P,
    Mp,
    Mf,
    F,
    Ff,
    Fff,
    Other(u8),
}

impl DynamicMark {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            1 => DynamicMark::Ppp,
            2 => DynamicMark::Pp,
            3 => DynamicMark::P,
            4 => DynamicMark::Mp,
            5 => DynamicMark::Mf,
            6 => DynamicMark::F,
            7 => DynamicMark::Ff,
            8 => DynamicMark::Fff,
            other => DynamicMark::Other(other),
        }
    }
}
