//! The conductor (`ECConductor`): the score's staves, and the conductor
//! staff (`ECConductorStaff`) holding what all staves share - time
//! signatures, tempos, texts, measures with their barlines, and repeats.
//!
//! Times are in quarter notes from the start of the score, as Forte keeps
//! them.

use crate::staff::Staff;

/// The score.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conductor {
    pub staff: ConductorStaff,
    pub staves: Vec<Staff>,
}

/// What all staves share.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConductorStaff {
    /// Time signature changes (`ECMeterEnv`), in time order.
    pub meters: Vec<Meter>,
    /// Tempo changes (`ECTempoEnv`), including the ones Forte inserts to
    /// play fermatas.
    pub tempos: Vec<Tempo>,
    /// Header and footer texts: title, composer, copyright.
    pub texts: Vec<Text>,
    pub measures: Vec<ConductorMeasure>,
    /// Repeat signs (`ECFlowControl`).
    pub flow_controls: Vec<FlowControl>,
}

/// A time signature (`ECMeter`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    pub time: f32,
    pub beats: u8,
    pub beat_type: u8,
    /// The time signature shown, when it differs from the one counted: a
    /// pickup measure counts its own length but shows the full measure's.
    pub shown: Option<(u8, u8)>,
}

/// A tempo change (`ECTempo`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub time: f32,
    /// Quarter notes per minute.
    pub bpm: f32,
    /// One of the changes Forte inserts to play a fermata (a slowdown at
    /// the fermata, then a change back), not a tempo mark.
    pub fermata: bool,
    /// Where a fermata's slowdown starts.
    pub fermata_start: bool,
}

/// A text block (`ECText`), in the conductor or on a staff.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub kind: TextKind,
    /// The text, with Windows line breaks (`\r\n`).
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Copyright,
    /// A staff's name.
    StaffName,
    Title,
    /// Composer and lyricist ("Musik u. Text: ...").
    Composer,
    Other(u8),
}

impl TextKind {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0x01 => TextKind::Copyright,
            0x03 => TextKind::StaffName,
            0x08 => TextKind::Title,
            0x0a => TextKind::Composer,
            other => TextKind::Other(other),
        }
    }
}

/// A measure's conductor part (`ECConductorMeasure`): its barlines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConductorMeasure {
    pub barlines: Vec<Barline>,
}

impl ConductorMeasure {
    /// The barline closing the measure (the others open a system).
    pub fn closing_barline(&self) -> Option<Barline> {
        self.barlines
            .iter()
            .rev()
            .copied()
            .find(|b| !matches!(b, Barline::SystemStart))
    }
}

/// A barline (`ECBarline`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Barline {
    Regular,
    Double,
    Final,
    /// The barline at the start of a system (before a brace).
    SystemStart,
    Other(u8),
}

impl Barline {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0x40 => Barline::Regular,
            0x41 => Barline::Double,
            0x43 => Barline::Final,
            c if c & 0x40 == 0 => Barline::SystemStart,
            other => Barline::Other(other),
        }
    }
}

/// A repeat sign (`ECFlowControl`). It can sit inside a measure: Forte
/// puts one there to complete a pickup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowControl {
    pub time: f32,
    pub kind: FlowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    RepeatStart,
    RepeatEnd,
    Other(u8),
}

impl FlowKind {
    pub(crate) fn from_code(code: u8) -> Self {
        match code {
            0 => FlowKind::RepeatStart,
            1 => FlowKind::RepeatEnd,
            other => FlowKind::Other(other),
        }
    }
}
