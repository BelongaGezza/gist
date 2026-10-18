//! The Import file chooser.
//!
//! `GtkNativeDialog` is not a widget and GTK does not keep it alive: the caller
//! must hold a reference until the user answers. Dropping it straight after
//! `show()` destroyed the dialog before it painted wherever there is no desktop
//! portal (for example WSLg), so Import appeared to do nothing. [`ChooserSlot`]
//! holds the chooser for exactly as long as it is open, and refuses a second one.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;

pub const SUPPORTED_EXTENSIONS: [&str; 6] = ["txt", "text", "md", "epub", "docx", "pdf"];

/// Owns the open chooser, if any.
#[derive(Clone, Default)]
pub struct ChooserSlot(Rc<RefCell<Option<gtk::FileChooserNative>>>);

impl ChooserSlot {
    pub fn is_open(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// Show the chooser. `on_path` gets the picked file's path (`None` if the
    /// pick has no local path); it is not called on cancel. Does nothing if a
    /// chooser is already open.
    pub fn present(
        &self,
        window: &gtk::ApplicationWindow,
        on_path: impl Fn(Option<PathBuf>) + 'static,
    ) {
        if self.is_open() {
            return;
        }
        let chooser = gtk::FileChooserNative::new(
            Some("Import a document"),
            Some(window),
            gtk::FileChooserAction::Open,
            Some("Import"),
            Some("Cancel"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Supported documents"));
        for extension in SUPPORTED_EXTENSIONS {
            filter.add_pattern(&format!("*.{extension}"));
        }
        chooser.add_filter(&filter);

        let slot = Rc::clone(&self.0);
        chooser.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                on_path(dialog.file().and_then(|file| file.path()));
            }
            dialog.destroy();
            // Release only now that the user has answered.
            slot.borrow_mut().take();
        });
        *self.0.borrow_mut() = Some(chooser.clone());
        chooser.show();
    }
}
