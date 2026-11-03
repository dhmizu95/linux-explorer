//! Back / forward navigation history of a tab.

use crate::location::Location;

#[derive(Debug, Clone, Default)]
pub struct History {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl History {
    /// Record leaving `from` for a new location; clears forward history.
    pub fn push(&mut self, from: Location) {
        if self.back.last() != Some(&from) {
            self.back.push(from);
        }
        self.forward.clear();
        const MAX: usize = 200;
        if self.back.len() > MAX {
            self.back.remove(0);
        }
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self, current: Location) -> Option<Location> {
        let to = self.back.pop()?;
        self.forward.push(current);
        Some(to)
    }

    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let to = self.forward.pop()?;
        self.back.push(current);
        Some(to)
    }

    /// Jump `n` steps back (1 = previous), as from the recent-locations menu.
    pub fn back_n(&mut self, mut current: Location, n: usize) -> Option<Location> {
        if n == 0 || n > self.back.len() {
            return None;
        }
        for _ in 0..n {
            current = self.back(current)?;
        }
        Some(current)
    }

    /// Most recent first.
    pub fn back_list(&self) -> impl Iterator<Item = &Location> {
        self.back.iter().rev()
    }

    pub fn forward_list(&self) -> impl Iterator<Item = &Location> {
        self.forward.iter().rev()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> Location {
        Location::Path(PathBuf::from(s))
    }

    #[test]
    fn back_and_forward() {
        let mut h = History::default();
        h.push(p("/a"));
        h.push(p("/b"));
        // current is /c
        assert_eq!(h.back(p("/c")), Some(p("/b")));
        assert_eq!(h.back(p("/b")), Some(p("/a")));
        assert!(!h.can_back());
        assert_eq!(h.forward(p("/a")), Some(p("/b")));
        // navigating somewhere new clears forward
        h.push(p("/b"));
        assert!(!h.can_forward());
    }

    #[test]
    fn jump_back_several() {
        let mut h = History::default();
        for s in ["/1", "/2", "/3"] {
            h.push(p(s));
        }
        assert_eq!(h.back_n(p("/4"), 2), Some(p("/2")));
        let fwd: Vec<_> = h.forward_list().cloned().collect();
        assert_eq!(fwd, vec![p("/3"), p("/4")]);
    }
}
