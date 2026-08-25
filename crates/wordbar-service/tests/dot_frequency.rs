//! Throwaway measurement: how often would the panel's due dot be lit?
use chrono::{Duration, TimeZone, Utc};
use wordbar_core::{Rating, SelectionPolicy};
use wordbar_service::{Engine, SqliteStore};

#[test]
fn measure_due_dot_frequency() {
    for (label, knows) in [("fresh vocabulary", 0), ("lightly reviewed", 3), ("well reviewed", 8)] {
        let store = SqliteStore::open_in_memory().unwrap();
        let mut e = Engine::with_seed(Box::new(store), 11);
        e.set_policy(SelectionPolicy::default());
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();

        let mut ids = Vec::new();
        for i in 0..40 {
            ids.push(e.add_word(&format!("word{i}"), t0).unwrap().value.id);
        }
        // Push most words out into the future by rating them Know repeatedly.
        let mut now = t0;
        for id in ids.iter().take(36) {
            for _ in 0..knows {
                let card = e.rate(*id, Rating::Know, now).unwrap().value;
                now = card.due;
            }
        }

        let mut due = 0;
        let mut total = 0;
        let mut when = now + Duration::hours(1);
        for _ in 0..300 {
            if let Some(word) = e.advance_display(when).unwrap().value {
                let card = e.get_card(word.id).unwrap().unwrap();
                if card.is_due(when) {
                    due += 1;
                }
                total += 1;
            }
            when += Duration::minutes(15);
        }
        println!(
            "{label:>18}: dot lit on {due}/{total} rotations ({:.0}%)",
            100.0 * due as f64 / total as f64
        );
    }
}
