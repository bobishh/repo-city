//! Unfinished reading time survives transient camera and preview state.
use std::collections::HashMap;

#[derive(Clone, Default)]
pub(crate) struct Timers {
    unfinished: HashMap<u64, f32>,
}

impl Timers {
    pub(crate) fn advance(&mut self, block: u64, delta: f32, seconds: f32) -> f32 {
        let elapsed = (*self.unfinished.entry(block).or_default() + delta.max(0.)).min(seconds);
        if elapsed >= seconds {
            self.unfinished.remove(&block);
        } else {
            self.unfinished.insert(block, elapsed);
        }
        elapsed
    }

    pub(crate) fn clear(&mut self) {
        self.unfinished.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_reading_resumes_and_other_blocks_do_not_add_time() {
        let mut timers = Timers::default();
        assert_eq!(timers.advance(1, 0.4, 1.), 0.4);
        // A different file or block has its own clock; looking away calls neither.
        assert_eq!(timers.advance(2, 0.8, 1.), 0.8);
        assert!((timers.advance(1, 0.5, 1.) - 0.9).abs() < 0.0001);
        assert_eq!(timers.advance(1, 0.11, 1.), 1.);
        assert!(!timers.unfinished.contains_key(&1));
    }

    #[test]
    fn reset_discards_unfinished_time_and_estimates_can_change() {
        let mut timers = Timers::default();
        timers.advance(1, 0.4, 1.);
        assert_eq!(timers.advance(1, 0.1, 2.), 0.5);
        timers.clear();
        assert_eq!(timers.advance(1, 0.1, 2.), 0.1);
        assert_eq!(timers.advance(2, 100., 0.), 0.);
        assert!(!timers.unfinished.contains_key(&2));
    }
}
