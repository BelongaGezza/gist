use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use gist_core::Core;
use gist_model::{Block, Document, TokenKind};
use gist_rsvp::{Config, RsvpSession};
use gist_store::LibraryItem;
use gtk::{glib, prelude::*};

const PAGE_SIZE: usize = 200;

enum UiMessage {
    Ready(Arc<Core>),
    ItemsLoaded {
        query: String,
        items: Vec<LibraryItem>,
    },
    RefreshLibrary,
    FlowLoaded {
        title: String,
        text: String,
    },
    RsvpLoaded {
        item_id: String,
        title: String,
        session: RsvpSession,
    },
    Status(String),
    Failed(String),
}

struct RsvpPlayback {
    item_id: String,
    session: RsvpSession,
    playing_since: Option<Instant>,
    last_saved: Instant,
}

fn main() {
    let app = gtk::Application::builder()
        .application_id("dev.gist.LinuxGtkSpike")
        .build();
    app.connect_activate(build_window);
    app.run();
}

fn build_window(app: &gtk::Application) {
    let (sender, receiver) = mpsc::channel();
    let core_state = Rc::new(RefCell::new(None::<Arc<Core>>));
    let visible_items = Rc::new(RefCell::new(Vec::<LibraryItem>::new()));
    let selected_item = Rc::new(RefCell::new(None::<LibraryItem>));
    let playback = Rc::new(RefCell::new(None::<RsvpPlayback>));

    let import_button = gtk::Button::with_label("Import");
    let remove_button = gtk::Button::with_label("Remove");
    let flow_button = gtk::Button::with_label("Flow");
    let rsvp_button = gtk::Button::with_label("RSVP");
    let search_entry = gtk::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search your library"));
    search_entry.set_hexpand(true);
    for button in [&remove_button, &flow_button, &rsvp_button] {
        button.set_sensitive(false);
    }

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.append(&import_button);
    toolbar.append(&search_entry);
    toolbar.append(&remove_button);
    toolbar.append(&flow_button);
    toolbar.append(&rsvp_button);

    let library_list = gtk::ListBox::new();
    library_list.set_selection_mode(gtk::SelectionMode::Single);
    library_list.set_vexpand(true);
    let library_scroll = gtk::ScrolledWindow::builder()
        .min_content_width(260)
        .vexpand(true)
        .child(&library_list)
        .build();

    let reader_title = gtk::Label::new(Some("Select a book to start reading"));
    reader_title.set_xalign(0.0);
    reader_title.add_css_class("title-2");

    let flow_text = gtk::TextView::new();
    flow_text.set_editable(false);
    flow_text.set_cursor_visible(false);
    flow_text.set_wrap_mode(gtk::WrapMode::WordChar);
    flow_text.set_left_margin(20);
    flow_text.set_right_margin(20);
    flow_text.set_top_margin(20);
    flow_text.set_bottom_margin(20);
    let flow_scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&flow_text)
        .build();

    let rsvp_word = gtk::Label::new(Some("Choose RSVP to begin"));
    rsvp_word.set_hexpand(true);
    rsvp_word.set_vexpand(true);
    rsvp_word.set_halign(gtk::Align::Center);
    rsvp_word.set_valign(gtk::Align::Center);
    rsvp_word.add_css_class("title-1");

    let position_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    position_scale.set_draw_value(false);
    position_scale.set_round_digits(0);
    position_scale.set_hexpand(true);
    position_scale.set_sensitive(false);
    position_scale.set_tooltip_text(Some("Position in text"));
    let position_label = gtk::Label::new(Some("0 / 0"));
    position_label.add_css_class("numeric");
    position_label.set_width_chars(10);
    position_label.set_xalign(1.0);

    let play_button = gtk::Button::with_label("Play");
    let wpm_label = gtk::Label::new(Some("250 WPM"));
    wpm_label.add_css_class("numeric");
    wpm_label.set_width_chars(8);
    wpm_label.set_xalign(1.0);
    wpm_label.add_css_class("title-4");
    let wpm_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 100.0, 1000.0, 25.0);
    wpm_scale.set_draw_value(false);
    wpm_scale.set_round_digits(0);
    wpm_scale.set_hexpand(true);
    wpm_scale.set_size_request(240, -1);
    wpm_scale.set_value(250.0);
    wpm_scale.set_sensitive(false);
    wpm_scale.set_tooltip_text(Some("Reading speed in words per minute"));
    let back_button = gtk::Button::with_label("Back 5 words");
    for button in [&play_button, &back_button] {
        button.set_sensitive(false);
    }
    let rsvp_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    rsvp_controls.set_halign(gtk::Align::Center);
    rsvp_controls.append(&play_button);
    rsvp_controls.append(&back_button);
    let position_controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    position_controls.set_margin_bottom(8);
    let position_caption = gtk::Label::new(Some("Position"));
    position_caption.add_css_class("dim-label");
    position_controls.append(&position_caption);
    position_controls.append(&position_scale);
    position_controls.append(&position_label);
    let speed_controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    speed_controls.set_halign(gtk::Align::Center);
    speed_controls.set_margin_top(8);
    let speed_caption = gtk::Label::new(Some("Speed"));
    speed_caption.add_css_class("dim-label");
    speed_controls.append(&speed_caption);
    speed_controls.append(&wpm_scale);
    speed_controls.append(&wpm_label);
    let rsvp_page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    rsvp_page.append(&rsvp_word);
    rsvp_page.append(&position_controls);
    rsvp_page.append(&rsvp_controls);
    rsvp_page.append(&speed_controls);

    let reader_stack = gtk::Stack::new();
    reader_stack.set_hexpand(true);
    reader_stack.set_vexpand(true);
    reader_stack.add_named(&flow_scroll, Some("flow"));
    reader_stack.add_named(&rsvp_page, Some("rsvp"));

    let reader = gtk::Box::new(gtk::Orientation::Vertical, 12);
    reader.set_margin_top(12);
    reader.set_margin_bottom(12);
    reader.set_margin_start(12);
    reader.set_margin_end(12);
    reader.append(&reader_title);
    reader.append(&reader_stack);

    let body = gtk::Paned::new(gtk::Orientation::Horizontal);
    body.set_wide_handle(true);
    body.set_start_child(Some(&library_scroll));
    body.set_end_child(Some(&reader));
    body.set_position(280);

    let status = gtk::Label::new(Some("Opening library…"));
    status.set_xalign(0.0);
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.append(&toolbar);
    root.append(&body);
    root.append(&status);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("GIST — Linux GTK4")
        .default_width(1120)
        .default_height(740)
        .child(&root)
        .build();

    connect_import(
        &import_button,
        &window,
        Rc::clone(&core_state),
        sender.clone(),
    );
    connect_library_selection(
        &library_list,
        Rc::clone(&visible_items),
        Rc::clone(&selected_item),
        &remove_button,
        &flow_button,
        &rsvp_button,
    );
    connect_open_reader(
        &flow_button,
        Rc::clone(&core_state),
        Rc::clone(&selected_item),
        Rc::clone(&playback),
        sender.clone(),
        ReaderMode::Flow,
    );
    connect_open_reader(
        &rsvp_button,
        Rc::clone(&core_state),
        Rc::clone(&selected_item),
        Rc::clone(&playback),
        sender.clone(),
        ReaderMode::Rsvp,
    );
    connect_remove(
        &remove_button,
        &window,
        Rc::clone(&core_state),
        Rc::clone(&selected_item),
        sender.clone(),
    );
    connect_search(&search_entry, Rc::clone(&core_state), sender.clone());
    let position_syncing = Rc::new(Cell::new(false));
    let position_scrubbing = Rc::new(Cell::new(false));
    connect_rsvp_controls(
        &play_button,
        &position_scale,
        &position_label,
        Rc::clone(&position_syncing),
        Rc::clone(&position_scrubbing),
        &rsvp_word,
        &wpm_scale,
        &wpm_label,
        &back_button,
        Rc::clone(&playback),
        Rc::clone(&core_state),
        sender.clone(),
    );
    connect_messages(
        receiver,
        &window,
        Rc::clone(&core_state),
        Rc::clone(&visible_items),
        Rc::clone(&selected_item),
        Rc::clone(&playback),
        &library_list,
        &remove_button,
        &flow_button,
        &rsvp_button,
        &play_button,
        &position_scale,
        &position_label,
        Rc::clone(&position_syncing),
        Rc::clone(&position_scrubbing),
        &wpm_scale,
        &wpm_label,
        &back_button,
        &reader_title,
        &reader_stack,
        &flow_text,
        &rsvp_word,
        &status,
        &search_entry,
        sender.clone(),
    );

    let playback_on_close = Rc::clone(&playback);
    let core_on_close = Rc::clone(&core_state);
    window.connect_close_request(move |_| {
        save_playback(&playback_on_close, &core_on_close);
        glib::Propagation::Proceed
    });

    window.present();
    initialize_library(sender);
}

#[derive(Clone, Copy)]
enum ReaderMode {
    Flow,
    Rsvp,
}

fn connect_import(
    button: &gtk::Button,
    window: &gtk::ApplicationWindow,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    sender: mpsc::Sender<UiMessage>,
) {
    let weak_window = window.downgrade();
    button.connect_clicked(move |_| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        let Some(core) = core_state.borrow().as_ref().map(Arc::clone) else {
            return;
        };
        let chooser = gtk::FileChooserNative::new(
            Some("Import a document"),
            Some(&window),
            gtk::FileChooserAction::Open,
            Some("Import"),
            Some("Cancel"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Supported documents"));
        for extension in ["txt", "text", "md", "epub", "docx", "pdf"] {
            filter.add_pattern(&format!("*.{extension}"));
        }
        chooser.add_filter(&filter);
        let sender = sender.clone();
        chooser.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                match dialog.file().and_then(|file| file.path()) {
                    Some(path) => import_file(Arc::clone(&core), path, sender.clone()),
                    None => {
                        let _ = sender.send(UiMessage::Failed(
                            "The selected document has no local file path.".into(),
                        ));
                    }
                }
            }
            dialog.destroy();
        });
        chooser.show();
    });
}

fn connect_library_selection(
    list: &gtk::ListBox,
    visible_items: Rc<RefCell<Vec<LibraryItem>>>,
    selected_item: Rc<RefCell<Option<LibraryItem>>>,
    remove_button: &gtk::Button,
    flow_button: &gtk::Button,
    rsvp_button: &gtk::Button,
) {
    let remove_button = remove_button.downgrade();
    let flow_button = flow_button.downgrade();
    let rsvp_button = rsvp_button.downgrade();
    list.connect_row_selected(move |_, row| {
        let selected = row.and_then(|row| {
            usize::try_from(row.index())
                .ok()
                .and_then(|index| visible_items.borrow().get(index).cloned())
        });
        let enabled = selected.is_some();
        *selected_item.borrow_mut() = selected;
        if let Some(button) = remove_button.upgrade() {
            button.set_sensitive(enabled);
        }
        if let Some(button) = flow_button.upgrade() {
            button.set_sensitive(enabled);
        }
        if let Some(button) = rsvp_button.upgrade() {
            button.set_sensitive(enabled);
        }
    });
}

fn connect_search(
    search_entry: &gtk::SearchEntry,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    sender: mpsc::Sender<UiMessage>,
) {
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));
    search_entry.connect_search_changed(move |entry| {
        if let Some(source) = pending.borrow_mut().take() {
            source.remove();
        }
        let query = entry.text().to_string();
        let Some(core) = core_state.borrow().as_ref().map(Arc::clone) else {
            return;
        };
        let sender = sender.clone();
        let pending_for_callback = Rc::clone(&pending);
        let source = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            pending_for_callback.borrow_mut().take();
            list_items(core, query, sender);
        });
        *pending.borrow_mut() = Some(source);
    });
}

fn connect_open_reader(
    button: &gtk::Button,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    selected_item: Rc<RefCell<Option<LibraryItem>>>,
    playback: Rc<RefCell<Option<RsvpPlayback>>>,
    sender: mpsc::Sender<UiMessage>,
    mode: ReaderMode,
) {
    button.connect_clicked(move |_| {
        let (Some(core), Some(item)) = (
            core_state.borrow().as_ref().map(Arc::clone),
            selected_item.borrow().clone(),
        ) else {
            return;
        };
        save_playback(&playback, &core_state);
        *playback.borrow_mut() = None;
        let sender = sender.clone();
        match mode {
            ReaderMode::Flow => load_flow(core, item.id, item.title.unwrap_or_default(), sender),
            ReaderMode::Rsvp => load_rsvp(core, item.id, item.title.unwrap_or_default(), sender),
        }
    });
}

fn connect_remove(
    button: &gtk::Button,
    window: &gtk::ApplicationWindow,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    selected_item: Rc<RefCell<Option<LibraryItem>>>,
    sender: mpsc::Sender<UiMessage>,
) {
    let weak_window = window.downgrade();
    button.connect_clicked(move |_| {
        let (Some(window), Some(core), Some(item)) = (
            weak_window.upgrade(),
            core_state.borrow().as_ref().map(Arc::clone),
            selected_item.borrow().clone(),
        ) else {
            return;
        };
        let title = item.title.as_deref().unwrap_or("Untitled");
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&window)
            .modal(true)
            .message_type(gtk::MessageType::Warning)
            .buttons(gtk::ButtonsType::Cancel)
            .text(format!("Remove “{title}” from GIST?"))
            .secondary_text(
                "This deletes GIST's internal copy and library entry. The file you originally selected is never deleted.",
            )
            .build();
        dialog.add_button("Remove", gtk::ResponseType::Accept);
        let sender = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                remove_item(Arc::clone(&core), item.id.clone(), sender.clone());
            }
            dialog.close();
        });
        dialog.present();
    });
}

#[allow(clippy::too_many_arguments)]
fn connect_rsvp_controls(
    play_button: &gtk::Button,
    position_scale: &gtk::Scale,
    position_label: &gtk::Label,
    position_syncing: Rc<Cell<bool>>,
    position_scrubbing: Rc<Cell<bool>>,
    word_label: &gtk::Label,
    wpm_scale: &gtk::Scale,
    wpm_label: &gtk::Label,
    back_button: &gtk::Button,
    playback: Rc<RefCell<Option<RsvpPlayback>>>,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    sender: mpsc::Sender<UiMessage>,
) {
    let playback_for_play = Rc::clone(&playback);
    let core_for_play = Rc::clone(&core_state);
    let sender_for_play = sender.clone();
    let scrubbing_for_play = Rc::clone(&position_scrubbing);
    let position_scale_for_play = position_scale.downgrade();
    let position_label_for_play = position_label.downgrade();
    let position_syncing_for_play = Rc::clone(&position_syncing);
    play_button.connect_clicked(move |button| {
        let mut playback = playback_for_play.borrow_mut();
        let Some(playback) = playback.as_mut() else {
            return;
        };
        match playback.playing_since.take() {
            Some(since) => {
                playback.session.pause(elapsed_ms(since));
                button.set_label("Play");
                if let (Some(scale), Some(label)) = (
                    position_scale_for_play.upgrade(),
                    position_label_for_play.upgrade(),
                ) {
                    sync_position_widgets(
                        &scale,
                        &label,
                        playback.session.cursor,
                        playback.session.tokens.len(),
                        &position_syncing_for_play,
                        &scrubbing_for_play,
                    );
                }
                save_progress_async(
                    &core_for_play,
                    playback.item_id.clone(),
                    playback.session.cursor,
                    sender_for_play.clone(),
                );
            }
            None => {
                playback.session.resume();
                playback.playing_since = Some(Instant::now());
                button.set_label("Pause");
            }
        }
    });

    let playback_for_seek = Rc::clone(&playback);
    let scrubbing_for_seek = Rc::clone(&position_scrubbing);
    let pending_seek_source = Rc::new(RefCell::new(None::<glib::SourceId>));
    let pending_seek_value = Rc::new(RefCell::new(None::<(String, usize)>));
    let word_label_for_seek = word_label.downgrade();
    let position_label_for_seek = position_label.downgrade();
    let position_syncing_for_seek = Rc::clone(&position_syncing);
    let pending_source_for_seek = Rc::clone(&pending_seek_source);
    let pending_value_for_seek = Rc::clone(&pending_seek_value);
    let core_for_debounce = Rc::clone(&core_state);
    let sender_for_debounce = sender.clone();
    let scrubbing_for_debounce = Rc::clone(&position_scrubbing);
    position_scale.connect_value_changed(move |scale| {
        if position_syncing_for_seek.get() {
            return;
        }
        scrubbing_for_seek.set(true);
        let target = scale.value().round().max(0.0) as usize;
        let now = Instant::now();
        let Some((position, item_id, token_count)) =
            playback_for_seek.borrow_mut().as_mut().map(|playback| {
                let position = seek_playback(playback, target, now);
                (
                    position,
                    playback.item_id.clone(),
                    playback.session.tokens.len(),
                )
            })
        else {
            return;
        };
        if let Some(label) = word_label_for_seek.upgrade() {
            let playback = playback_for_seek.borrow();
            if let Some(playback) = playback.as_ref() {
                set_current_word(&label, &playback.session, position);
            }
        }
        if let Some(label) = position_label_for_seek.upgrade() {
            label.set_text(&position_text(position, token_count));
        }
        if target != position {
            set_scale_value(&position_syncing_for_seek, scale, position as f64);
        }
        *pending_value_for_seek.borrow_mut() = Some((item_id, position));
        if let Some(source) = pending_source_for_seek.borrow_mut().take() {
            source.remove();
        }
        let pending_value = Rc::clone(&pending_value_for_seek);
        let pending_source = Rc::clone(&pending_source_for_seek);
        let core = Rc::clone(&core_for_debounce);
        let sender = sender_for_debounce.clone();
        let scrubbing = Rc::clone(&scrubbing_for_debounce);
        let source = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            pending_source.borrow_mut().take();
            scrubbing.set(false);
            if let Some((item_id, position)) = pending_value.borrow_mut().take() {
                save_progress_async(&core, item_id, position, sender);
            }
        });
        *pending_source_for_seek.borrow_mut() = Some(source);
    });

    let scrubbing_for_gesture = Rc::clone(&position_scrubbing);
    let pending_source_for_release = Rc::clone(&pending_seek_source);
    let pending_value_for_release = Rc::clone(&pending_seek_value);
    let core_for_release = Rc::clone(&core_state);
    let sender_for_release = sender.clone();
    let gesture = gtk::GestureClick::new();
    gesture.connect_pressed(move |_, _, _, _| scrubbing_for_gesture.set(true));
    let scrubbing_for_release = Rc::clone(&position_scrubbing);
    gesture.connect_released(move |_, _, _, _| {
        scrubbing_for_release.set(false);
        if let Some(source) = pending_source_for_release.borrow_mut().take() {
            source.remove();
        }
        if let Some((item_id, position)) = pending_value_for_release.borrow_mut().take() {
            save_progress_async(
                &core_for_release,
                item_id,
                position,
                sender_for_release.clone(),
            );
        }
    });
    position_scale.add_controller(gesture);

    let playback_for_speed = Rc::clone(&playback);
    let core_for_speed = Rc::clone(&core_state);
    let sender_for_speed = sender.clone();
    let label_for_speed = wpm_label.downgrade();
    let setting_scale = Rc::new(Cell::new(false));
    let setting_scale_for_signal = Rc::clone(&setting_scale);
    wpm_scale.connect_value_changed(move |scale| {
        if setting_scale_for_signal.get() {
            return;
        }
        let target = normalize_wpm(scale.value());
        let Some(label) = label_for_speed.upgrade() else {
            return;
        };
        if let Some(wpm) = change_wpm(
            &playback_for_speed,
            &core_for_speed,
            target,
            sender_for_speed.clone(),
        ) {
            label.set_text(&format!("{wpm} WPM"));
            if target != wpm {
                setting_scale_for_signal.set(true);
                scale.set_value(f64::from(wpm));
                setting_scale_for_signal.set(false);
            }
        }
    });

    let playback_for_back = Rc::clone(&playback);
    let core_for_back = Rc::clone(&core_state);
    let sender_for_back = sender.clone();
    back_button.connect_clicked(move |_| {
        let mut state = playback_for_back.borrow_mut();
        let Some(playback) = state.as_mut() else {
            return;
        };
        let was_playing = playback.playing_since.take();
        let elapsed = was_playing.map(elapsed_ms).unwrap_or(0);
        playback.session.back_words(5, elapsed);
        if was_playing.is_some() {
            playback.playing_since = Some(Instant::now());
        }
        save_progress_async(
            &core_for_back,
            playback.item_id.clone(),
            playback.session.cursor,
            sender_for_back.clone(),
        );
    });
}

#[allow(clippy::too_many_arguments)]
fn connect_messages(
    receiver: mpsc::Receiver<UiMessage>,
    window: &gtk::ApplicationWindow,
    core_state: Rc<RefCell<Option<Arc<Core>>>>,
    visible_items: Rc<RefCell<Vec<LibraryItem>>>,
    selected_item: Rc<RefCell<Option<LibraryItem>>>,
    playback: Rc<RefCell<Option<RsvpPlayback>>>,
    library_list: &gtk::ListBox,
    remove_button: &gtk::Button,
    flow_button: &gtk::Button,
    rsvp_button: &gtk::Button,
    play_button: &gtk::Button,
    position_scale: &gtk::Scale,
    position_label: &gtk::Label,
    position_syncing: Rc<Cell<bool>>,
    position_scrubbing: Rc<Cell<bool>>,
    wpm_scale: &gtk::Scale,
    wpm_label: &gtk::Label,
    back_button: &gtk::Button,
    reader_title: &gtk::Label,
    reader_stack: &gtk::Stack,
    flow_text: &gtk::TextView,
    rsvp_word: &gtk::Label,
    status: &gtk::Label,
    search_entry: &gtk::SearchEntry,
    sender: mpsc::Sender<UiMessage>,
) {
    let window = window.downgrade();
    let list = library_list.downgrade();
    let remove_button = remove_button.downgrade();
    let flow_button = flow_button.downgrade();
    let rsvp_button = rsvp_button.downgrade();
    let play_button = play_button.downgrade();
    let position_scale = position_scale.downgrade();
    let position_label = position_label.downgrade();
    let wpm_scale = wpm_scale.downgrade();
    let wpm_label = wpm_label.downgrade();
    let back_button = back_button.downgrade();
    let reader_title = reader_title.downgrade();
    let reader_stack = reader_stack.downgrade();
    let flow_text = flow_text.downgrade();
    let rsvp_word = rsvp_word.downgrade();
    let status = status.downgrade();
    let search_entry = search_entry.downgrade();

    glib::timeout_add_local(Duration::from_millis(30), move || {
        loop {
            let message = match receiver.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            };
            let (
                Some(window),
                Some(list),
                Some(remove_button),
                Some(flow_button),
                Some(rsvp_button),
                Some(play_button),
                Some(position_scale),
                Some(position_label),
                Some(wpm_scale),
                Some(wpm_label),
                Some(back_button),
                Some(reader_title),
                Some(reader_stack),
                Some(flow_text),
                Some(rsvp_word),
                Some(status),
                Some(search_entry),
            ) = (
                window.upgrade(),
                list.upgrade(),
                remove_button.upgrade(),
                flow_button.upgrade(),
                rsvp_button.upgrade(),
                play_button.upgrade(),
                position_scale.upgrade(),
                position_label.upgrade(),
                wpm_scale.upgrade(),
                wpm_label.upgrade(),
                back_button.upgrade(),
                reader_title.upgrade(),
                reader_stack.upgrade(),
                flow_text.upgrade(),
                rsvp_word.upgrade(),
                status.upgrade(),
                search_entry.upgrade(),
            )
            else {
                return glib::ControlFlow::Break;
            };

            match message {
                UiMessage::Ready(core) => {
                    *core_state.borrow_mut() = Some(Arc::clone(&core));
                    status.set_text("Library ready.");
                    list_items(core, search_entry.text().to_string(), sender.clone());
                }
                UiMessage::ItemsLoaded { query, items } => {
                    if search_entry.text().as_str() != query {
                        continue;
                    }
                    render_items(&list, &items);
                    let count = items.len();
                    *visible_items.borrow_mut() = items;
                    *selected_item.borrow_mut() = None;
                    remove_button.set_sensitive(false);
                    flow_button.set_sensitive(false);
                    rsvp_button.set_sensitive(false);
                    if count == 0 {
                        status.set_text(if query.is_empty() {
                            "Your library is empty. Import a document to get started."
                        } else {
                            "No matching documents."
                        });
                    } else {
                        status.set_text(&format!("{count} document(s)"));
                    }
                }
                UiMessage::RefreshLibrary => {
                    if let Some(core) = core_state.borrow().as_ref().map(Arc::clone) {
                        list_items(core, search_entry.text().to_string(), sender.clone());
                    }
                }
                UiMessage::FlowLoaded { title, text } => {
                    reader_title.set_text(&title);
                    flow_text.buffer().set_text(&text);
                    reader_stack.set_visible_child_name("flow");
                    *playback.borrow_mut() = None;
                    for button in [&play_button, &back_button] {
                        button.set_sensitive(false);
                    }
                    position_scale.set_sensitive(false);
                    position_label.set_text("0 / 0");
                    wpm_scale.set_sensitive(false);
                    status.set_text("Flow reader");
                }
                UiMessage::RsvpLoaded {
                    item_id,
                    title,
                    session,
                } => {
                    reader_title.set_text(&title);
                    reader_stack.set_visible_child_name("rsvp");
                    set_current_word(&rsvp_word, &session, session.cursor);
                    let wpm = session.config.wpm;
                    let cursor = session.cursor;
                    let token_count = session.tokens.len();
                    play_button.set_label("Play");
                    position_scale.set_range(0.0, token_count.saturating_sub(1).max(1) as f64);
                    position_scale.set_sensitive(token_count > 1);
                    sync_position_widgets(
                        &position_scale,
                        &position_label,
                        cursor,
                        token_count,
                        &position_syncing,
                        &position_scrubbing,
                    );
                    *playback.borrow_mut() = Some(RsvpPlayback {
                        item_id,
                        session,
                        playing_since: None,
                        last_saved: Instant::now(),
                    });
                    wpm_label.set_text(&format!("{wpm} WPM"));
                    wpm_scale.set_value(f64::from(wpm));
                    for button in [&play_button, &back_button] {
                        button.set_sensitive(true);
                    }
                    wpm_scale.set_sensitive(true);
                    status.set_text("RSVP ready");
                }
                UiMessage::Status(message) => status.set_text(&message),
                UiMessage::Failed(error) => {
                    status.set_text(&error);
                    show_error(&window, &error);
                }
            }
        }
        if let (
            Some(rsvp_word),
            Some(status),
            Some(play_button),
            Some(position_scale),
            Some(position_label),
        ) = (
            rsvp_word.upgrade(),
            status.upgrade(),
            play_button.upgrade(),
            position_scale.upgrade(),
            position_label.upgrade(),
        ) {
            tick_rsvp(
                &playback,
                &core_state,
                &rsvp_word,
                &status,
                &play_button,
                &position_scale,
                &position_label,
                &position_syncing,
                &position_scrubbing,
                &sender,
            );
        }
        glib::ControlFlow::Continue
    });
}

fn initialize_library(sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || match initialize_core() {
        Ok(core) => {
            let _ = sender.send(UiMessage::Ready(core));
        }
        Err(error) => {
            let _ = sender.send(UiMessage::Failed(error));
        }
    });
}

fn initialize_core() -> Result<Arc<Core>, String> {
    let data_dir = data_dir()
        .ok_or_else(|| "Could not determine a home directory for the Linux GTK app.".to_string())?;
    let storage_dir = data_dir.join("storage");
    fs::create_dir_all(&storage_dir)
        .map_err(|error| format!("Could not create {}: {error}", storage_dir.display()))?;
    Core::init(&data_dir.join("gist.sqlite3"), &storage_dir)
        .map(Arc::new)
        .map_err(|error| format!("Could not initialize the GIST core: {error}"))
}

fn list_items(core: Arc<Core>, query: String, sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || {
        let result = if query.trim().is_empty() {
            core.list_items(0, PAGE_SIZE)
        } else {
            core.search_items(query.trim(), PAGE_SIZE)
        };
        match result {
            Ok(items) => {
                let _ = sender.send(UiMessage::ItemsLoaded { query, items });
            }
            Err(error) => {
                let _ = sender.send(UiMessage::Failed(format!(
                    "Could not load library: {error}"
                )));
            }
        }
    });
}

fn import_file(core: Arc<Core>, path: PathBuf, sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || {
        let name = file_title(&path);
        match core.import_file(&path, &gist_core::NullObserver) {
            Ok(_) => {
                let _ = sender.send(UiMessage::Status(format!("Imported {name}")));
                let _ = sender.send(UiMessage::RefreshLibrary);
            }
            Err(error) => {
                let _ = sender.send(UiMessage::Failed(format!(
                    "Could not import {name}: {error}"
                )));
            }
        }
    });
}

fn remove_item(core: Arc<Core>, item_id: String, sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || match core.remove_items_detailed(&[item_id], true) {
        Ok(outcome) => {
            let _ = sender.send(UiMessage::Status(format!(
                "Removed from library; internal files deleted: {}",
                outcome.files_deleted
            )));
            let _ = sender.send(UiMessage::RefreshLibrary);
        }
        Err(error) => {
            let _ = sender.send(UiMessage::Failed(format!("Could not remove item: {error}")));
        }
    });
}

fn load_flow(core: Arc<Core>, item_id: String, title: String, sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || {
        let result = core
            .get_document(&item_id)
            .map_err(|error| error.to_string())
            .and_then(|json| {
                serde_json::from_str::<Document>(&json).map_err(|error| error.to_string())
            })
            .map(|document| document_text(&document));
        match result {
            Ok(text) => {
                if let Err(error) = core.mark_item_opened(&item_id) {
                    let _ = sender.send(UiMessage::Failed(format!(
                        "Opened the document but could not update reading state: {error}"
                    )));
                    return;
                }
                let _ = sender.send(UiMessage::FlowLoaded { title, text });
            }
            Err(error) => {
                let _ = sender.send(UiMessage::Failed(format!(
                    "Could not open document: {error}"
                )));
            }
        }
    });
}

fn load_rsvp(core: Arc<Core>, item_id: String, title: String, sender: mpsc::Sender<UiMessage>) {
    thread::spawn(move || {
        let result = core
            .new_rsvp_session(&item_id, Config::default())
            .map_err(|error| error.to_string());
        match result {
            Ok(session) => {
                if let Err(error) = core.mark_item_opened(&item_id) {
                    let _ = sender.send(UiMessage::Failed(format!(
                        "Opened the document but could not update reading state: {error}"
                    )));
                    return;
                }
                let _ = sender.send(UiMessage::RsvpLoaded {
                    item_id,
                    title,
                    session,
                });
            }
            Err(error) => {
                let _ = sender.send(UiMessage::Failed(format!("Could not open RSVP: {error}")));
            }
        }
    });
}

fn document_text(document: &Document) -> String {
    document
        .sections
        .iter()
        .flat_map(|section| {
            let heading = section
                .heading
                .as_ref()
                .map(|(_, heading)| format!("{heading}\n\n"));
            heading
                .into_iter()
                .chain(section.blocks.iter().map(block_text))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn block_text(block: &Block) -> String {
    block.plain_text()
}

fn render_items(list: &gtk::ListBox, items: &[LibraryItem]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for item in items {
        let title = item.title.as_deref().unwrap_or("Untitled");
        let subtitle = format!(
            "{}  ·  {}% read{}",
            if item.source_type.is_empty() {
                "Document"
            } else {
                item.source_type.as_str()
            },
            (item.progress_fraction * 100.0).round() as u32,
            if item.content_encrypted {
                "  ·  🔒 encrypted"
            } else {
                ""
            }
        );
        let title_label = gtk::Label::new(Some(title));
        title_label.set_xalign(0.0);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.add_css_class("heading");
        let subtitle_label = gtk::Label::new(Some(&subtitle));
        subtitle_label.set_xalign(0.0);
        subtitle_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        subtitle_label.add_css_class("dim-label");
        let row_content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        row_content.set_margin_top(8);
        row_content.set_margin_bottom(8);
        row_content.set_margin_start(10);
        row_content.set_margin_end(10);
        row_content.append(&title_label);
        row_content.append(&subtitle_label);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_content));
        row.set_tooltip_text(Some(&format!(
            "{}\n{}\n{}",
            title,
            item.authors.join(", "),
            item.source_type
        )));
        list.append(&row);
    }
}

fn set_current_word(label: &gtk::Label, session: &RsvpSession, index: usize) {
    let token = session.tokens.get(index);
    let text = token
        .filter(|token| token.kind == TokenKind::Word)
        .map(|token| token.text.as_str())
        .unwrap_or("—");
    label.set_text(text);
}

fn rsvp_position(session: &RsvpSession, elapsed_ms: u64) -> (usize, bool) {
    let Some(last_index) = session.tokens.len().checked_sub(1) else {
        return (0, true);
    };
    let index = session.token_at_elapsed(elapsed_ms);
    let finished = index == last_index && elapsed_ms >= session.elapsed_at_token_end(index);
    (index, finished)
}

#[allow(clippy::too_many_arguments)]
fn tick_rsvp(
    playback: &Rc<RefCell<Option<RsvpPlayback>>>,
    core_state: &Rc<RefCell<Option<Arc<Core>>>>,
    word_label: &gtk::Label,
    status: &gtk::Label,
    play_button: &gtk::Button,
    position_scale: &gtk::Scale,
    position_label: &gtk::Label,
    position_syncing: &Rc<Cell<bool>>,
    position_scrubbing: &Rc<Cell<bool>>,
    sender: &mpsc::Sender<UiMessage>,
) {
    let mut playback_ref = playback.borrow_mut();
    let Some(playback) = playback_ref.as_mut() else {
        return;
    };
    let Some(started) = playback.playing_since else {
        return;
    };
    let elapsed = elapsed_ms(started);
    let (index, finished) = rsvp_position(&playback.session, elapsed);
    if playback.session.tokens.is_empty() {
        playback.session.pause(elapsed);
        playback.playing_since = None;
        play_button.set_label("Play");
        status.set_text("No words to read.");
        return;
    }
    sync_position_widgets(
        position_scale,
        position_label,
        index,
        playback.session.tokens.len(),
        position_syncing,
        position_scrubbing,
    );
    if finished {
        playback.session.pause(elapsed);
        playback.playing_since = None;
        play_button.set_label("Play");
        sync_position_widgets(
            position_scale,
            position_label,
            playback.session.cursor,
            playback.session.tokens.len(),
            position_syncing,
            position_scrubbing,
        );
        save_progress_async(
            core_state,
            playback.item_id.clone(),
            playback.session.cursor,
            sender.clone(),
        );
        status.set_text("RSVP complete.");
        return;
    }
    set_current_word(word_label, &playback.session, index);
    let words = playback.session.words_shown_through(index);
    status.set_text(&format!("Word {}", words.saturating_add(1)));
    if playback.last_saved.elapsed() >= Duration::from_secs(1) {
        playback.last_saved = Instant::now();
        save_progress_async(core_state, playback.item_id.clone(), index, sender.clone());
    }
}

fn change_wpm(
    playback: &Rc<RefCell<Option<RsvpPlayback>>>,
    core_state: &Rc<RefCell<Option<Arc<Core>>>>,
    target_wpm: u32,
    sender: mpsc::Sender<UiMessage>,
) -> Option<u32> {
    let mut state = playback.borrow_mut();
    let playback = state.as_mut()?;
    let current = playback.session.config.wpm;
    let target_wpm = target_wpm.clamp(100, 1000);
    if target_wpm == current {
        return Some(current);
    }
    let was_playing = playback.playing_since.take();
    let elapsed = was_playing.map(elapsed_ms).unwrap_or(0);
    playback.session.set_wpm(target_wpm, elapsed);
    if was_playing.is_some() {
        playback.playing_since = Some(Instant::now());
    }
    save_progress_async(
        core_state,
        playback.item_id.clone(),
        playback.session.cursor,
        sender,
    );
    Some(playback.session.config.wpm)
}

fn normalize_wpm(value: f64) -> u32 {
    ((value / 25.0).round() as u32 * 25).clamp(100, 1000)
}

fn position_text(index: usize, token_count: usize) -> String {
    if token_count == 0 {
        "0 / 0".to_string()
    } else {
        format!("{}/{}", index.min(token_count - 1) + 1, token_count)
    }
}

fn set_scale_value(syncing: &Cell<bool>, scale: &gtk::Scale, value: f64) {
    syncing.set(true);
    scale.set_value(value);
    syncing.set(false);
}

fn sync_position_widgets(
    scale: &gtk::Scale,
    label: &gtk::Label,
    index: usize,
    token_count: usize,
    syncing: &Cell<bool>,
    scrubbing: &Cell<bool>,
) {
    label.set_text(&position_text(index, token_count));
    if !scrubbing.get() && (scale.value() - index as f64).abs() >= 1.0 {
        set_scale_value(syncing, scale, index as f64);
    }
}

fn seek_playback(playback: &mut RsvpPlayback, target: usize, now: Instant) -> usize {
    if let Some(started) = playback.playing_since.take() {
        let elapsed = elapsed_ms_between(started, now);
        playback.session.pause(elapsed);
        playback.session.seek(target);
        playback.session.resume();
        playback.playing_since = Some(now);
    } else {
        playback.session.seek(target);
    }
    playback.last_saved = now;
    playback.session.cursor
}

fn elapsed_ms_between(started: Instant, now: Instant) -> u64 {
    u64::try_from(now.saturating_duration_since(started).as_millis()).unwrap_or(u64::MAX)
}

fn save_progress_async(
    core_state: &Rc<RefCell<Option<Arc<Core>>>>,
    item_id: String,
    token_index: usize,
    sender: mpsc::Sender<UiMessage>,
) {
    let Some(core) = core_state.borrow().as_ref().map(Arc::clone) else {
        return;
    };
    thread::spawn(move || {
        if let Err(error) = core.save_progress(&item_id, token_index) {
            let _ = sender.send(UiMessage::Failed(format!(
                "Could not save reading progress: {error}"
            )));
        }
    });
}

fn save_playback(
    playback: &Rc<RefCell<Option<RsvpPlayback>>>,
    core_state: &Rc<RefCell<Option<Arc<Core>>>>,
) {
    let mut state = playback.borrow_mut();
    let Some(playback) = state.as_mut() else {
        return;
    };
    if let Some(started) = playback.playing_since.take() {
        playback.session.pause(elapsed_ms(started));
    }
    let Some(core) = core_state.borrow().as_ref().map(Arc::clone) else {
        return;
    };
    let item_id = playback.item_id.clone();
    let token_index = playback.session.cursor;
    thread::spawn(move || {
        if let Err(error) = core.save_progress(&item_id, token_index) {
            eprintln!("Could not save RSVP progress on shutdown: {error}");
        }
    });
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn show_error(window: &gtk::ApplicationWindow, error: &str) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk::MessageType::Error)
        .buttons(gtk::ButtonsType::Close)
        .text("GIST could not complete that action")
        .secondary_text(error)
        .build();
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

fn file_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })?;
    Some(base.join("gist-linux-gtk-spike"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gist_model::Token;

    fn word(text: &str) -> Token {
        Token {
            text: text.into(),
            kind: TokenKind::Word,
            section_idx: 0,
            block_idx: 0,
            char_offset: 0,
        }
    }

    #[test]
    fn rsvp_position_uses_the_session_cursor_as_its_elapsed_anchor() {
        let mut session = RsvpSession::new(vec![word("first"), word("second")], Config::default());
        session.seek(0);
        let first = rsvp_position(&session, 0);
        let second = rsvp_position(&session, session.token_duration_ms(0));
        assert_eq!(first, (0, false));
        assert_eq!(second, (1, false));
        assert_eq!(session.cursor, 0);
    }

    #[test]
    fn rsvp_position_marks_completion_at_the_end_of_the_final_token() {
        let session = RsvpSession::new(vec![word("last")], Config::default());
        let end = session.elapsed_at_token_end(0);
        assert_eq!(rsvp_position(&session, end - 1), (0, false));
        assert_eq!(rsvp_position(&session, end), (0, true));
    }

    #[test]
    fn empty_rsvp_sessions_are_already_finished() {
        let session = RsvpSession::new(Vec::new(), Config::default());
        assert_eq!(rsvp_position(&session, 0), (0, true));
    }

    #[test]
    fn wpm_slider_snaps_to_25_wpm_steps_and_stays_in_range() {
        assert_eq!(normalize_wpm(100.0), 100);
        assert_eq!(normalize_wpm(262.0), 250);
        assert_eq!(normalize_wpm(263.0), 275);
        assert_eq!(normalize_wpm(999.0), 1000);
        assert_eq!(normalize_wpm(f64::NAN), 100);
    }

    #[test]
    fn seeking_while_paused_moves_to_a_clamped_token() {
        let session = RsvpSession::new(
            vec![word("first"), word("second"), word("third")],
            Config::default(),
        );
        let now = Instant::now();
        let mut playback = RsvpPlayback {
            item_id: "item".into(),
            session,
            playing_since: None,
            last_saved: now,
        };

        let position = seek_playback(&mut playback, usize::MAX, now + Duration::from_secs(1));

        assert_eq!(position, 2);
        assert_eq!(playback.session.cursor, 2);
        assert_eq!(playback.session.state, gist_rsvp::PlayState::Paused);
        assert_eq!(playback.playing_since, None);
    }

    #[test]
    fn seeking_during_playback_reanchors_the_live_session() {
        let mut session = RsvpSession::new(
            vec![word("first"), word("second"), word("third")],
            Config::default(),
        );
        session.resume();
        let started = Instant::now();
        let mut playback = RsvpPlayback {
            item_id: "item".into(),
            session,
            playing_since: Some(started),
            last_saved: started,
        };
        let seeked_at = started + Duration::from_secs(1);

        let position = seek_playback(&mut playback, 1, seeked_at);

        assert_eq!(position, 1);
        assert_eq!(playback.session.cursor, 1);
        assert_eq!(playback.session.state, gist_rsvp::PlayState::Playing);
        assert_eq!(playback.session.elapsed_at_pause, 1000);
        assert_eq!(playback.playing_since, Some(seeked_at));
        assert_eq!(playback.session.token_at_elapsed(0), 1);
    }

    #[test]
    fn position_label_shows_one_based_token_progress() {
        assert_eq!(position_text(0, 0), "0 / 0");
        assert_eq!(position_text(0, 12), "1/12");
        assert_eq!(position_text(usize::MAX, 12), "12/12");
    }
}
