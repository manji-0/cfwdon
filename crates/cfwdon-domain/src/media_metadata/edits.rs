use std::collections::BTreeMap;
use std::ops::Range;

/// Overwrites a same-length rewrite makes, recorded while the walk reads the
/// original bytes and applied afterwards, so the file can be rewritten in the
/// buffer it arrived in rather than in a copy.
///
/// Fills are kept as disjoint ranges, merged as they are recorded, so a file
/// that points many metadata ranges at the same bytes costs no more than one
/// that points one: applying the edits writes each byte at most once for
/// the fills, and once per position for the writes.
#[derive(Debug, Default)]
pub(super) struct Edits {
    /// Fill ranges by start, holding their end and byte; no two overlap.
    fills: BTreeMap<usize, (usize, u8)>,
    /// Short constant data (a box type, an element header, an empty EXIF
    /// payload) by position, written over the fills.
    writes: BTreeMap<usize, Vec<u8>>,
}

impl Edits {
    /// Fill `range` with `byte`, over any earlier fill of those bytes.
    pub(super) fn fill(&mut self, range: Range<usize>, byte: u8) {
        if range.is_empty() {
            return;
        }
        let (mut start, mut end) = (range.start, range.end);
        // Every fill that overlaps or touches the range, latest start first;
        // ends ascend with starts, so the walk stops at the first one ending
        // before the range.
        let touching: Vec<(usize, usize, u8)> = self
            .fills
            .range(..=range.end)
            .rev()
            .take_while(|(_, (fill_end, _))| *fill_end >= range.start)
            .map(|(&fill_start, &(fill_end, fill_byte))| (fill_start, fill_end, fill_byte))
            .collect();
        for (fill_start, fill_end, fill_byte) in touching {
            if fill_byte == byte {
                self.fills.remove(&fill_start);
                start = start.min(fill_start);
                end = end.max(fill_end);
            } else if fill_end > range.start && fill_start < range.end {
                self.fills.remove(&fill_start);
                if fill_start < range.start {
                    self.fills.insert(fill_start, (range.start, fill_byte));
                }
                if fill_end > range.end {
                    self.fills.insert(range.end, (fill_end, fill_byte));
                }
            }
        }
        self.fills.insert(start, (end, byte));
    }

    pub(super) fn write(&mut self, at: usize, data: &[u8]) {
        self.writes.insert(at, data.to_vec());
    }

    /// How many bytes applying the edits writes.
    #[cfg(test)]
    pub(super) fn written_bytes(&self) -> usize {
        let filled: usize = self.fills.iter().map(|(start, (end, _))| end - start).sum();
        filled + self.writes.values().map(Vec::len).sum::<usize>()
    }

    /// Apply the edits to the bytes they were recorded against.
    pub(super) fn apply(self, out: &mut [u8]) {
        for (start, (end, byte)) in self.fills {
            out[start..end].fill(byte);
        }
        for (at, data) in self.writes {
            out[at..at + data.len()].copy_from_slice(&data);
        }
    }

    /// Whether applying the edits to `bytes` would change any of them,
    /// judged without a copy of the bytes.
    pub(super) fn change(&self, bytes: &[u8]) -> bool {
        if self
            .writes
            .iter()
            .any(|(&at, data)| bytes[at..at + data.len()] != data[..])
        {
            return true;
        }
        // The writes hold, so a fill changes something only where no write
        // lands over it. Both are in position order.
        let mut written = self
            .writes
            .iter()
            .map(|(&at, data)| (at, at + data.len()))
            .peekable();
        for (&start, &(end, byte)) in &self.fills {
            let mut pos = start;
            while pos < end {
                while written
                    .next_if(|&(_, write_end)| write_end <= pos)
                    .is_some()
                {}
                let unwritten_end = match written.peek() {
                    Some(&(write_start, write_end)) if write_start <= pos => {
                        pos = write_end;
                        continue;
                    }
                    Some(&(write_start, _)) => write_start.min(end),
                    None => end,
                };
                if bytes[pos..unwritten_end].iter().any(|&b| b != byte) {
                    return true;
                }
                pos = unwritten_end;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Edits;

    fn applied(edits: Edits, len: usize) -> Vec<u8> {
        let mut out = vec![b'.'; len];
        edits.apply(&mut out);
        out
    }

    #[test]
    fn a_later_fill_wins_over_the_bytes_it_overlaps() {
        let mut edits = Edits::default();
        edits.fill(2..8, b'a');
        edits.fill(4..6, b'b');
        edits.fill(0..3, b'c');
        edits.fill(7..10, b'a');
        assert_eq!(applied(edits, 12), b"cccabbaaaa..");
    }

    #[test]
    fn writes_land_over_fills() {
        let mut edits = Edits::default();
        edits.write(1, b"xy");
        edits.fill(0..4, 0);
        assert_eq!(applied(edits, 5), b"\0xy\0.");
    }

    #[test]
    fn a_change_is_judged_on_the_bytes_the_edits_leave() {
        let mut edits = Edits::default();
        edits.fill(0..6, 0);
        edits.write(1, b"ab");
        assert!(!edits.change(b"\0ab\0\0\0."));
        assert!(edits.change(b"\0ab\0x\0."));
        assert!(edits.change(b"\0aX\0\0\0."));
    }

    #[test]
    fn repeated_fills_of_the_same_bytes_cost_one_fill() {
        let mut edits = Edits::default();
        for round in 0..100_000 {
            edits.fill(0..1000, (round % 2) as u8);
            edits.fill(round % 999..round % 999 + 1, 7);
        }
        assert!(edits.written_bytes() <= 1000);
    }
}
