/// Finds a success marker in a byte stream that arrives in arbitrary chunks.
///
/// A log line can be split across two `read()` calls, so a scanner that only
/// inspects each chunk would miss the marker. The scanner retains the last
/// `marker.len() - 1` bytes and prepends them to the next chunk.
#[derive(Debug, Clone)]
pub struct MarkerScanner {
    marker: Vec<u8>,
    tail: Vec<u8>,
    seen: bool,
}

impl MarkerScanner {
    pub fn new(marker: impl Into<String>) -> Self {
        Self {
            marker: marker.into().into_bytes(),
            tail: Vec::new(),
            seen: false,
        }
    }

    /// Feed a chunk. Returns `true` once the marker has been observed.
    pub fn push(&mut self, chunk: &[u8]) -> bool {
        if self.seen {
            return true;
        }
        if self.marker.is_empty() {
            self.seen = true;
            return true;
        }
        let mut window = Vec::with_capacity(self.tail.len() + chunk.len());
        window.extend_from_slice(&self.tail);
        window.extend_from_slice(chunk);
        if window
            .windows(self.marker.len())
            .any(|candidate| candidate == self.marker.as_slice())
        {
            self.seen = true;
            self.tail.clear();
            return true;
        }
        let keep = self.marker.len().saturating_sub(1).min(window.len());
        self.tail = window[window.len() - keep..].to_vec();
        false
    }

    pub fn seen(&self) -> bool {
        self.seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_marker_in_one_chunk() {
        let mut s = MarkerScanner::new("READY");
        assert!(s.push(b"booting... READY\n"));
        assert!(s.seen());
    }

    #[test]
    fn finds_marker_split_across_chunks() {
        let mut s = MarkerScanner::new("READY");
        assert!(!s.push(b"boot"));
        assert!(!s.push(b"ing RE"));
        assert!(s.push(b"ADY\n"));
    }

    #[test]
    fn does_not_match_a_substring_of_the_marker() {
        let mut s = MarkerScanner::new("READY");
        assert!(!s.push(b"REA"));
        assert!(!s.seen());
    }

    #[test]
    fn remembers_after_first_hit() {
        let mut s = MarkerScanner::new("OK");
        assert!(s.push(b"OK"));
        assert!(s.push(b"anything"));
    }

    #[test]
    fn empty_marker_matches_immediately() {
        let mut s = MarkerScanner::new("");
        assert!(s.push(b""));
    }

    #[test]
    fn handles_marker_larger_than_chunks() {
        let mut s = MarkerScanner::new("DROLIN_GUEST_OK");
        for chunk in [b"DRO".as_slice(), b"LIN_GUEST_", b"OK"] {
            s.push(chunk);
        }
        assert!(s.seen());
    }
}
