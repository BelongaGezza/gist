//! Display check for the Import chooser (needs a graphical session; not run in CI).
//! Exercises the real `ChooserSlot`: a dialog window must appear and stay up,
//! a second open must be ignored, and cancelling must release the slot.
//!     cargo run --example chooser_probe        (prints PASS/FAIL, exit code 0/1)
#[path = "../src/chooser.rs"]
mod chooser;

use gtk::{glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;

fn dialogs() -> Vec<gtk::FileChooserDialog> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::FileChooserDialog>().ok())
        .collect()
}

fn main() {
    let failed = Rc::new(Cell::new(false));
    let app = gtk::Application::builder()
        .application_id("dev.gist.ChooserProbe")
        .build();
    let failed_in = Rc::clone(&failed);
    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("probe")
            .build();
        window.present();
        let slot = chooser::ChooserSlot::default();
        slot.present(&window, |_| ());
        slot.present(&window, |_| ()); // second open must be ignored

        let (app, failed) = (app.clone(), Rc::clone(&failed_in));
        glib::timeout_add_seconds_local_once(2, move || {
            let check = |ok: bool, what: &str| {
                println!("{} {what}", if ok { "ok  " } else { "FAIL" });
                if !ok {
                    failed.set(true);
                }
            };
            let open = dialogs();
            check(slot.is_open(), "slot holds the chooser while it is open");
            check(
                open.len() == 1,
                &format!(
                    "exactly one chooser window is visible (found {})",
                    open.len()
                ),
            );
            if let Some(dialog) = open.first() {
                dialog.response(gtk::ResponseType::Cancel);
            }
            let (app, failed2) = (app.clone(), Rc::clone(&failed));
            glib::timeout_add_seconds_local_once(1, move || {
                let ok = !slot.is_open() && dialogs().is_empty();
                println!(
                    "{} cancel releases the slot and closes the window",
                    if ok { "ok  " } else { "FAIL" }
                );
                if !ok {
                    failed2.set(true);
                }
                app.quit();
            });
        });
    });
    app.run_with_args::<&str>(&[]);
    println!("{}", if failed.get() { "FAIL" } else { "PASS" });
    std::process::exit(i32::from(failed.get()));
}
