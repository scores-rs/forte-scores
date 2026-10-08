//! Notes (`ECNote`), rests (`ECRest`), chords (`ECChord`) and tuplets
//! (`ECTuplet`), and what's attached to notes: dots (`ECDot`),
//! accidentals (`ECAccidental`) and articulations (`ECAccent`).
//!
//! Every timed element knows its own start (quarter notes from the start of
//! the score) and which staff of a grand staff it's on.

/// Identifies a note for the ties and slurs that refer to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteId(pub usize);

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: NoteId,
    pub time: f32,
    pub lower: bool,
    /// The MIDI pitch played.
    pub midi: u8,
    /// The diatonic step it's written on, C = 0.
    pub step: u8,
    /// The note value's code (see [`NoteValue::from_code`]). Chord
    /// members' own codes aren't reliable: the chord's applies.
    pub value: u8,
    pub dots: u8,
    /// A written accidental.
    pub accidental: Option<Accidental>,
    pub articulations: Vec<Articulation>,
}

impl Note {
    pub fn note_value(&self) -> Option<NoteValue> {
        NoteValue::from_code(self.value)
    }

    /// The written pitch as `(step, alter, octave)` (C4 is middle C), from
    /// the MIDI pitch and the step: `None` if they're more than a double
    /// accidental apart.
    pub fn spelling(&self) -> Option<(u8, i8, i8)> {
        const NATURAL: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
        let natural = *NATURAL.get(self.step as usize)?;
        let midi = self.midi as i32;
        let octave = ((midi - natural) as f64 / 12.0).round() as i32 - 1;
        let alter = midi - (12 * (octave + 1) + natural);
        (alter.abs() <= 2).then_some((self.step, alter as i8, octave as i8))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rest {
    pub time: f32,
    pub lower: bool,
    /// The note value's code. A whole rest also stands for a full measure
    /// in any time signature.
    pub value: u8,
    pub dots: u8,
}

impl Rest {
    pub fn note_value(&self) -> Option<NoteValue> {
        NoteValue::from_code(self.value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub time: f32,
    pub lower: bool,
    /// The note value's code, for all its notes.
    pub value: u8,
    pub notes: Vec<Note>,
}

impl Chord {
    pub fn note_value(&self) -> Option<NoteValue> {
        NoteValue::from_code(self.value)
    }

    /// The chord's dots (members can list theirs more than once).
    pub fn dots(&self) -> u8 {
        self.notes.first().map_or(0, |n| n.dots)
    }
}

/// `actual` notes in the time of `normal` ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuplet {
    pub actual: u8,
    pub normal: u8,
    /// Notes, rests and chords.
    pub elements: Vec<TupletElement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TupletElement {
    Note(Note),
    Rest(Rest),
    Chord(Chord),
}

/// Forte's note values, by code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteValue {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    SixtyFourth,
    HundredTwentyEighth,
}

impl NoteValue {
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => NoteValue::Whole,
            2 => NoteValue::Half,
            3 => NoteValue::Quarter,
            4 => NoteValue::Eighth,
            5 => NoteValue::Sixteenth,
            6 => NoteValue::ThirtySecond,
            7 => NoteValue::SixtyFourth,
            8 => NoteValue::HundredTwentyEighth,
            _ => return None,
        })
    }

    /// Length in quarter notes.
    pub fn quarters(self) -> f64 {
        4.0 / (1u32 << (self as u32)) as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accidental {
    Sharp,
    Flat,
    DoubleSharp,
    DoubleFlat,
    Natural,
    Other(u8),
}

impl Accidental {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0 => Accidental::Sharp,
            1 => Accidental::Flat,
            2 => Accidental::DoubleSharp,
            3 => Accidental::DoubleFlat,
            4 => Accidental::Natural,
            other => Accidental::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Articulation {
    Staccato,
    Accent,
    Other(u8),
}

impl Articulation {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0 => Articulation::Staccato,
            2 => Articulation::Accent,
            other => Articulation::Other(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(midi: u8, step: u8) -> Note {
        Note {
            id: NoteId(0),
            time: 0.0,
            lower: false,
            midi,
            step,
            value: 3,
            dots: 0,
            accidental: None,
            articulations: Vec::new(),
        }
    }

    #[test]
    fn spells_from_step_and_midi() {
        assert_eq!(note(60, 0).spelling(), Some((0, 0, 4)));
        assert_eq!(note(61, 1).spelling(), Some((1, -1, 4)));
        assert_eq!(note(42, 3).spelling(), Some((3, 1, 2)));
        // B sharp is spelled in the octave below its C.
        assert_eq!(note(60, 6).spelling(), Some((6, 1, 3)));
        assert_eq!(note(59, 0).spelling(), Some((0, -1, 4)));
        assert_eq!(note(66, 0).spelling(), None);
    }

    #[test]
    fn note_values_halve() {
        assert_eq!(NoteValue::from_code(1).unwrap().quarters(), 4.0);
        assert_eq!(NoteValue::from_code(4).unwrap().quarters(), 0.5);
        assert_eq!(NoteValue::from_code(8).unwrap().quarters(), 0.03125);
        assert_eq!(NoteValue::from_code(9), None);
    }
}
