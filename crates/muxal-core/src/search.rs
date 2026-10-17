//! Runtime navigation state for one pane's scrollback search. Match coordinates
//! are snapshots: callers refresh them from the live buffer before navigating.

use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ScrollbackSearch {
    instance: Uuid,
    query: String,
    matches: Vec<i32>,
    index: usize,
}

impl ScrollbackSearch {
    pub fn new(instance: Uuid) -> Self {
        Self {
            instance,
            query: String::new(),
            matches: Vec::new(),
            index: 0,
        }
    }

    pub fn instance(&self) -> Uuid {
        self.instance
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// A changed query starts at its newest match.
    pub fn set_query(&mut self, query: &str, matches: Vec<i32>) -> Option<i32> {
        self.query = query.to_string();
        self.index = matches.len().saturating_sub(1);
        self.matches = matches;
        self.selected_line()
    }

    /// Replace stale coordinates without moving the viewport. Keep the selected
    /// ordinal where possible; history eviction or clearing may remove matches.
    pub fn refresh(&mut self, matches: Vec<i32>) -> bool {
        if self.matches == matches {
            return false;
        }
        self.index = if self.matches.is_empty() {
            matches.len().saturating_sub(1)
        } else {
            self.index.min(matches.len().saturating_sub(1))
        };
        self.matches = matches;
        true
    }

    pub fn step(&mut self, delta: i32) -> Option<i32> {
        if self.matches.is_empty() {
            return None;
        }
        self.index =
            (self.index as i64 + i64::from(delta)).rem_euclid(self.matches.len() as i64) as usize;
        self.selected_line()
    }

    pub fn selected_line(&self) -> Option<i32> {
        self.matches.get(self.index).copied()
    }

    /// Human-facing match ordinal and total, including the empty `0/0` state.
    pub fn position(&self) -> (usize, usize) {
        let total = self.matches.len();
        (if total == 0 { 0 } else { self.index + 1 }, total)
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollbackSearch;
    use uuid::Uuid;

    #[test]
    fn queries_remain_owned_by_the_original_pane() {
        let pane = Uuid::new_v4();
        let mut search = ScrollbackSearch::new(pane);
        assert_eq!(search.set_query("error", vec![-8, -3]), Some(-3));
        assert_eq!(search.position(), (2, 2));
        assert_eq!(search.step(-1), Some(-8));
        assert_eq!(search.step(-1), Some(-3));
        search.refresh(vec![-9, -4]);
        assert_eq!(search.instance(), pane);
        assert_eq!(search.query(), "error");
        assert_eq!(search.set_query("warning", vec![-6]), Some(-6));
        assert_eq!(search.position(), (1, 1));
    }

    #[test]
    fn new_output_replaces_coordinates_before_the_next_step() {
        let mut search = ScrollbackSearch::new(Uuid::new_v4());
        search.set_query("error", vec![-8, -3]);
        // Two new rows scroll both matches upward, and a new match arrives.
        assert!(search.refresh(vec![-10, -5, 1]));
        assert_eq!(search.position(), (2, 3));
        assert_eq!(search.step(-1), Some(-10));
        assert_eq!(search.step(1), Some(-5));
        assert_eq!(search.step(1), Some(1));
        assert!(!search.refresh(vec![-10, -5, 1]));
    }

    #[test]
    fn evicted_or_cleared_matches_never_leave_an_invalid_selection() {
        let mut search = ScrollbackSearch::new(Uuid::new_v4());
        search.set_query("error", vec![-8, -3]);
        search.refresh(vec![-1]);
        assert_eq!(search.position(), (1, 1));
        assert_eq!(search.step(1), Some(-1));
        search.refresh(Vec::new());
        assert_eq!(search.position(), (0, 0));
        assert_eq!(search.step(-1), None);
        search.refresh(vec![0, 2]);
        assert_eq!(search.position(), (2, 2));
        assert_eq!(search.selected_line(), Some(2));
    }
}
