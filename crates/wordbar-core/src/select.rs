//! Panel display policy.
//!
//! FSRS answers *when is this card due*. This module answers a different
//! question: *which word should sit in the top bar right now*. Always showing
//! the single most overdue card makes the panel feel mechanical, so the policy
//! scores candidates and samples among the good ones.

use chrono::{DateTime, Duration, Utc};
use rand::Rng;

use crate::model::{ReviewCard, Word, WordId, WordStatus};

/// A word plus its scheduling state, as handed to the selector.
#[derive(Debug, Clone)]
pub struct DisplayCandidate {
    pub word: Word,
    pub card: ReviewCard,
}

impl DisplayCandidate {
    pub fn id(&self) -> WordId {
        self.word.id
    }
}

/// Tunables for [`select_display_word`]. Not user-facing; the defaults are the
/// product behaviour and tests override them to remove randomness.
#[derive(Debug, Clone)]
pub struct SelectionPolicy {
    /// Score added to a card that is due.
    pub due_bonus: f64,
    /// Extra score per day overdue, capped by `max_overdue_days`.
    pub overdue_per_day: f64,
    pub max_overdue_days: f64,
    /// Score added to a word that has never been rated.
    pub new_word_bonus: f64,
    /// Window over which the new-word boost decays to zero.
    pub new_word_window: Duration,
    /// Penalty applied to a word displayed within `recent_display_window`.
    pub recent_display_penalty: f64,
    pub recent_display_window: Duration,
    /// Upper bound of the uniform jitter added to every score.
    pub jitter: f64,
}

impl Default for SelectionPolicy {
    fn default() -> Self {
        Self {
            due_bonus: 3.0,
            overdue_per_day: 0.1,
            max_overdue_days: 30.0,
            new_word_bonus: 2.0,
            new_word_window: Duration::days(3),
            recent_display_penalty: 4.0,
            recent_display_window: Duration::hours(6),
            jitter: 1.0,
        }
    }
}

impl SelectionPolicy {
    /// A policy with the randomness removed, for deterministic tests.
    pub fn deterministic() -> Self {
        Self {
            jitter: 0.0,
            ..Self::default()
        }
    }
}

/// Pick the next panel word.
///
/// `current` is the word on screen right now; it is excluded whenever another
/// active word exists, so the panel never appears stuck. Returns `None` only
/// when there is nothing active to show.
pub fn select_display_word(
    candidates: &[DisplayCandidate],
    current: Option<WordId>,
    now: DateTime<Utc>,
    policy: &SelectionPolicy,
    rng: &mut impl Rng,
) -> Option<WordId> {
    let active: Vec<&DisplayCandidate> = candidates
        .iter()
        .filter(|c| c.word.status == WordStatus::Active)
        .collect();

    if active.is_empty() {
        return None;
    }

    // Rule 2: exclude the current word *if possible*.
    let pool: Vec<&DisplayCandidate> = match current {
        Some(id) if active.iter().any(|c| c.id() != id) => {
            active.iter().copied().filter(|c| c.id() != id).collect()
        }
        _ => active,
    };

    pool.iter()
        .map(|c| (score(c, now, policy, rng), c.id()))
        .max_by(|(a, a_id), (b, b_id)| {
            a.partial_cmp(b)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Stable ordering when scores tie, so tests are reproducible.
                .then_with(|| b_id.cmp(a_id))
        })
        .map(|(_, id)| id)
}

fn score(
    candidate: &DisplayCandidate,
    now: DateTime<Utc>,
    policy: &SelectionPolicy,
    rng: &mut impl Rng,
) -> f64 {
    let mut score = 0.0;

    // Rule 3: strongly prefer due words, a little more the longer they are due.
    if candidate.card.is_due(now) {
        let overdue_days = (now - candidate.card.due).num_seconds() as f64 / 86_400.0;
        score += policy.due_bonus
            + overdue_days.clamp(0.0, policy.max_overdue_days) * policy.overdue_per_day;
    }

    // Rule 4: boost recently added words so new captures surface quickly.
    if candidate.card.is_new() {
        let age = now - candidate.word.created_at;
        let window = policy.new_word_window.num_seconds() as f64;
        if window > 0.0 {
            let remaining = 1.0 - (age.num_seconds() as f64 / window);
            score += policy.new_word_bonus * remaining.clamp(0.0, 1.0);
        }
    }

    // Rule 5: penalize words shown recently, decaying back to zero.
    if let Some(shown) = candidate.word.last_displayed_at {
        let since = now - shown;
        let window = policy.recent_display_window.num_seconds() as f64;
        if window > 0.0 && since < policy.recent_display_window {
            let freshness = 1.0 - (since.num_seconds().max(0) as f64 / window);
            score -= policy.recent_display_penalty * freshness;
        }
    }

    // Rule 6: a little randomness so rotation does not feel like a fixed list.
    if policy.jitter > 0.0 {
        score += rng.gen_range(0.0..policy.jitter);
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WordStatus;
    use chrono::TimeZone;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 10, 12, 0, 0).unwrap()
    }

    fn candidate(id: WordId, status: WordStatus) -> DisplayCandidate {
        DisplayCandidate {
            word: Word {
                id,
                term: format!("word{id}"),
                normalized_term: format!("word{id}"),
                definition: None,
                example: None,
                part_of_speech: None,
                phonetic: None,
                audio_url: None,
                status,
                created_at: now() - Duration::days(365),
                updated_at: now() - Duration::days(365),
                last_displayed_at: None,
            },
            // Not due for a long time unless a test says otherwise.
            card: ReviewCard {
                state: rs_fsrs::State::Review as i32,
                due: now() + Duration::days(30),
                reps: 5,
                ..ReviewCard::new(id, now() - Duration::days(365))
            },
        }
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    fn pick(candidates: &[DisplayCandidate], current: Option<WordId>) -> Option<WordId> {
        select_display_word(
            candidates,
            current,
            now(),
            &SelectionPolicy::deterministic(),
            &mut rng(),
        )
    }

    #[test]
    fn empty_vocabulary_selects_nothing() {
        assert_eq!(pick(&[], None), None);
    }

    #[test]
    fn learned_words_never_appear() {
        let words = vec![
            candidate(1, WordStatus::Learned),
            candidate(2, WordStatus::Learned),
        ];
        assert_eq!(pick(&words, None), None);
    }

    #[test]
    fn learned_words_are_skipped_when_active_ones_exist() {
        let words = vec![
            candidate(1, WordStatus::Learned),
            candidate(2, WordStatus::Active),
        ];
        assert_eq!(pick(&words, Some(9)), Some(2));
    }

    #[test]
    fn no_immediate_repetition_when_alternatives_exist() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        // Make the current word by far the most attractive choice.
        words[0].card.due = now() - Duration::days(10);

        assert_eq!(pick(&words, Some(1)), Some(2));
    }

    #[test]
    fn a_single_word_repeats_rather_than_showing_nothing() {
        let words = vec![candidate(1, WordStatus::Active)];
        assert_eq!(pick(&words, Some(1)), Some(1));
    }

    #[test]
    fn overdue_words_are_favoured() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
            candidate(3, WordStatus::Active),
        ];
        words[1].card.due = now() - Duration::days(5);

        assert_eq!(pick(&words, None), Some(2));
    }

    #[test]
    fn more_overdue_beats_barely_due() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        words[0].card.due = now() - Duration::minutes(1);
        words[1].card.due = now() - Duration::days(20);

        assert_eq!(pick(&words, None), Some(2));
    }

    #[test]
    fn new_words_surface_quickly() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        // 1 is due; 2 was just captured and has never been rated.
        words[0].card.due = now() - Duration::hours(1);
        words[1].word.created_at = now() - Duration::minutes(5);
        words[1].card = ReviewCard::new(2, now() - Duration::minutes(5));

        assert_eq!(pick(&words, None), Some(2));
    }

    #[test]
    fn recently_displayed_words_are_penalized() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        words[0].card.due = now() - Duration::days(1);
        words[1].card.due = now() - Duration::days(1);
        words[0].word.last_displayed_at = Some(now() - Duration::minutes(10));

        assert_eq!(pick(&words, None), Some(2));
    }

    #[test]
    fn an_old_display_no_longer_penalizes() {
        let mut words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        words[0].card.due = now() - Duration::days(2);
        words[1].card.due = now() - Duration::days(1);
        words[0].word.last_displayed_at = Some(now() - Duration::days(3));

        assert_eq!(pick(&words, None), Some(1));
    }

    #[test]
    fn randomness_spreads_selection_across_comparable_words() {
        let words: Vec<DisplayCandidate> = (1..=5)
            .map(|id| {
                let mut c = candidate(id, WordStatus::Active);
                c.card.due = now() - Duration::days(1);
                c
            })
            .collect();

        let mut rng = rng();
        let policy = SelectionPolicy::default();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            if let Some(id) = select_display_word(&words, None, now(), &policy, &mut rng) {
                seen.insert(id);
            }
        }
        assert!(seen.len() >= 4, "expected variety, saw {seen:?}");
    }

    #[test]
    fn not_due_words_still_get_picked_when_nothing_is_due() {
        let words = vec![
            candidate(1, WordStatus::Active),
            candidate(2, WordStatus::Active),
        ];
        assert!(pick(&words, None).is_some());
    }
}
