//! Wordbar vocabulary manager.
//!
//! A thin GTK4/libadwaita front end over the same D-Bus API the GNOME
//! extension and `wordbarctl` use. It never opens the database.

mod client;
mod edit;
mod window;
mod word;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use clap::Parser;

/// Deliberately NOT org.wordbar.Wordbar: that name belongs to the service.
/// Sharing it makes GApplication treat the service as the primary instance,
/// fail to register, and exit -- leaving a zombie per launch.
const APP_ID: &str = "org.wordbar.Manager";

#[derive(Parser, Debug)]
#[command(name = "wordbar-app", version, about = "Wordbar vocabulary manager")]
struct Cli {
    /// Page to open: vocabulary or preferences.
    #[arg(long, default_value = "vocabulary")]
    page: String,

    /// Open the editor for this word id straight away.
    #[arg(long)]
    edit: Option<i64>,

    /// Show only words that still need a definition.
    #[arg(long)]
    incomplete: bool,
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        // HANDLES_COMMAND_LINE so a second launch hands its arguments to the
        // running instance instead of opening another window. The service
        // spawns us on every "Open vocabulary", and without this each click
        // left another copy of the manager on screen.
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let client = client::Client::start();
    let ui: Rc<RefCell<Option<Rc<window::Ui>>>> = Rc::new(RefCell::new(None));

    app.connect_command_line(move |app, command_line| {
        let cli = match Cli::try_parse_from(command_line.arguments()) {
            Ok(cli) => cli,
            Err(err) => {
                eprintln!("{err}");
                return 1;
            }
        };

        let existing = ui.borrow().clone();
        let ui_ref = match existing {
            Some(ui_ref) => ui_ref,
            None => {
                let (window, built) = window::build(app, client.clone(), &cli.page);
                window.present();
                *ui.borrow_mut() = Some(Rc::clone(&built));
                built
            }
        };

        ui_ref.present_window(&cli.page);
        if cli.incomplete {
            ui_ref.show_incomplete_only();
        }
        if let Some(id) = cli.edit {
            ui_ref.edit_word(id);
        }
        0
    });

    app.run()
}
