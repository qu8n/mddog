#[derive(Debug)]
pub struct List {
    pub labels: Vec<String>,
    pub sel: usize,
    pub offset: usize,
    pub current: usize,
    pub shown: usize,
}

impl List {
    pub fn new(labels: Vec<String>, current: usize, height: usize) -> List {
        let mut list = List { labels, sel: current, offset: 0, current, shown: current };
        list.reveal(height);
        list
    }

    pub fn preview(&mut self) -> bool {
        let changed = self.shown != self.sel;
        self.shown = self.sel;
        changed
    }

    pub fn step(&mut self, down: bool, height: usize) -> bool {
        let to = if down { self.sel + 1 } else { self.sel.wrapping_sub(1) };
        if to >= self.labels.len() {
            return false;
        }
        self.sel = to;
        self.reveal(height);
        true
    }

    pub fn point(&mut self, row: usize) -> bool {
        let i = self.offset + row;
        if row >= self.labels.len().saturating_sub(self.offset) || i == self.sel {
            return false;
        }
        self.sel = i;
        true
    }

    pub fn at(&self, row: usize) -> Option<usize> {
        Some(self.offset + row).filter(|&i| i < self.labels.len())
    }

    pub fn reveal(&mut self, height: usize) {
        let height = height.max(1);
        self.offset = self.offset.min(self.sel).max((self.sel + 1).saturating_sub(height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("t{i}")).collect()
    }

    #[test]
    fn the_highlight_stays_in_view() {
        let mut l = List::new(labels(5), 0, 2);
        assert!(!l.step(false, 2));
        assert!(l.step(true, 2) && l.step(true, 2));
        assert_eq!((l.sel, l.offset), (2, 1));
        assert!(l.step(true, 2) && l.step(true, 2) && !l.step(true, 2));
        assert_eq!((l.sel, l.offset), (4, 3));
        assert!(l.point(0));
        assert_eq!(l.sel, 3);
        assert!(!l.point(0) && !l.point(2));
        assert_eq!((l.at(1), l.at(2)), (Some(4), None));
    }

    #[test]
    fn a_preview_shows_the_highlight_once() {
        let mut l = List::new(labels(5), 0, 10);
        assert!(!l.preview());
        assert!(l.step(true, 10) && l.step(true, 10));
        assert_eq!(l.shown, 0);
        assert!(l.preview() && !l.preview());
        assert_eq!(l.shown, 2);
    }
}
