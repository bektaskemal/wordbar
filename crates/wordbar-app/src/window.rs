//! The manager window: vocabulary and preferences.
//!
//! No decks, no tags, no learning dashboard — the product is a vocabulary
//! list you can search and correct, plus the handful of settings that change
//! how the panel behaves.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;

use crate::client::Client;
use crate::edit;
use crate::word::Word;

/// The extension's schema. Preferences are stored there so the panel picks
/// changes up immediately, with no second source of truth.
const SCHEMA_ID: &str = "org.gnome.shell.extensions.wordbar";
const EXTENSION_UUID: &str = "wordbar@kemalbektas.dev";
const ROTATION_OPTIONS: [i32; 5] = [5, 10, 15, 30, 60];

pub struct Ui {
    client: Client,
    toasts: adw::ToastOverlay,
    list: gtk::ListBox,
    status: gtk::Label,
    search: gtk::SearchEntry,
    empty: adw::StatusPage,
    scroller: gtk::ScrolledWindow,
    window: adw::ApplicationWindow,
    /// Which slice of the vocabulary is on screen.
    showing_learned: RefCell<bool>,
    /// The words currently rendered, so row actions can resolve an id.
    words: RefCell<Vec<Word>>,
    /// Set once the pages are built, so a later launch can switch page.
    stack: RefCell<Option<adw::ViewStack>>,
    /// Restrict the list to words that still need a definition.
    incomplete_only: RefCell<bool>,
    /// Says so when that filter is on, and offers a way out of it.
    banner: adw::Banner,
}

pub fn build(
    app: &adw::Application,
    client: Client,
    page: &str,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Wordbar")
        .default_width(560)
        .default_height(640)
        .build();

    let stack = adw::ViewStack::new();
    let toasts = adw::ToastOverlay::new();

    let ui = Rc::new(Ui::new(client, toasts.clone(), window.clone()));

    stack.add_titled_with_icon(
        &ui.build_vocabulary_page(),
        Some("vocabulary"),
        "Vocabulary",
        "view-list-symbolic",
    );
    stack.add_titled_with_icon(
        &ui.build_preferences_page(),
        Some("preferences"),
        "Preferences",
        "preferences-system-symbolic",
    );
    stack.set_visible_child_name(match page {
        "preferences" => "preferences",
        _ => "vocabulary",
    });

    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    let header = adw::HeaderBar::builder().title_widget(&switcher).build();

    // Import and export act on the vocabulary, so they belong with it rather
    // than among the settings that change how the panel behaves.
    let menu = gio::Menu::new();
    menu.append(Some("Import vocabulary…"), Some("win.import"));
    menu.append(Some("Export vocabulary…"), Some("win.export"));
    header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main menu")
            .build(),
    );

    toasts.set_child(Some(&stack));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&toasts);
    toasts.set_vexpand(true);
    window.set_content(Some(&content));

    *ui.stack.borrow_mut() = Some(stack.clone());
    ui.install_actions();
    ui.connect_change_signal();
    ui.reload();

    (window, ui)
}

fn row_action(label: &str, action: &str, id: i64) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), None);
    item.set_action_and_target_value(Some(action), Some(&id.to_variant()));
    item
}

impl Ui {
    fn new(client: Client, toasts: adw::ToastOverlay, window: adw::ApplicationWindow) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.add_css_class("boxed-list");

        Self {
            client,
            toasts,
            list,
            status: gtk::Label::builder().css_classes(["dim-label"]).build(),
            search: gtk::SearchEntry::builder()
                .placeholder_text("Search vocabulary…")
                .hexpand(true)
                .build(),
            empty: adw::StatusPage::builder()
                .icon_name("accessories-dictionary-symbolic")
                .title("No words yet")
                .description("Select a word anywhere and press Super+Shift+V to add it.")
                .vexpand(true)
                .build(),
            scroller: gtk::ScrolledWindow::builder().vexpand(true).build(),
            window,
            showing_learned: RefCell::new(false),
            words: RefCell::new(Vec::new()),
            stack: RefCell::new(None),
            incomplete_only: RefCell::new(false),
            banner: adw::Banner::builder()
                .title("Showing words that need a definition")
                .button_label("Show all")
                .revealed(false)
                .build(),
        }
    }

    // -- vocabulary -------------------------------------------------------

    fn build_vocabulary_page(self: &Rc<Self>) -> gtk::Widget {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
        page.set_margin_top(12);
        page.set_margin_bottom(12);
        page.set_margin_start(12);
        page.set_margin_end(12);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.append(&self.search);

        // Active / Learned, so "restore learned" has somewhere to happen.
        let switcher = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        switcher.add_css_class("linked");
        let active_button = gtk::ToggleButton::with_label("Active");
        let learned_button = gtk::ToggleButton::with_label("Learned");
        learned_button.set_group(Some(&active_button));
        active_button.set_active(true);
        switcher.append(&active_button);
        switcher.append(&learned_button);
        controls.append(&switcher);

        let add_button = gtk::Button::from_icon_name("list-add-symbolic");
        add_button.set_tooltip_text(Some("Add a word"));
        controls.append(&add_button);
        page.append(&controls);
        page.append(&self.banner);

        self.banner
            .connect_button_clicked(clone!(@weak self as ui => move |_| ui.show_all_words()));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&self.list);
        self.scroller.set_child(Some(&clamp(&content)));
        page.append(&self.scroller);
        page.append(&self.empty);
        page.append(&self.status);

        self.search
            .connect_search_changed(clone!(@weak self as ui => move |_| ui.reload()));

        learned_button.connect_toggled(clone!(@weak self as ui => move |button| {
            *ui.showing_learned.borrow_mut() = button.is_active();
            ui.reload();
        }));

        add_button.connect_clicked(clone!(@weak self as ui => move |_| ui.prompt_add()));

        page.upcast()
    }

    fn filter(&self) -> &'static str {
        if *self.showing_learned.borrow() {
            "learned"
        } else {
            "active"
        }
    }

    /// Refresh the list and the counters. Every call is asynchronous.
    fn reload(self: &Rc<Self>) {
        let ui = Rc::clone(self);
        glib::spawn_future_local(async move {
            let query = ui.search.text().to_string();
            match ui.client.list(ui.filter(), &query).await {
                Ok(mut words) => {
                    if *ui.incomplete_only.borrow() {
                        words.retain(|w| w.definition.is_empty());
                    }
                    ui.show_words(&words, !query.is_empty());
                }
                Err(err) => {
                    ui.show_words(&[], false);
                    ui.error(&err);
                }
            }

            match ui.client.stats().await {
                Ok((active, learned)) => {
                    ui.status
                        .set_text(&format!("{active} active · {learned} learned"));
                }
                Err(_) => ui.status.set_text(""),
            }
        });
    }

    fn show_words(self: &Rc<Self>, words: &[Word], searching: bool) {
        *self.words.borrow_mut() = words.to_vec();
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        if words.is_empty() {
            self.scroller.set_visible(false);
            self.empty.set_visible(true);
            if *self.incomplete_only.borrow() {
                self.empty.set_title("Nothing needs a definition");
                self.empty
                    .set_description(Some("Every word has one. Choose Show all to see them."));
            } else if searching {
                self.empty.set_title("No matches");
                self.empty.set_description(Some("Try a different search."));
            } else if *self.showing_learned.borrow() {
                self.empty.set_title("Nothing learned yet");
                self.empty
                    .set_description(Some("Words you mark as learned appear here."));
            } else {
                self.empty.set_title("No words yet");
                self.empty.set_description(Some(
                    "Select a word anywhere and press Super+Shift+V to add it.",
                ));
            }
            return;
        }

        self.empty.set_visible(false);
        self.scroller.set_visible(true);
        for word in words {
            self.list.append(&self.build_row(word));
        }
    }

    fn build_row(self: &Rc<Self>, word: &Word) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&word.term))
            .subtitle(glib::markup_escape_text(&word.subtitle()))
            .build();

        if word.definition.is_empty() {
            row.add_css_class("dim-label");
        }

        // Built with explicit target values, never with a detailed action
        // string: "word.edit(3)" parses its target as int32, these actions take
        // int64, and GTK silently drops an activation whose target type does
        // not match -- so every row menu entry did nothing at all.
        let menu = gio::Menu::new();
        menu.append_item(&row_action("Edit…", "word.edit", word.id));
        menu.append_item(&row_action("Choose sense…", "word.sense", word.id));
        if word.learned {
            menu.append_item(&row_action("Restore", "word.restore", word.id));
        } else {
            menu.append_item(&row_action("Mark as learned", "word.learned", word.id));
        }
        menu.append_item(&row_action("Delete", "word.delete", word.id));

        let button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .valign(gtk::Align::Center)
            .menu_model(&menu)
            .build();
        button.add_css_class("flat");
        row.add_suffix(&button);

        row
    }

    /// Wire the row menu actions once, on the window.
    fn install_actions(self: &Rc<Self>) {
        let group = gio::SimpleActionGroup::new();

        let edit = gio::SimpleAction::new("edit", Some(glib::VariantTy::INT64));
        edit.connect_activate(clone!(@weak self as ui => move |_, param| {
            let Some(id) = param.and_then(|p| p.get::<i64>()) else { return };
            let word = ui.words.borrow().iter().find(|w| w.id == id).cloned();
            match word {
                Some(word) => ui.present_editor(&word),
                None => ui.edit_word(id),
            }
        }));
        group.add_action(&edit);

        let sense = gio::SimpleAction::new("sense", Some(glib::VariantTy::INT64));
        sense.connect_activate(clone!(@weak self as ui => move |_, param| {
            let Some(id) = param.and_then(|p| p.get::<i64>()) else { return };
            ui.choose_sense(id);
        }));
        group.add_action(&sense);

        let learned = gio::SimpleAction::new("learned", Some(glib::VariantTy::INT64));
        learned.connect_activate(clone!(@weak self as ui => move |_, param| {
            let Some(id) = param.and_then(|p| p.get::<i64>()) else { return };
            glib::spawn_future_local(clone!(@weak ui => async move {
                match ui.client.mark_learned(id).await {
                    Ok(()) => {
                        ui.toast_with_undo("Marked as learned", move |ui| ui.restore_word(id));
                        ui.reload();
                    }
                    Err(err) => ui.error(&err),
                }
            }));
        }));
        group.add_action(&learned);

        let restore = gio::SimpleAction::new("restore", Some(glib::VariantTy::INT64));
        restore.connect_activate(clone!(@weak self as ui => move |_, param| {
            let Some(id) = param.and_then(|p| p.get::<i64>()) else { return };
            ui.restore_word(id);
        }));
        group.add_action(&restore);

        let delete = gio::SimpleAction::new("delete", Some(glib::VariantTy::INT64));
        delete.connect_activate(clone!(@weak self as ui => move |_, param| {
            let Some(id) = param.and_then(|p| p.get::<i64>()) else { return };
            let term = ui
                .words
                .borrow()
                .iter()
                .find(|w| w.id == id)
                .map(|w| w.term.clone())
                .unwrap_or_default();
            ui.confirm_delete(id, &term);
        }));
        group.add_action(&delete);

        self.window.insert_action_group("word", Some(&group));

        let window_actions = gio::SimpleActionGroup::new();

        let import = gio::SimpleAction::new("import", None);
        import.connect_activate(clone!(@weak self as ui => move |_, _| ui.import()));
        window_actions.add_action(&import);

        let export = gio::SimpleAction::new("export", None);
        export.connect_activate(clone!(@weak self as ui => move |_, _| ui.export()));
        window_actions.add_action(&export);

        self.window
            .insert_action_group("win", Some(&window_actions));
    }

    /// Show only words with no definition, as the popup's hint asks for.
    pub fn show_incomplete_only(self: &Rc<Self>) {
        *self.incomplete_only.borrow_mut() = true;
        self.banner.set_revealed(true);
        if let Some(stack) = self.stack.borrow().as_ref() {
            stack.set_visible_child_name("vocabulary");
        }
        self.reload();
    }

    /// Leave the "needs a definition" view.
    pub fn show_all_words(self: &Rc<Self>) {
        self.clear_incomplete_filter();
        self.reload();
    }

    fn clear_incomplete_filter(self: &Rc<Self>) {
        if *self.incomplete_only.borrow() {
            *self.incomplete_only.borrow_mut() = false;
            self.banner.set_revealed(false);
        }
    }

    /// Raise the window and switch pages, for a second launch that was
    /// handed to the already-running instance.
    pub fn present_window(self: &Rc<Self>, page: &str) {
        if let Some(stack) = self.stack.borrow().as_ref() {
            stack.set_visible_child_name(match page {
                "preferences" => "preferences",
                _ => "vocabulary",
            });
        }
        self.window.present();
    }

    /// Open the editor for one word, wherever it sits in the list.
    ///
    /// Used by the popup's "Add definition" link so a word whose enrichment
    /// found nothing can be completed without hunting for it.
    pub fn edit_word(self: &Rc<Self>, id: i64) {
        let ui = Rc::clone(self);
        glib::spawn_future_local(async move {
            match ui.client.get(id).await {
                Ok(word) => ui.present_editor(&word),
                Err(err) => ui.error(&err),
            }
        });
    }

    /// Open the editor, fetching the word's senses first so the chooser is
    /// populated when the dialog appears.
    fn present_editor(self: &Rc<Self>, word: &Word) {
        let ui = Rc::clone(self);
        let word = word.clone();
        glib::spawn_future_local(async move {
            let senses = ui.client.senses(word.id).await.unwrap_or_default();
            ui.present_editor_with(&word, &senses);
        });
    }

    fn present_editor_with(self: &Rc<Self>, word: &Word, senses: &[crate::word::Sense]) {
        let id = word.id;
        let ui = Rc::clone(self);
        edit::present(&self.window, word, senses, move |fields| {
            let ui = Rc::clone(&ui);
            glib::spawn_future_local(async move {
                match ui.client.update(id, fields).await {
                    Ok(w) => {
                        ui.toast(&format!("Updated “{}”", w.term));
                        ui.reload();
                    }
                    Err(err) => ui.error(&err),
                }
            });
        });
    }

    /// Pick which sense a word shows, straight from the list.
    ///
    /// The editor can do this too, but a wrong sense is common enough to
    /// deserve its own one-step route rather than being buried in a form.
    fn choose_sense(self: &Rc<Self>, id: i64) {
        let ui = Rc::clone(self);
        glib::spawn_future_local(async move {
            let senses = match ui.client.senses(id).await {
                Ok(senses) => senses,
                Err(err) => return ui.error(&err),
            };
            if senses.len() < 2 {
                ui.toast("No other senses were found for this word");
                return;
            }
            ui.present_sense_chooser(&senses);
        });
    }

    fn present_sense_chooser(self: &Rc<Self>, senses: &[crate::word::Sense]) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .heading("Choose sense")
            .body("The chosen sense is what the panel shows from now on.")
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("boxed-list");
        for sense in senses {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&sense.summary))
                .build();
            row.set_title_lines(3);
            list.append(&row);
        }
        if let Some(index) = senses.iter().position(|s| s.selected) {
            if let Some(row) = list.row_at_index(index as i32) {
                list.select_row(Some(&row));
            }
        }

        let scroller = gtk::ScrolledWindow::builder()
            .min_content_height(220)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        dialog.set_extra_child(Some(&scroller));

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("use", "Use this sense");
        dialog.set_response_appearance("use", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("use"));

        let owned: Vec<crate::word::Sense> = senses.to_vec();
        dialog.connect_response(
            None,
            clone!(@weak self as ui, @weak list => move |_, response| {
                if response != "use" {
                    return;
                }
                let Some(row) = list.selected_row() else { return };
                let Some(sense) = owned.get(row.index() as usize) else { return };
                let sense_id = sense.id;
                glib::spawn_future_local(clone!(@weak ui => async move {
                    match ui.client.select_sense(sense_id).await {
                        Ok(word) => {
                            ui.toast(&format!("Updated \u{201c}{}\u{201d}", word.term));
                            ui.reload();
                        }
                        Err(err) => ui.error(&err),
                    }
                }));
            }),
        );
        dialog.present();
    }

    fn restore_word(self: &Rc<Self>, id: i64) {
        let ui = Rc::clone(self);
        glib::spawn_future_local(async move {
            match ui.client.restore(id).await {
                Ok(()) => {
                    ui.toast("Restored");
                    ui.reload();
                }
                Err(err) => ui.error(&err),
            }
        });
    }

    /// Deletion drops review history too, so it always asks first.
    fn confirm_delete(self: &Rc<Self>, id: i64, term: &str) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .heading(format!("Delete “{term}”?"))
            .body("This removes the word and its review history permanently.")
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));

        dialog.connect_response(
            None,
            clone!(@weak self as ui => move |_, response| {
                if response != "delete" {
                    return;
                }
                glib::spawn_future_local(clone!(@weak ui => async move {
                    match ui.client.delete(id).await {
                        Ok(()) => {
                            ui.toast("Deleted");
                            ui.reload();
                        }
                        Err(err) => ui.error(&err),
                    }
                }));
            }),
        );
        dialog.present();
    }

    fn prompt_add(self: &Rc<Self>) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .heading("Add word")
            .build();
        let entry = gtk::Entry::builder()
            .placeholder_text("ephemeral")
            .activates_default(true)
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("add", "Add");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));

        dialog.connect_response(
            None,
            clone!(@weak self as ui, @weak entry => move |_, response| {
                if response != "add" {
                    return;
                }
                let text = entry.text().trim().to_string();
                if text.is_empty() {
                    return;
                }
                glib::spawn_future_local(clone!(@weak ui => async move {
                    match ui.client.add(&text).await {
                        Ok(word) => {
                            ui.toast(&format!("Added “{}”", word.term));
                            ui.reload();
                        }
                        Err(err) => ui.error(&err),
                    }
                }));
            }),
        );
        dialog.present();
    }

    // -- preferences ------------------------------------------------------

    fn build_preferences_page(self: &Rc<Self>) -> gtk::Widget {
        let page = adw::PreferencesPage::new();

        match settings() {
            Some(settings) => page.add(&self.build_panel_group(&settings)),
            None => {
                let group = adw::PreferencesGroup::builder()
                    .title("Panel")
                    .description(
                        "The Wordbar GNOME Shell extension is not installed, so panel \
                         settings are unavailable.",
                    )
                    .build();
                page.add(&group);
            }
        }

        page.upcast()
    }

    fn build_panel_group(self: &Rc<Self>, settings: &gio::Settings) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder().title("Panel").build();

        let options = gtk::StringList::new(
            &ROTATION_OPTIONS
                .iter()
                .map(|m| format!("{m} minutes"))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        let rotation = adw::ComboRow::builder()
            .title("Change word every")
            .model(&options)
            .build();
        let current = settings.int("rotation-minutes");
        let index = ROTATION_OPTIONS
            .iter()
            .position(|m| *m == current)
            .unwrap_or(2) as u32;
        rotation.set_selected(index);
        rotation.connect_selected_notify(clone!(@strong settings => move |row| {
            let minutes = ROTATION_OPTIONS
                .get(row.selected() as usize)
                .copied()
                .unwrap_or(15);
            let _ = settings.set_int("rotation-minutes", minutes);
        }));
        group.add(&rotation);

        group.add(&self.build_shortcut_row(settings));
        group
    }

    /// Editable capture shortcut.
    ///
    /// Lives here rather than in the extension's own preferences so there is
    /// one window for every setting; it writes the extension's GSettings, so
    /// the Shell rebinds the key immediately.
    fn build_shortcut_row(self: &Rc<Self>, settings: &gio::Settings) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title("Capture shortcut")
            .subtitle("Select a word anywhere, then press this")
            .build();

        let label = gtk::ShortcutLabel::builder()
            .accelerator(current_accelerator(settings))
            .valign(gtk::Align::Center)
            .build();
        row.add_suffix(&label);

        let button = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Set a new shortcut")
            .build();
        button.connect_clicked(clone!(
            @weak self as ui, @weak label, @strong settings => move |_| {
                ui.capture_shortcut(&label, &settings);
            }
        ));
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        row
    }

    /// Grab the next key combination the user presses.
    fn capture_shortcut(self: &Rc<Self>, label: &gtk::ShortcutLabel, settings: &gio::Settings) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .heading("Set capture shortcut")
            .body("Press the key combination, or Escape to cancel.")
            .build();
        dialog.add_response("cancel", "Cancel");

        let controller = gtk::EventControllerKey::new();
        controller.connect_key_pressed(clone!(
            @weak dialog, @weak label, @strong settings
            => @default-return glib::Propagation::Stop,
            move |_, keyval, keycode, state| {
                if keyval == gdk::Key::Escape {
                    dialog.close();
                    return glib::Propagation::Stop;
                }

                let mask = state & gtk::accelerator_get_default_mod_mask();
                // Require a modifier: a bare key would swallow ordinary typing.
                if mask.is_empty() || !gtk::accelerator_valid(keyval, mask) {
                    return glib::Propagation::Stop;
                }

                let accelerator =
                    gtk::accelerator_name_with_keycode(gdk::Display::NONE, keyval, keycode, mask);
                if !accelerator.is_empty() {
                    let _ = settings.set_strv("capture-shortcut", [accelerator.as_str()]);
                    label.set_accelerator(&accelerator);
                }
                dialog.close();
                glib::Propagation::Stop
            }
        ));
        dialog.add_controller(controller);
        dialog.present();
    }

    fn import(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Import vocabulary")
            .build();
        dialog.set_filters(Some(&exchange_filters()));

        let ui = Rc::clone(self);
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file.path() else { return };
            let format = format_for(&path);
            glib::spawn_future_local(async move {
                let data = match std::fs::read_to_string(&path) {
                    Ok(data) => data,
                    Err(err) => {
                        ui.error(&format!("Cannot read {}: {err}", path.display()));
                        return;
                    }
                };
                match ui.client.import(&data, format).await {
                    Ok((added, duplicates, invalid)) => {
                        ui.toast(&format!(
                            "Imported {added}, skipped {duplicates} duplicate(s), \
                                 {invalid} invalid"
                        ));
                        ui.reload();
                    }
                    Err(err) => ui.error(&err),
                }
            });
        });
    }

    fn export(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Export vocabulary")
            .initial_name("wordbar-vocabulary.json")
            .build();
        dialog.set_filters(Some(&exchange_filters()));

        let ui = Rc::clone(self);
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file.path() else { return };
            let format = format_for(&path);
            glib::spawn_future_local(async move {
                match ui.client.export("all", format).await {
                    Ok(data) => match std::fs::write(&path, data) {
                        Ok(()) => ui.toast(&format!("Exported to {}", path.display())),
                        Err(err) => ui.error(&format!("Cannot write {}: {err}", path.display())),
                    },
                    Err(err) => ui.error(&err),
                }
            });
        });
    }

    // -- feedback ---------------------------------------------------------

    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    fn toast_with_undo(self: &Rc<Self>, message: &str, undo: impl Fn(&Rc<Self>) + 'static) {
        let toast = adw::Toast::new(message);
        toast.set_button_label(Some("Undo"));
        let ui = Rc::clone(self);
        toast.connect_button_clicked(move |_| undo(&ui));
        self.toasts.add_toast(toast);
    }

    fn error(&self, message: &str) {
        let toast = adw::Toast::new(message);
        toast.set_timeout(8);
        self.toasts.add_toast(toast);
    }

    /// Captures made from the Shell should appear here without a manual reload.
    fn connect_change_signal(self: &Rc<Self>) {
        let changes = self.client.changes();
        let ui = Rc::clone(self);
        glib::spawn_future_local(async move {
            while changes.recv().await.is_ok() {
                ui.reload();
            }
        });
    }
}

fn clamp(child: &impl IsA<gtk::Widget>) -> adw::Clamp {
    adw::Clamp::builder().maximum_size(640).child(child).build()
}

fn exchange_filters() -> gio::ListStore {
    let filters = gio::ListStore::new::<gtk::FileFilter>();

    let json = gtk::FileFilter::new();
    json.set_name(Some("JSON"));
    json.add_suffix("json");
    filters.append(&json);

    let csv = gtk::FileFilter::new();
    csv.set_name(Some("CSV"));
    csv.add_suffix("csv");
    filters.append(&csv);

    filters
}

fn format_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") => "csv",
        _ => "json",
    }
}

/// Open the extension's settings.
///
/// A GNOME Shell extension compiles its schema inside its own directory, which
/// the Shell knows to look in but the default GSettings source does not -- so
/// the schema has to be loaded from there by hand. Falls back to the default
/// source in case Wordbar was installed system-wide, and returns None when the
/// extension really is absent, so preferences degrade to a note.
fn settings() -> Option<gio::Settings> {
    if let Some(schema) =
        gio::SettingsSchemaSource::default().and_then(|s| s.lookup(SCHEMA_ID, true))
    {
        return Some(settings_for(&schema));
    }

    for dir in extension_schema_dirs() {
        let Ok(source) = gio::SettingsSchemaSource::from_directory(
            &dir,
            gio::SettingsSchemaSource::default().as_ref(),
            true,
        ) else {
            continue;
        };
        if let Some(schema) = source.lookup(SCHEMA_ID, true) {
            return Some(settings_for(&schema));
        }
    }

    None
}

fn settings_for(schema: &gio::SettingsSchema) -> gio::Settings {
    gio::Settings::new_full(schema, None::<&gio::SettingsBackend>, None)
}

/// Where the extension might be installed, user location first.
fn extension_schema_dirs() -> Vec<std::path::PathBuf> {
    let suffix = format!("gnome-shell/extensions/{EXTENSION_UUID}/schemas");
    let mut dirs = vec![glib::user_data_dir().join(&suffix)];

    // Also the literal default, because we may be launched with an
    // XDG_DATA_HOME that is not the one the extension was installed under --
    // a sandboxed terminal, for instance.
    if let Some(home) = glib::home_dir().to_str() {
        dirs.push(
            std::path::Path::new(home)
                .join(".local/share")
                .join(&suffix),
        );
    }

    dirs.extend(
        glib::system_data_dirs()
            .into_iter()
            .map(|d| d.join(&suffix)),
    );
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

fn current_accelerator(settings: &gio::Settings) -> String {
    settings
        .strv("capture-shortcut")
        .first()
        .map(|s| s.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A detailed action string like "word.edit(3)" parses its target as
    /// int32, while the actions take int64 -- and GTK silently drops an
    /// activation whose target type does not match, so every row menu entry
    /// became a no-op. Keep the targets explicitly typed.
    #[test]
    fn row_action_targets_are_int64() {
        for action in [
            "word.edit",
            "word.sense",
            "word.learned",
            "word.restore",
            "word.delete",
        ] {
            let item = row_action("Label", action, 3);
            let target = item
                .attribute_value("target", None)
                .expect("the item carries a target");
            assert_eq!(
                target.type_().as_str(),
                "x",
                "{action} target must be int64, got {}",
                target.type_().as_str()
            );
            assert_eq!(target.get::<i64>(), Some(3));
        }
    }

    #[test]
    fn a_detailed_action_string_would_have_produced_int32() {
        // Documents the original defect: this is what the code used to build.
        let (_, target) = gio::Action::parse_detailed_name("word.edit(3)").expect("parses");
        let target = target.expect("has a target");
        assert_eq!(target.type_().as_str(), "i", "still int32, hence the bug");
    }
}
