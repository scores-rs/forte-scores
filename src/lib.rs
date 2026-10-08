//! Read Forte `.fnf` score files.
//!
//! `.fnf` is the native, undocumented file format of Forte, Lugert Verlag's
//! notation program (discontinued in 2025). This crate reads a document's
//! musical content: staves, measures, notes, rests, chords, tuplets and
//! what's attached to them, and the conductor's time signatures, tempos,
//! texts, barlines and repeat signs. The format was reverse-engineered from
//! files saved by Forte 4 (Free and Premium) and older versions, and checked
//! against Forte's own MIDI exports of them.
//!
//! # Reading
//!
//! ```no_run
//! use forte_scores::FnfFile;
//! use forte_scores::staff::Element;
//!
//! let file = FnfFile::open("song.fnf")?;
//! let doc = &file.document;
//! println!("{:?}", doc.title());
//! for staff in &doc.conductor.staves {
//!     for measure in &staff.measures {
//!         for element in &measure.elements {
//!             if let Element::Note(note) = element {
//!                 println!("{}: MIDI pitch {}", note.time, note.midi);
//!             }
//!         }
//!     }
//! }
//! # Ok::<(), forte_scores::ForteError>(())
//! ```
//!
//! # The format
//!
//! A `.fnf` file is an OLE compound file whose `Contents` stream holds the
//! document as an MFC object archive (Forte is an MFC application): objects
//! written depth-first, each class writing its own fields with no length
//! prefix. Every class has to be decoded field by field, so files with
//! classes or variants none of the sample files had fail to read
//! ([`ForteError::Malformed`]).
//!
//! The document is a tree of score elements ("EC" classes): the conductor,
//! its conductor staff and staves, their measures, and the notes and marks
//! in those. Each element has views: one with its playback data (`DCsq*`
//! classes) and one with its notation (`DCsv*`), which is where this crate
//! reads from. The modules follow that tree:
//!
//! - [`conductor`] - the conductor and the conductor staff: time
//!   signatures, tempos, texts, measures' barlines, repeat signs.
//! - [`staff`] - staves, their measures and everything in them but notes;
//!   ties, slurs and dynamics.
//! - [`note`] - notes, rests, chords and tuplets.
//!
//! Each staff's MIDI setup (patches, controller envelopes) and the page
//! setup aren't decoded. Dynamics live in a staff's volume envelope and are
//! recovered from it by their shape.

mod archive;
pub mod conductor;
mod container;
mod document;
mod error;
pub mod note;
pub mod staff;
#[cfg(test)]
mod tests;

pub use container::CONTENTS;
pub use document::Document;
pub use error::{ForteError, Result};

use std::path::Path;

/// A `.fnf` file.
#[derive(Debug, Clone, PartialEq)]
pub struct FnfFile {
    pub document: Document,
}

impl FnfFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// Reads a `.fnf` file, or a bare `Contents` stream.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let contents = container::contents(data)?;
        Ok(Self {
            document: Document::from_contents(&contents)?,
        })
    }
}
