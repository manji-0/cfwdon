use std::ops::Range;

/// Overwrites a same-length rewrite makes, recorded while the walk reads the
/// original bytes and applied in order afterwards, so the file can be
/// rewritten in the buffer it arrived in rather than in a copy.
#[derive(Debug, Default)]
pub(super) struct Edits(Vec<Edit>);

#[derive(Debug)]
enum Edit {
    Fill { range: Range<usize>, byte: u8 },
    Write { at: usize, data: Vec<u8> },
}

impl Edits {
    pub(super) fn fill(&mut self, range: Range<usize>, byte: u8) {
        self.0.push(Edit::Fill { range, byte });
    }

    pub(super) fn write(&mut self, at: usize, data: &[u8]) {
        self.0.push(Edit::Write {
            at,
            data: data.to_vec(),
        });
    }

    /// Apply the edits to the bytes they were recorded against.
    pub(super) fn apply(self, out: &mut [u8]) {
        for edit in self.0 {
            match edit {
                Edit::Fill { range, byte } => out[range].fill(byte),
                Edit::Write { at, data } => out[at..at + data.len()].copy_from_slice(&data),
            }
        }
    }
}
