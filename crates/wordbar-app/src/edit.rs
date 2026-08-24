//! Edit dialog for a single word.
//!
//! Term, definition, example and part of speech — the four fields a user can
//! meaningfully correct. Scheduler state is not editable and not shown.

use adw::prelude::*;
use gtk::glib;

use crate::client::EditFields;
use crate::word::Word;

/// Present the dialog. `on_save` receives the edited fields.
pub fn present(
    parent: &impl IsA<gtk::Window>,
    word: &Word,
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
    for row in [&term, &part_of_speech, &definition, &example] {
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
