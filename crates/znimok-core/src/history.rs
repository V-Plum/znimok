//! Undo/redo as state snapshots, not inverse commands (LH `EdSnap`, inventory §2.3).
//!
//! A snapshot is taken *before* a change and pushed only if the change really altered the
//! state, so a click that changes nothing does not create an empty step. Series of small
//! changes (arrow nudges, one drag of a slider, typing into one text) share a [`MergeKey`]
//! and collapse into one step. Pixels are never copied: snapshots hold bank numbers.
//!
//! Depth: owner's decision 28.09 — "reasonably maximal": 500 steps (LH had 120), additionally
//! capped by an estimated memory budget so a document with huge pen trails cannot grow
//! without bound.

use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Default depth of the undo stack.
pub const DEFAULT_MAX_STEPS: usize = 500;
/// Default memory budget of the undo stack (estimated bytes).
pub const DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;

/// Identifies a series of changes that should become a single undo step.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "series", rename_all = "snake_case")]
pub enum MergeKey {
    /// Arrow-key nudges of the current selection.
    Nudge,
    /// One drag of a slider or handle; `id` distinguishes different controls or drags.
    Drag { id: u64 },
    /// Typing into one text mark.
    Typing { object: u32 },
}

/// Estimated memory taken by a snapshot, for the budget.
pub trait Weigh {
    fn weigh(&self) -> usize;
}

#[derive(Debug)]
pub struct History<T> {
    undo: VecDeque<(T, usize)>,
    redo: Vec<(T, usize)>,
    bytes: usize,
    last_merge: Option<MergeKey>,
    pub max_steps: usize,
    pub max_bytes: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            last_merge: None,
            max_steps: DEFAULT_MAX_STEPS,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl<T: Weigh> History<T> {
    pub fn new(max_steps: usize, max_bytes: usize) -> Self {
        Self {
            max_steps: max_steps.max(1),
            max_bytes,
            ..Self::default()
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Estimated bytes held by both stacks.
    pub fn bytes(&self) -> usize {
        self.bytes + self.redo.iter().map(|(_, b)| b).sum::<usize>()
    }

    /// Records the state from *before* a change that did happen. With the same merge key as the
    /// previous record the change joins that step (its "before" is already stored).
    /// Returns `true` when a new step was created.
    pub fn record(&mut self, before: T, merge: Option<MergeKey>) -> bool {
        if merge.is_some() && merge == self.last_merge && !self.undo.is_empty() {
            return false;
        }
        self.clear_redo();
        let w = before.weigh();
        self.bytes += w;
        self.undo.push_back((before, w));
        self.last_merge = merge;
        self.trim();
        true
    }

    /// Ends any running series: the next change starts a new step even with the same key
    /// (e.g. the user released the arrow key and clicked elsewhere).
    pub fn break_series(&mut self) {
        self.last_merge = None;
    }

    /// Returns the state to restore; `current` goes to the redo stack.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let (prev, w) = self.undo.pop_back()?;
        self.bytes -= w;
        let cw = current.weigh();
        self.redo.push((current, cw));
        self.last_merge = None;
        Some(prev)
    }

    /// Returns the state to restore; `current` goes back to the undo stack.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let (next, _) = self.redo.pop()?;
        let cw = current.weigh();
        self.bytes += cw;
        self.undo.push_back((current, cw));
        self.last_merge = None;
        Some(next)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.last_merge = None;
    }

    fn clear_redo(&mut self) {
        self.redo.clear();
    }

    /// Drops the oldest steps beyond the depth or the memory budget; the newest step is kept
    /// even if it alone exceeds the budget.
    fn trim(&mut self) {
        while self.undo.len() > 1
            && (self.undo.len() > self.max_steps || self.bytes > self.max_bytes)
        {
            if let Some((_, w)) = self.undo.pop_front() {
                self.bytes -= w;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct S(i32, usize);
    impl Weigh for S {
        fn weigh(&self) -> usize {
            self.1
        }
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut h = History::<S>::default();
        let mut cur = S(0, 1);
        for v in 1..=3 {
            h.record(cur.clone(), None);
            cur = S(v, 1);
        }
        assert_eq!(h.undo_len(), 3);
        cur = h.undo(cur).unwrap();
        assert_eq!(cur, S(2, 1));
        cur = h.undo(cur).unwrap();
        assert_eq!(cur, S(1, 1));
        cur = h.redo(cur).unwrap();
        assert_eq!(cur, S(2, 1));
        // A new change clears redo.
        h.record(cur.clone(), None);
        assert!(!h.can_redo());
    }

    #[test]
    fn series_merge_into_one_step_until_broken() {
        let mut h = History::<S>::default();
        assert!(h.record(S(0, 1), Some(MergeKey::Nudge)));
        assert!(!h.record(S(1, 1), Some(MergeKey::Nudge)));
        assert!(!h.record(S(2, 1), Some(MergeKey::Nudge)));
        assert_eq!(h.undo_len(), 1);
        // Undo returns to the state before the whole series.
        assert_eq!(h.undo(S(3, 1)), Some(S(0, 1)));
        h.redo(S(0, 1));
        h.break_series();
        assert!(h.record(S(3, 1), Some(MergeKey::Nudge)));
        assert!(h.record(S(4, 1), Some(MergeKey::Drag { id: 7 })));
        assert!(!h.record(S(5, 1), Some(MergeKey::Drag { id: 7 })));
        assert!(h.record(S(6, 1), Some(MergeKey::Drag { id: 8 })));
    }

    #[test]
    fn depth_and_budget_drop_oldest() {
        let mut h = History::<S>::new(3, usize::MAX);
        for v in 0..10 {
            h.record(S(v, 1), None);
        }
        assert_eq!(h.undo_len(), 3);
        assert_eq!(h.undo(S(10, 1)), Some(S(9, 1)));

        let mut h = History::<S>::new(500, 100);
        for v in 0..10 {
            h.record(S(v, 30), None);
        }
        assert_eq!(h.undo_len(), 3);
        assert!(h.bytes() <= 100 + 30);
        // A single oversized step is still kept.
        let mut h = History::<S>::new(500, 10);
        h.record(S(0, 1000), None);
        assert_eq!(h.undo_len(), 1);
    }

    #[test]
    fn default_depth_is_owner_decision() {
        assert_eq!(History::<S>::default().max_steps, 500);
    }
}
