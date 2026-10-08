//! The `.fnf` container: an OLE compound file (structured storage) with the
//! document in a single stream, `Contents`.

use crate::error::{ForteError, Result};
use std::io::{Cursor, Read};

/// The compound file signature.
const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// The document stream's name.
pub const CONTENTS: &str = "Contents";

/// The `Contents` stream of a `.fnf` file, or `data` itself if it already
/// is a bare stream.
pub fn contents(data: &[u8]) -> Result<Vec<u8>> {
    if !data.starts_with(&MAGIC) {
        return if data.starts_with(b"MMMF") {
            Ok(data.to_vec())
        } else {
            Err(ForteError::NotForte)
        };
    }
    let mut file = cfb::CompoundFile::open(Cursor::new(data))?;
    let mut contents = Vec::new();
    file.open_stream(format!("/{CONTENTS}"))?
        .read_to_end(&mut contents)?;
    Ok(contents)
}
