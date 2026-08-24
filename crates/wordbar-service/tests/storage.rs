//! Phase 4 proof: the vocabulary works end to end with no UI in sight.

use chrono::{Duration, TimeZone, Utc};
use wordbar_core::{Error, NewWord, Rating, Storage, WordFilter, WordStatus};
use wordbar_service::SqliteStore;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap()
}

fn store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("in-memory database")
}

#[test]
fn add_stores_the_display_form_and_the_normalized_key() {
    let mut s = store();
    let word = s
        .add_word(NewWord::term("\u{201c}Ephemeral\u{201d}"), now())
        .unwrap();

    assert_eq!(word.term, "Ephemeral");
    assert_eq!(word.normalized_term, "ephemeral");
    assert_eq!(word.status, WordStatus::Active);
    assert!(word.last_displayed_at.is_none());
    assert!(word.is_incomplete());
}

#[test]
fn every_new_word_gets_a_card_due_immediately() {
    let mut s = store();
    let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    let card = s.get_card(word.id).unwrap().expect("card");

    assert!(card.is_new());
    assert!(card.is_due(now()));
    assert_eq!(card.reps, 0);
}

#[test]
fn duplicates_are_detected_across_case_and_punctuation() {
    let mut s = store();
    s.add_word(NewWord::term("ephemeral"), now()).unwrap();

    for variant in [
        "Ephemeral",
        "EPHEMERAL",
        "\u{201c}ephemeral\u{201d}",
        "ephemeral.",
    ] {
        let err = s.add_word(NewWord::term(variant), now()).unwrap_err();
        assert!(
            matches!(err, Error::DuplicateTerm(_)),
            "{variant} should be a duplicate, got {err:?}"
        );
    }
    assert_eq!(s.count_words(WordFilter::All).unwrap(), 1);
}

#[test]
fn invalid_terms_are_rejected_before_storage() {
    let mut s = store();
    assert!(s.add_word(NewWord::term("   "), now()).is_err());
    assert!(s.add_word(NewWord::term("a b c d e f"), now()).is_err());
    assert_eq!(s.count_words(WordFilter::All).unwrap(), 0);
}

#[test]
fn find_by_term_normalizes_the_query() {
    let mut s = store();
    s.add_word(NewWord::term("Ad Hoc"), now()).unwrap();

    assert!(s.find_by_term("ad hoc").unwrap().is_some());
    assert!(s.find_by_term("  AD   HOC  ").unwrap().is_some());
    assert!(s.find_by_term("adhoc").unwrap().is_none());
}

#[test]
fn edit_updates_fields_and_can_clear_them() {
    let mut s = store();
    let word = s
        .add_word(
            NewWord {
                term: "ephemeral".into(),
                definition: Some("wrong".into()),
                ..NewWord::default()
            },
            now(),
        )
        .unwrap();

    let patch = wordbar_core::WordPatch {
        definition: Some(Some("lasting for a very short time".into())),
        example: Some(Some("The opportunity was ephemeral.".into())),
        ..Default::default()
    };
    let edited = s
        .update_word(word.id, patch, now() + Duration::hours(1))
        .unwrap();
    assert_eq!(
        edited.definition.as_deref(),
        Some("lasting for a very short time")
    );
    assert!(edited.updated_at > word.updated_at);
    assert!(!edited.is_incomplete());

    let cleared = s
        .update_word(
            word.id,
            wordbar_core::WordPatch {
                definition: Some(None),
                ..Default::default()
            },
            now(),
        )
        .unwrap();
    assert!(cleared.definition.is_none());
}

#[test]
fn renaming_onto_another_word_is_refused() {
    let mut s = store();
    s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    let other = s.add_word(NewWord::term("ubiquitous"), now()).unwrap();

    let err = s
        .update_word(
            other.id,
            wordbar_core::WordPatch {
                term: Some("Ephemeral".into()),
                ..Default::default()
            },
            now(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateTerm(_)));
}

#[test]
fn learned_and_restore_round_trip_without_losing_the_card() {
    let mut s = store();
    let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();

    let mut card = s.get_card(word.id).unwrap().unwrap();
    card.reps = 7;
    card.stability = 12.5;
    s.save_card(&card).unwrap();

    let learned = s.mark_learned(word.id, now()).unwrap();
    assert_eq!(learned.status, WordStatus::Learned);
    assert_eq!(s.list_active().unwrap().len(), 0);
    assert_eq!(s.list_learned().unwrap().len(), 1);

    let restored = s.restore_word(word.id, now()).unwrap();
    assert_eq!(restored.status, WordStatus::Active);

    let card = s.get_card(word.id).unwrap().unwrap();
    assert_eq!(card.reps, 7, "restoring must not reset FSRS state");
    assert_eq!(card.stability, 12.5);
}

#[test]
fn delete_removes_the_word_its_card_and_its_reviews() {
    let mut s = store();
    let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    s.record_review(word.id, Rating::Know, now()).unwrap();

    s.delete_word(word.id).unwrap();

    assert!(s.get_word(word.id).unwrap().is_none());
    assert!(s.get_card(word.id).unwrap().is_none());
    assert!(s.list_reviews(word.id).unwrap().is_empty());
    assert!(matches!(
        s.delete_word(word.id).unwrap_err(),
        Error::UnknownWord(_)
    ));
}

#[test]
fn search_matches_term_definition_and_example() {
    let mut s = store();
    s.add_word(
        NewWord {
            term: "ephemeral".into(),
            definition: Some("lasting for a very short time".into()),
            example: Some("A fleeting, ephemeral glow.".into()),
            ..Default::default()
        },
        now(),
    )
    .unwrap();
    s.add_word(NewWord::term("ubiquitous"), now()).unwrap();

    assert_eq!(s.search_words("EPHEM", WordFilter::All).unwrap().len(), 1);
    assert_eq!(
        s.search_words("short time", WordFilter::All).unwrap().len(),
        1
    );
    assert_eq!(
        s.search_words("fleeting", WordFilter::All).unwrap().len(),
        1
    );
    assert_eq!(s.search_words("zzz", WordFilter::All).unwrap().len(), 0);
    assert_eq!(
        s.search_words("i", WordFilter::All).unwrap().len(),
        2,
        "matches ubiquitous by term and ephemeral by definition"
    );
}

#[test]
fn search_respects_the_status_filter() {
    let mut s = store();
    let a = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    s.add_word(NewWord::term("ephemera"), now()).unwrap();
    s.mark_learned(a.id, now()).unwrap();

    assert_eq!(s.search_words("ephemer", WordFilter::All).unwrap().len(), 2);
    assert_eq!(
        s.search_words("ephemer", WordFilter::Active).unwrap().len(),
        1
    );
    assert_eq!(
        s.search_words("ephemer", WordFilter::Learned)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn reviews_are_kept_in_order() {
    let mut s = store();
    let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();

    s.record_review(word.id, Rating::Again, now()).unwrap();
    s.record_review(word.id, Rating::Know, now() + Duration::days(1))
        .unwrap();

    let reviews = s.list_reviews(word.id).unwrap();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0].rating, Rating::Again);
    assert_eq!(reviews[1].rating, Rating::Know);
    assert!(reviews[1].reviewed_at > reviews[0].reviewed_at);
}

#[test]
fn display_candidates_only_include_active_words() {
    let mut s = store();
    let a = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    s.add_word(NewWord::term("ubiquitous"), now()).unwrap();
    s.mark_learned(a.id, now()).unwrap();

    let candidates = s.display_candidates().unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].word.term, "ubiquitous");
}

#[test]
fn marking_the_current_word_learned_clears_the_pointer() {
    let mut s = store();
    let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
    s.set_current_word_id(Some(word.id)).unwrap();

    s.mark_learned(word.id, now()).unwrap();
    assert_eq!(s.current_word_id().unwrap(), None);
}

#[test]
fn the_database_survives_being_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wordbar.db");

    let id = {
        let mut s = SqliteStore::open(&path).unwrap();
        let word = s.add_word(NewWord::term("ephemeral"), now()).unwrap();
        s.touch_displayed(word.id, now()).unwrap();
        s.set_current_word_id(Some(word.id)).unwrap();
        word.id
    };

    let s = SqliteStore::open(&path).unwrap();
    let word = s.get_word(id).unwrap().expect("word survives");
    assert_eq!(word.term, "ephemeral");
    assert_eq!(word.last_displayed_at, Some(now()));
    assert_eq!(s.current_word_id().unwrap(), Some(id));
}

#[test]
fn migrating_an_already_current_database_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wordbar.db");
    SqliteStore::open(&path).unwrap();
    SqliteStore::open(&path).unwrap();
    let mut s = SqliteStore::open(&path).unwrap();
    assert!(s.add_word(NewWord::term("ephemeral"), now()).is_ok());
}
