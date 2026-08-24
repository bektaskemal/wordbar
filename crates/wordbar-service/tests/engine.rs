//! Phase 5/6 proof: FSRS mapping, the display invariant, and the full
//! add → rate → learn → restore workflow, all without D-Bus or a UI.

use chrono::{DateTime, Duration, TimeZone, Utc};
use wordbar_core::{Error, Rating, SelectionPolicy, WordFilter, WordStatus};
use wordbar_service::{Engine, SqliteStore};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap()
}

fn engine() -> Engine {
    let store = SqliteStore::open_in_memory().expect("in-memory database");
    let mut engine = Engine::with_seed(Box::new(store), 7);
    engine.set_policy(SelectionPolicy::deterministic());
    engine
}

fn add(engine: &mut Engine, term: &str) -> i64 {
    engine.add_word(term, t0()).unwrap().value.id
}

#[test]
fn an_empty_vocabulary_has_no_current_word() {
    let mut e = engine();
    assert!(e.current_word(t0()).unwrap().value.is_none());
    assert!(e.advance_display(t0()).unwrap().value.is_none());
}

#[test]
fn adding_a_word_signals_it_and_marks_the_vocabulary_changed() {
    let mut e = engine();
    let outcome = e.add_word("ephemeral", t0()).unwrap();

    assert_eq!(outcome.effects.word_added, Some(outcome.value.id));
    assert!(outcome.effects.vocabulary_changed);
    assert!(outcome.effects.current_word_changed.is_none());
}

#[test]
fn duplicates_are_reported_and_never_stored_twice() {
    let mut e = engine();
    add(&mut e, "ephemeral");

    let err = e.add_word("Ephemeral", t0()).unwrap_err();
    assert!(matches!(err, Error::DuplicateTerm(_)));
    assert_eq!(e.stats().unwrap().active, 1);
}

#[test]
fn current_word_is_stable_across_repeated_calls() {
    let mut e = engine();
    add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");

    let first = e.current_word(t0()).unwrap().value.unwrap().id;
    for offset in 1..10 {
        let again = e.current_word(t0() + Duration::minutes(offset)).unwrap();
        assert_eq!(again.value.unwrap().id, first, "polling must not rotate");
        assert!(again.effects.current_word_changed.is_none());
    }
}

#[test]
fn advance_moves_to_a_different_word() {
    let mut e = engine();
    add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");

    let first = e.current_word(t0()).unwrap().value.unwrap().id;
    let second = e
        .advance_display(t0() + Duration::minutes(15))
        .unwrap()
        .value
        .unwrap()
        .id;

    assert_ne!(first, second);
}

#[test]
fn displaying_a_word_is_not_a_review() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");

    let before = e.get_card(id).unwrap().unwrap();
    for i in 0..5 {
        e.advance_display(t0() + Duration::hours(i)).unwrap();
    }
    let after = e.get_card(id).unwrap().unwrap();

    assert_eq!(before, after, "display must never touch FSRS state");
    assert!(e.list_reviews(id).unwrap().is_empty());
    assert!(e.get_word(id).unwrap().last_displayed_at.is_some());
}

#[test]
fn again_schedules_soon_and_records_history() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");

    let card = e.rate(id, Rating::Again, t0()).unwrap().value;
    assert!(card.due > t0());
    assert!(card.due - t0() < Duration::days(1));

    let reviews = e.list_reviews(id).unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].rating, Rating::Again);
}

#[test]
fn repeated_know_grows_the_interval() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");

    let mut now = t0();
    let mut last = Duration::zero();
    for _ in 0..5 {
        let card = e.rate(id, Rating::Know, now).unwrap().value;
        let interval = card.due - now;
        assert!(interval >= last, "interval shrank: {last} -> {interval}");
        last = interval;
        now = card.due;
    }
    assert!(last > Duration::days(7), "final interval {last}");
    assert_eq!(e.list_reviews(id).unwrap().len(), 5);
}

#[test]
fn rate_and_advance_rotates_the_panel() {
    let mut e = engine();
    add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");

    let current = e.current_word(t0()).unwrap().value.unwrap().id;
    let outcome = e.rate_and_advance(current, Rating::Know, t0()).unwrap();

    assert_ne!(outcome.value.unwrap().id, current);
    assert_eq!(outcome.effects.word_updated, Some(current));
    assert!(outcome.effects.current_word_changed.is_some());
}

#[test]
fn learned_words_leave_the_rotation_but_keep_their_state() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");

    e.rate(id, Rating::Know, t0()).unwrap();
    let card_before = e.get_card(id).unwrap().unwrap();

    let learned = e.mark_learned(id, t0()).unwrap().value;
    assert_eq!(learned.status, WordStatus::Learned);

    for i in 0..30 {
        let shown = e.advance_display(t0() + Duration::hours(i)).unwrap().value;
        assert_ne!(
            shown.map(|w| w.id),
            Some(id),
            "a learned word must never resurface"
        );
    }

    assert_eq!(e.get_card(id).unwrap().unwrap(), card_before);
    assert!(e.list_reviews(id).unwrap().len() == 1, "history is kept");
}

#[test]
fn marking_the_last_active_word_learned_empties_the_panel() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.current_word(t0()).unwrap();

    let outcome = e.mark_learned(id, t0()).unwrap();
    assert_eq!(outcome.effects.current_word_changed, Some(None));
    assert!(e.current_word(t0()).unwrap().value.is_none());
}

#[test]
fn restore_brings_a_word_back_with_its_schedule() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.rate(id, Rating::Know, t0()).unwrap();
    let card = e.get_card(id).unwrap().unwrap();

    e.mark_learned(id, t0()).unwrap();
    let restored = e.restore_word(id, t0()).unwrap().value;

    assert_eq!(restored.status, WordStatus::Active);
    assert_eq!(e.get_card(id).unwrap().unwrap(), card);
    assert_eq!(e.current_word(t0()).unwrap().value.unwrap().id, id);
}

#[test]
fn deleting_the_current_word_selects_another_one() {
    let mut e = engine();
    let a = add(&mut e, "ephemeral");
    let b = add(&mut e, "ubiquitous");

    let current = e.current_word(t0()).unwrap().value.unwrap().id;
    e.delete_word(current).unwrap();

    let next = e.current_word(t0()).unwrap().value.unwrap().id;
    assert_ne!(next, current);
    assert!(next == a || next == b);
}

#[test]
fn a_stale_current_pointer_recovers_silently() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");
    e.current_word(t0()).unwrap();

    // Archive whichever word is showing; the next read must not fail.
    let current = e.current_word_id().unwrap().unwrap();
    e.mark_learned(current, t0()).unwrap();

    let recovered = e.current_word(t0()).unwrap().value.expect("a word");
    assert_eq!(recovered.status, WordStatus::Active);
    assert_ne!(recovered.id, current);
    let _ = id;
}

#[test]
fn listing_filters_and_searches() {
    let mut e = engine();
    let a = add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");
    e.mark_learned(a, t0()).unwrap();

    assert_eq!(e.list_words(WordFilter::All, "").unwrap().len(), 2);
    assert_eq!(e.list_words(WordFilter::Active, "").unwrap().len(), 1);
    assert_eq!(e.list_words(WordFilter::Learned, "").unwrap().len(), 1);
    assert_eq!(e.list_words(WordFilter::All, "ephem").unwrap().len(), 1);
    assert_eq!(e.stats().unwrap().active, 1);
    assert_eq!(e.stats().unwrap().learned, 1);
}

/// Phase 10, expressed as a test: the whole documented CLI workflow.
#[test]
fn the_full_backend_workflow() {
    let mut e = engine();
    let mut now = t0();

    // wordbarctl add ephemeral
    let id = e.add_word("ephemeral", now).unwrap().value.id;

    // wordbarctl current
    assert_eq!(e.current_word(now).unwrap().value.unwrap().id, id);

    // wordbarctl again
    now += Duration::minutes(1);
    e.rate_and_advance(id, Rating::Again, now).unwrap();
    assert_eq!(e.list_reviews(id).unwrap().len(), 1);

    // wordbarctl next
    now += Duration::minutes(15);
    e.advance_display(now).unwrap();

    // wordbarctl know
    now += Duration::minutes(1);
    e.rate_and_advance(id, Rating::Know, now).unwrap();
    assert_eq!(e.list_reviews(id).unwrap().len(), 2);

    // wordbarctl learned
    e.mark_learned(id, now).unwrap();
    assert_eq!(e.list_words(WordFilter::Learned, "").unwrap().len(), 1);
    assert!(e.current_word(now).unwrap().value.is_none());

    // wordbarctl restore ephemeral
    let found = e.find_by_term("Ephemeral").unwrap().expect("found by term");
    e.restore_word(found.id, now).unwrap();
    assert_eq!(e.current_word(now).unwrap().value.unwrap().id, id);
}
