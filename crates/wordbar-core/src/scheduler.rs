//! Spaced repetition, delegated to `rs-fsrs`.
//!
//! Wordbar deliberately does not implement its own scheduling mathematics. The
//! trait below is only a narrowing: it exposes the two ratings Wordbar's UI
//! offers and hides FSRS types from the rest of the codebase.

use chrono::{DateTime, Utc};
use rs_fsrs::{Card as FsrsCard, Parameters, State, FSRS};

use crate::model::{Rating, ReviewCard, WordId};

/// Turns a rating into the next review state of a card.
///
/// `Learned` is intentionally absent: archiving a word is a vocabulary
/// operation, not a scheduling one, so it never touches FSRS.
pub trait Scheduler: Send + Sync {
    /// The user could not recall the word. Maps to FSRS `Again`.
    fn again(&self, card: &ReviewCard, now: DateTime<Utc>) -> ReviewCard;

    /// The user recalled the word. Maps to FSRS `Good`.
    fn know(&self, card: &ReviewCard, now: DateTime<Utc>) -> ReviewCard;

    /// Convenience dispatch over [`Rating`].
    fn apply(&self, card: &ReviewCard, rating: Rating, now: DateTime<Utc>) -> ReviewCard {
        match rating {
            Rating::Again => self.again(card, now),
            Rating::Know => self.know(card, now),
        }
    }

    /// A fresh card for a newly added word.
    fn new_card(&self, word_id: WordId, now: DateTime<Utc>) -> ReviewCard {
        ReviewCard::new(word_id, now)
    }
}

/// The production scheduler: stock FSRS with default parameters.
///
/// FSRS knobs (desired retention, stability, difficulty, fuzz) are not exposed
/// to users and are not persisted, so upstream default changes apply
/// automatically.
#[derive(Debug, Clone)]
pub struct FsrsScheduler {
    fsrs: FSRS,
}

impl Default for FsrsScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl FsrsScheduler {
    pub fn new() -> Self {
        Self {
            fsrs: FSRS::new(Parameters::default()),
        }
    }

    fn review(&self, card: &ReviewCard, rating: rs_fsrs::Rating, now: DateTime<Utc>) -> ReviewCard {
        let scheduled = self.fsrs.next(to_fsrs(card), now, rating);
        from_fsrs(card.word_id, &scheduled.card)
    }
}

impl Scheduler for FsrsScheduler {
    fn again(&self, card: &ReviewCard, now: DateTime<Utc>) -> ReviewCard {
        self.review(card, rs_fsrs::Rating::Again, now)
    }

    fn know(&self, card: &ReviewCard, now: DateTime<Utc>) -> ReviewCard {
        self.review(card, rs_fsrs::Rating::Good, now)
    }
}

fn state_from_i32(raw: i32) -> State {
    match raw {
        1 => State::Learning,
        2 => State::Review,
        3 => State::Relearning,
        _ => State::New,
    }
}

fn to_fsrs(card: &ReviewCard) -> FsrsCard {
    FsrsCard {
        due: card.due,
        stability: card.stability,
        difficulty: card.difficulty,
        elapsed_days: card.elapsed_days,
        scheduled_days: card.scheduled_days,
        reps: card.reps,
        lapses: card.lapses,
        state: state_from_i32(card.state),
        last_review: card.last_review,
    }
}

fn from_fsrs(word_id: WordId, card: &FsrsCard) -> ReviewCard {
    ReviewCard {
        word_id,
        due: card.due,
        stability: card.stability,
        difficulty: card.difficulty,
        elapsed_days: card.elapsed_days,
        scheduled_days: card.scheduled_days,
        reps: card.reps,
        lapses: card.lapses,
        state: card.state as i32,
        last_review: card.last_review,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn t(days: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap() + Duration::days(days)
    }

    #[test]
    fn again_on_a_new_word_schedules_it_close_by() {
        let s = FsrsScheduler::new();
        let card = s.new_card(1, t(0));
        let after = s.again(&card, t(0));

        assert!(after.due > t(0));
        assert!(
            after.due - t(0) < Duration::days(1),
            "Again should resurface within a day, got {}",
            after.due
        );
        assert_eq!(after.reps, 1);
    }

    #[test]
    fn repeated_know_grows_the_interval() {
        let s = FsrsScheduler::new();
        let mut card = s.new_card(1, t(0));
        let mut now = t(0);
        let mut previous = Duration::zero();
        let mut grew = 0;

        for _ in 0..6 {
            let next = s.know(&card, now);
            let interval = next.due - now;
            if interval > previous {
                grew += 1;
            }
            previous = interval;
            now = next.due;
            card = next;
        }

        assert!(grew >= 5, "intervals should keep growing, grew {grew}/6");
        assert!(previous > Duration::days(7), "final interval {previous}");
    }

    #[test]
    fn again_after_review_records_a_lapse() {
        let s = FsrsScheduler::new();
        let mut card = s.new_card(1, t(0));
        let mut now = t(0);
        for _ in 0..3 {
            card = s.know(&card, now);
            now = card.due;
        }
        assert_eq!(card.lapses, 0);

        let lapsed = s.again(&card, now);
        assert_eq!(lapsed.lapses, 1);
        assert!(lapsed.due - now < card.due - t(0));
    }

    #[test]
    fn apply_maps_the_two_wordbar_ratings() {
        let s = FsrsScheduler::new();
        let card = s.new_card(1, t(0));
        assert_eq!(s.apply(&card, Rating::Again, t(0)), s.again(&card, t(0)));
        assert_eq!(s.apply(&card, Rating::Know, t(0)), s.know(&card, t(0)));
    }

    #[test]
    fn state_roundtrips_through_the_i32_representation() {
        for raw in 0..4 {
            assert_eq!(state_from_i32(raw) as i32, raw);
        }
    }
}
