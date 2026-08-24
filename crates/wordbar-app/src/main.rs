//! Wordbar vocabulary manager.
//!
//! A thin GTK4/libadwaita front end over the same D-Bus API the GNOME
//! extension and `wordbarctl` use. It never opens the database.

mod client;
mod edit;
mod window;
mod word;

use adw::prelude::*;
use clap::Parser;

const APP_ID: &str = "org.wordbar.Wordbar";

#[derive(Parser)]
#[command(name = "wordbar-app", version, about = "Wordbar vocabulary manager")]
struct Cli {
    /// Page to open: vocabulary or preferences.
    #[arg(long, default_value = "vocabulary")]
    page: String,
}

fn main() -> glib::ExitCode {
    let cli = Cli::parse();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let client = client::Client::start();
    app.connect_activate(move |app| {
        let window = window::build(app, client.clone(), &cli.page);
        window.present();
    });

    // clap already consumed our arguments.
    app.run_with_args::<&str>(&[])
}
