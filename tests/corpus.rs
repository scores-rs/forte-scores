//! Reads real `.fnf` files and checks that each staff's events leave no
//! gaps: every note, rest or chord starts no later than the one before it
//! on its staff ends (by its value, dots and tuplet) - a gap would mean an
//! event was missed or a duration misread. The files aren't part of
//! the repo, so the test is ignored by default: point `FNF_SAMPLES_DIR` at
//! a directory of `.fnf` files (searched recursively) and run
//! `cargo test --test corpus -- --ignored --nocapture`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use forte_scores::FnfFile;
use forte_scores::note::{NoteValue, TupletElement};
use forte_scores::staff::Element;

fn fnf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            fnf_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fnf"))
        {
            out.push(path);
        }
    }
}

fn length(value: u8, dots: u8, tuplet: Option<(u8, u8)>) -> f64 {
    let base = NoteValue::from_code(value).map_or(0.0, NoteValue::quarters);
    let (mut length, mut add) = (base, base);
    for _ in 0..dots {
        add /= 2.0;
        length += add;
    }
    tuplet.map_or(length, |(actual, normal)| {
        length * normal as f64 / actual as f64
    })
}

/// `(start, length)` of each event, per staff of a grand staff.
fn timeline(
    element: &Element,
    tuplet: Option<(u8, u8)>,
    out: &mut BTreeMap<bool, Vec<(f64, f64)>>,
) {
    let (lower, start, len) = match element {
        Element::Note(n) => (n.lower, n.time, length(n.value, n.dots, tuplet)),
        Element::Rest(r) => (r.lower, r.time, length(r.value, r.dots, tuplet)),
        Element::Chord(c) => (c.lower, c.time, length(c.value, c.dots(), tuplet)),
        Element::Tuplet(t) => {
            for e in &t.elements {
                let e = match e {
                    TupletElement::Note(n) => Element::Note(n.clone()),
                    TupletElement::Rest(r) => Element::Rest(r.clone()),
                    TupletElement::Chord(c) => Element::Chord(c.clone()),
                };
                timeline(&e, Some((t.actual, t.normal)), out);
            }
            return;
        }
        _ => return,
    };
    out.entry(lower).or_default().push((start as f64, len));
}

#[test]
#[ignore]
fn reads_real_files() {
    let dir =
        std::env::var("FNF_SAMPLES_DIR").expect("set FNF_SAMPLES_DIR to a directory of .fnf files");
    let mut files = Vec::new();
    fnf_files(Path::new(&dir), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .fnf files under {dir}");
    let mut failures = Vec::new();
    let (mut events, mut gaps) = (0, 0);
    for path in &files {
        let doc = match FnfFile::open(path) {
            Ok(file) => file.document,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        println!(
            "{}: version {}, {:?}, {} staves",
            path.display(),
            doc.version,
            doc.edition,
            doc.conductor.staves.len()
        );
        for staff in &doc.conductor.staves {
            let mut lines = BTreeMap::new();
            for measure in &staff.measures {
                for element in &measure.elements {
                    timeline(element, None, &mut lines);
                }
            }
            for line in lines.values_mut() {
                line.sort_by(|a, b| a.0.total_cmp(&b.0));
                events += line.len();
                for pair in line.windows(2) {
                    // Overlaps are fine: they're separate voices.
                    if pair[1].0 > pair[0].0 + pair[0].1 + 1e-3 {
                        gaps += 1;
                        println!("  gap after {:?}: next at {}", pair[0], pair[1].0);
                    }
                }
            }
        }
    }
    println!("{} file(s), {events} event(s), {gaps} gap(s)", files.len());
    assert!(
        failures.is_empty(),
        "unreadable files:\n{}",
        failures.join("\n")
    );
    assert_eq!(gaps, 0, "gaps between events");
}
