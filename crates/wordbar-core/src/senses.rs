//! Ranking dictionary senses.
//!
//! A dictionary lists senses in its own order, which is often not the order a
//! learner wants: `ephemeral` comes back with a noun sense first, though
//! nobody means the noun. Wordbar keeps every sense but decides which one to
//! show.

/// A sense as offered by a [`crate::DictionaryProvider`], before storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedSense {
    pub part_of_speech: Option<String>,
    pub definition: String,
    pub example: Option<String>,
}

/// Order senses so the most likely intended one comes first.
///
/// There is no frequency data to hand, so the ranking uses what the entry
/// itself reveals:
///
/// 1. **How many senses share a part of speech.** A dictionary spends its
///    entry where the word is actually used, so the part of speech with the
///    most senses is the word's dominant role. This is what puts `ephemeral`'s
///    three adjective senses ahead of its single noun sense.
/// 2. **Where that part of speech first appeared**, to keep the source's own
///    judgement as the tie-break between equally sized groups.
/// 3. **Whether the sense carries an example.** Editors illustrate primary
///    senses, and an example makes a much better card.
/// 4. **The source order**, so the result is deterministic.
pub fn rank_senses(senses: Vec<RankedSense>) -> Vec<RankedSense> {
    let key_of = |sense: &RankedSense| sense.part_of_speech.as_deref().unwrap_or("").to_lowercase();

    let mut group_size = std::collections::HashMap::new();
    let mut first_seen = std::collections::HashMap::new();
    for (index, sense) in senses.iter().enumerate() {
        let key = key_of(sense);
        *group_size.entry(key.clone()).or_insert(0usize) += 1;
        first_seen.entry(key).or_insert(index);
    }

    let mut ordered: Vec<(usize, RankedSense)> = senses.into_iter().enumerate().collect();
    ordered.sort_by(|(a_index, a), (b_index, b)| {
        let (a_key, b_key) = (key_of(a), key_of(b));
        let a_size = group_size.get(&a_key).copied().unwrap_or(0);
        let b_size = group_size.get(&b_key).copied().unwrap_or(0);

        b_size
            .cmp(&a_size)
            .then_with(|| first_seen[&a_key].cmp(&first_seen[&b_key]))
            .then_with(|| b.example.is_some().cmp(&a.example.is_some()))
            .then_with(|| a_index.cmp(b_index))
    });

    ordered.into_iter().map(|(_, sense)| sense).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sense(pos: &str, definition: &str, example: Option<&str>) -> RankedSense {
        RankedSense {
            part_of_speech: Some(pos.to_string()),
            definition: definition.to_string(),
            example: example.map(str::to_string),
        }
    }

    fn shape(senses: &[RankedSense]) -> Vec<(&str, &str)> {
        senses
            .iter()
            .map(|s| {
                (
                    s.part_of_speech.as_deref().unwrap_or(""),
                    s.definition.as_str(),
                )
            })
            .collect()
    }

    /// The case that prompted all of this: the API lists the noun first.
    #[test]
    fn the_dominant_part_of_speech_wins() {
        let ranked = rank_senses(vec![
            sense("noun", "Something short-lived.", None),
            sense("adjective", "Lasting a short time.", None),
            sense("adjective", "Existing for only one day.", None),
            sense("adjective", "Usually dry.", None),
        ]);
        assert_eq!(
            shape(&ranked)[0],
            ("adjective", "Lasting a short time."),
            "three adjective senses should outrank one noun sense"
        );
        assert_eq!(ranked.len(), 4, "no sense is discarded");
    }

    #[test]
    fn every_sense_is_kept() {
        let ranked = rank_senses(vec![
            sense("noun", "A.", None),
            sense("verb", "B.", None),
            sense("adjective", "C.", None),
        ]);
        assert_eq!(ranked.len(), 3);
    }

    #[test]
    fn equal_groups_keep_the_source_order() {
        let ranked = rank_senses(vec![
            sense("verb", "First listed.", None),
            sense("noun", "Second listed.", None),
        ]);
        assert_eq!(shape(&ranked)[0], ("verb", "First listed."));
    }

    #[test]
    fn within_a_group_an_example_floats_up() {
        let ranked = rank_senses(vec![
            sense("noun", "Bare.", None),
            sense("noun", "Illustrated.", Some("An example.")),
        ]);
        assert_eq!(shape(&ranked)[0], ("noun", "Illustrated."));
    }

    #[test]
    fn a_bigger_group_beats_an_example_in_a_smaller_one() {
        let ranked = rank_senses(vec![
            sense("noun", "Illustrated.", Some("An example.")),
            sense("verb", "One.", None),
            sense("verb", "Two.", None),
        ]);
        assert_eq!(
            shape(&ranked)[0].0,
            "verb",
            "the dominant role matters more than an example"
        );
    }

    #[test]
    fn a_missing_part_of_speech_is_tolerated() {
        let ranked = rank_senses(vec![
            RankedSense {
                part_of_speech: None,
                definition: "No part of speech.".into(),
                example: None,
            },
            sense("noun", "A noun.", None),
        ]);
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn ranking_is_deterministic() {
        let input = vec![
            sense("noun", "A.", None),
            sense("verb", "B.", None),
            sense("verb", "C.", None),
        ];
        assert_eq!(rank_senses(input.clone()), rank_senses(input));
    }

    #[test]
    fn an_empty_entry_ranks_to_nothing() {
        assert!(rank_senses(vec![]).is_empty());
    }
}
