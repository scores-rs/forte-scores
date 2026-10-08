//! Prints a `.fnf` file's staves and measures.
//!
//! `cargo run --example read_score -- song.fnf`

use forte_scores::FnfFile;
use forte_scores::staff::Element;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: read_score <file.fnf>")?;
    let doc = FnfFile::open(path)?.document;
    println!("title: {:?}, composer: {:?}", doc.title(), doc.composer());
    for meter in &doc.conductor.staff.meters {
        println!("{}/{} at {}", meter.beats, meter.beat_type, meter.time);
    }
    for (i, staff) in doc.conductor.staves.iter().enumerate() {
        println!("staff {}: {:?}", i + 1, staff.name());
        for (m, measure) in staff.measures.iter().enumerate() {
            let notes = measure
                .elements
                .iter()
                .filter(|e| matches!(e, Element::Note(_) | Element::Chord(_)))
                .count();
            println!(
                "  measure {} at {}: {notes} notes and chords",
                m + 1,
                measure.start
            );
        }
    }
    Ok(())
}
