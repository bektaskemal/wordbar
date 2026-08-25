//! Edit dialog for a single word.
//!
//! Term, definition, example and part of speech — the four fields a user can
//! meaningfully correct. Scheduler state is not editable and not shown.

use adw::prelude::*;
use gtk::glib;

use crate::client::EditFields;
use crate::word::{Sense, Word};

/// Present the dialog. `on_save` receives the edited fields.
/// `senses` are the word's alternatives; choosing one fills the fields in, so
/// a wrong dictionary pick is corrected by picking rather than retyping.
/// Nothing is stored until Save, which is what an editor should do.
pub fn present(
    parent: &impl IsA<gtk::Window>,
    word: &Word,
    senses: &[Sense],
    on_save: impl Fn(EditFields) + 'static,
) {
    let dialog = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title(format!("Edit “{}”", word.term))
        .default_width(460)
        .build();

    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .build();

    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&save);

    let group = adw::PreferencesGroup::new();
    let term = entry_row("Term", &word.term);
    let part_of_speech = entry_row("Part of speech", &word.part_of_speech);
    let definition = entry_row("Definition", &word.definition);
    let example = entry_row("Example", &word.example);

    group.add(&term);

    // Only worth showing when the dictionary actually offered a choice.
    if senses.len() > 1 {
        let labels: Vec<String> = senses.iter().map(|s| truncate(&s.summary, 70)).collect();
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let chooser = adw::ComboRow::builder()
            .title("Sense")
            .subtitle("Pick one to fill in the fields below")
            .model(&model)
            .build();
        let selected = senses.iter().position(|s| s.selected).unwrap_or(0);
        chooser.set_selected(selected as u32);

        let owned: Vec<Sense> = senses.to_vec();
        chooser.connect_selected_notify(glib::clone!(
            @weak part_of_speech, @weak definition, @weak example => move |row| {
                let Some(sense) = owned.get(row.selected() as usize) else { return };
                part_of_speech.set_text(&sense.part_of_speech);
                definition.set_text(&sense.definition);
                example.set_text(&sense.example);
            }
        ));
        group.add(&chooser);
    }

    for row in [&part_of_speech, &definition, &example] {
        group.add(row);
    }

    let page = adw::PreferencesPage::new();
    page.add(&group);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&page);
    dialog.set_content(Some(&content));

    cancel.connect_clicked(glib::clone!(@weak dialog => move |_| dialog.close()));

    save.connect_clicked(glib::clone!(
        @weak dialog, @weak term, @weak part_of_speech, @weak definition, @weak example
        => move |_| {
            let fields = EditFields {
                term: term.text().trim().to_string(),
                part_of_speech: part_of_speech.text().trim().to_string(),
                definition: definition.text().trim().to_string(),
                example: example.text().trim().to_string(),
            };
            // An empty term would delete the word's identity; ignore the save.
            if fields.term.is_empty() {
                term.add_css_class("error");
                return;
            }
            dialog.close();
            on_save(fields);
        }
    ));

    dialog.present();
}

fn entry_row(title: &str, value: &str) -> adw::EntryRow {
    let row = adw::EntryRow::builder().title(title).build();
    row.set_text(value);
    row
}

/// Keep a chooser row readable; dictionary senses can be long.
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}
