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
    // The panel was empty, so this capture also becomes the panel word; see
    // the_first_capture_fills_an_empty_panel.
    assert_eq!(
        outcome.effects.current_word_changed,
        Some(Some(outcome.value.id))
    );
}

#[test]
fn the_first_capture_fills_an_empty_panel() {
    let mut e = engine();
    assert!(e.current_word(t0()).unwrap().value.is_none());

    let outcome = e.add_word("ephemeral", t0()).unwrap();
    let id = outcome.value.id;

    // Without this the panel keeps its empty state until the next rotation
    // tick, which reads as the capture having silently failed.
    assert_eq!(
        outcome.effects.current_word_changed,
        Some(Some(id)),
        "a capture into an empty rotation must announce the new panel word"
    );
    assert_eq!(e.current_word_id().unwrap(), Some(id));
}

#[test]
fn a_later_capture_does_not_hijack_the_panel() {
    let mut e = engine();
    let first = add(&mut e, "ephemeral");
    e.current_word(t0()).unwrap();

    let outcome = e.add_word("ubiquitous", t0()).unwrap();
    assert_eq!(
        outcome.effects.current_word_changed, None,
        "capturing while a word is showing must not interrupt it"
    );
    assert_eq!(e.current_word_id().unwrap(), Some(first));
}

#[test]
fn restoring_the_only_word_refills_an_empty_panel() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.current_word(t0()).unwrap();
    e.mark_learned(id, t0()).unwrap();
    assert!(e.current_word(t0()).unwrap().value.is_none());

    let outcome = e.restore_word(id, t0()).unwrap();
    assert_eq!(outcome.effects.current_word_changed, Some(Some(id)));
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

// -- senses ---------------------------------------------------------------

fn entry(senses: &[(&str, &str, Option<&str>)]) -> wordbar_core::DictionaryEntry {
    wordbar_core::DictionaryEntry {
        senses: senses
            .iter()
            .map(|(pos, definition, example)| wordbar_core::EntrySense {
                part_of_speech: Some(pos.to_string()),
                definition: definition.to_string(),
                example: example.map(str::to_string),
            })
            .collect(),
        phonetic: Some("/test/".into()),
        audio_url: Some("https://example.test/a.mp3".into()),
    }
}

#[test]
fn enrichment_keeps_every_sense_and_shows_the_first() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");

    e.apply_enrichment(
        id,
        &entry(&[
            (
                "adjective",
                "Lasting a short time.",
                Some("It was ephemeral."),
            ),
            ("noun", "Something short-lived.", None),
        ]),
        t0(),
    )
    .unwrap();

    let word = e.get_word(id).unwrap();
    assert_eq!(word.definition.as_deref(), Some("Lasting a short time."));
    assert_eq!(word.part_of_speech.as_deref(), Some("adjective"));
    assert_eq!(word.phonetic.as_deref(), Some("/test/"));
    assert!(!word.is_incomplete());

    let senses = e.list_senses(id).unwrap();
    assert_eq!(senses.len(), 2, "both senses are kept");
    assert_eq!(senses[0].position, 0);
    assert!(senses[0].is_selected(&word));
    assert!(!senses[1].is_selected(&word));
}

#[test]
fn switching_sense_sticks() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.apply_enrichment(
        id,
        &entry(&[
            ("adjective", "Lasting a short time.", None),
            ("noun", "Something short-lived.", None),
        ]),
        t0(),
    )
    .unwrap();

    let senses = e.list_senses(id).unwrap();
    let noun = senses
        .iter()
        .find(|s| s.part_of_speech.as_deref() == Some("noun"))
        .unwrap();

    let outcome = e.select_sense(noun.id, t0()).unwrap();
    assert_eq!(
        outcome.value.definition.as_deref(),
        Some("Something short-lived.")
    );
    assert_eq!(outcome.effects.word_updated, Some(id));

    // Persisted, not a preview.
    let reread = e.get_word(id).unwrap();
    assert_eq!(reread.definition.as_deref(), Some("Something short-lived."));
    assert_eq!(reread.part_of_speech.as_deref(), Some("noun"));

    let senses = e.list_senses(id).unwrap();
    assert!(senses
        .iter()
        .find(|s| s.id == noun.id)
        .unwrap()
        .is_selected(&reread));
}

#[test]
fn a_fresh_lookup_replaces_the_previous_senses() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.apply_enrichment(id, &entry(&[("noun", "Old.", None)]), t0())
        .unwrap();
    assert_eq!(e.list_senses(id).unwrap().len(), 1);

    e.apply_enrichment(
        id,
        &entry(&[
            ("adjective", "New one.", None),
            ("adjective", "New two.", None),
        ]),
        t0(),
    )
    .unwrap();

    let senses = e.list_senses(id).unwrap();
    assert_eq!(senses.len(), 2);
    assert!(senses.iter().all(|s| s.definition.starts_with("New")));
}

#[test]
fn senses_go_when_the_word_does() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.apply_enrichment(id, &entry(&[("noun", "A thing.", None)]), t0())
        .unwrap();

    e.delete_word(id).unwrap();
    assert!(e.list_senses(id).unwrap().is_empty());
}

#[test]
fn selecting_an_unknown_sense_is_refused() {
    let mut e = engine();
    assert!(e.select_sense(9999, t0()).is_err());
}

#[test]
fn a_word_with_no_senses_still_works() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    assert!(e.list_senses(id).unwrap().is_empty());
    assert!(e.get_word(id).unwrap().is_incomplete());
}

// -- learning vs known ----------------------------------------------------

/// Rate a word Know until its interval passes the known threshold.
fn drill_until_known(e: &mut Engine, id: i64) {
    let mut now = t0();
    for _ in 0..12 {
        let card = e.rate(id, Rating::Know, now).unwrap().value;
        if card.is_known() {
            return;
        }
        now = card.due;
    }
    panic!("a word rated Know repeatedly should eventually count as known");
}

#[test]
fn a_new_word_is_still_being_learned() {
    let mut e = engine();
    add(&mut e, "ephemeral");

    assert_eq!(e.list_words(WordFilter::Learning, "").unwrap().len(), 1);
    assert_eq!(e.list_words(WordFilter::Known, "").unwrap().len(), 0);
}

#[test]
fn a_word_crosses_over_once_its_interval_grows() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");

    drill_until_known(&mut e, id);

    let learning = e.list_words(WordFilter::Learning, "").unwrap();
    let known = e.list_words(WordFilter::Known, "").unwrap();
    assert_eq!(
        learning.len(),
        1,
        "the undrilled word is still being learned"
    );
    assert_eq!(learning[0].term, "ubiquitous");
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].term, "ephemeral");
}

#[test]
fn a_known_word_stays_in_the_rotation() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    drill_until_known(&mut e, id);

    // Knowing a word is not a reason to stop testing it; FSRS simply shows it
    // rarely. Only a manual archive takes it out.
    let card = e.get_card(id).unwrap().unwrap();
    let candidates = e.list_words(WordFilter::Active, "").unwrap();
    assert!(candidates.iter().any(|w| w.id == id));
    assert!(card.is_known());
}

#[test]
fn archiving_by_hand_also_counts_as_known() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    e.mark_learned(id, t0()).unwrap();

    assert_eq!(e.list_words(WordFilter::Known, "").unwrap().len(), 1);
    assert_eq!(e.list_words(WordFilter::Learning, "").unwrap().len(), 0);
}

#[test]
fn the_counts_follow_the_same_split() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    add(&mut e, "ubiquitous");
    drill_until_known(&mut e, id);

    let stats = e.stats().unwrap();
    assert_eq!(stats.active, 1, "still learning");
    assert_eq!(stats.learned, 1, "known");
}

#[test]
fn search_respects_the_split() {
    let mut e = engine();
    let id = add(&mut e, "ephemeral");
    add(&mut e, "ephemera");
    drill_until_known(&mut e, id);

    assert_eq!(e.list_words(WordFilter::Known, "ephemer").unwrap().len(), 1);
    assert_eq!(
        e.list_words(WordFilter::Learning, "ephemer").unwrap().len(),
        1
    );
}

#[test]
fn enrichment_never_overwrites_an_example_you_brought() {
    use wordbar_core::NewWord;

    let mut e = engine();
    // As an import from a Kindle would arrive: the sentence you met it in.
    let word = e
        .add_word_full(
            NewWord {
                term: "ephemeral".into(),
                example: Some("The effects were ephemeral.".into()),
                ..NewWord::default()
            },
            t0(),
        )
        .unwrap()
        .value;

    e.apply_enrichment(
        word.id,
        &entry(&[(
            "adjective",
            "Lasting a short time.",
            Some("A dictionary example."),
        )]),
        t0(),
    )
    .unwrap();

    let after = e.get_word(word.id).unwrap();
    assert_eq!(
        after.example.as_deref(),
        Some("The effects were ephemeral."),
        "the sentence you met the word in is irreplaceable; a dictionary's is not"
    );
    assert_eq!(after.definition.as_deref(), Some("Lasting a short time."));
}

#[test]
fn filling_gaps_keeps_a_definition_you_supplied() {
    use wordbar_core::NewWord;
    use wordbar_service::engine::Enrichment;

    let mut e = engine();
    let word = e
        .add_word_full(
            NewWord {
                term: "ephemeral".into(),
                definition: Some("my own wording".into()),
                ..NewWord::default()
            },
            t0(),
        )
        .unwrap()
        .value;

    e.apply_enrichment_with(
        word.id,
        &entry(&[("adjective", "Lasting a short time.", None)]),
        t0(),
        Enrichment::FillGaps,
    )
    .unwrap();

    assert_eq!(
        e.get_word(word.id).unwrap().definition.as_deref(),
        Some("my own wording")
    );
    // The senses are still recorded, so it can be switched later.
    assert_eq!(e.list_senses(word.id).unwrap().len(), 1);
}

#[test]
fn an_explicit_refresh_does_replace_the_definition() {
    use wordbar_core::NewWord;
    use wordbar_service::engine::Enrichment;

    let mut e = engine();
    let word = e
        .add_word_full(
            NewWord {
                term: "ephemeral".into(),
                definition: Some("stale".into()),
                ..NewWord::default()
            },
            t0(),
        )
        .unwrap()
        .value;

    e.apply_enrichment_with(
        word.id,
        &entry(&[("adjective", "Lasting a short time.", None)]),
        t0(),
        Enrichment::Replace,
    )
    .unwrap();

    assert_eq!(
        e.get_word(word.id).unwrap().definition.as_deref(),
        Some("Lasting a short time.")
    );
}
