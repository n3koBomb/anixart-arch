use crate::{
    api::ApiClient,
    config::{config_path, AppConfig},
    model::{
        extract_episode_items, extract_episode_sources, extract_release_cards,
        extract_release_detail, extract_voiceovers, EpisodeItem, EpisodeSource, ReleaseCard,
        Voiceover,
    },
    player::{
        media_check_report, playback_route, EmbeddedWebBackend, MpvBackend,
        PlaybackRoute,
    },
    APP_NAME, APP_VERSION,
};
use adw::prelude::*;
use gtk::glib::object::IsA;
use gtk::{glib, Orientation};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

const IMAGE_LIMIT: u64 = 12 * 1024 * 1024;

#[derive(Debug)]
enum UiMessage {
    Search(Result<(Vec<ReleaseCard>, Value), String>),
    Discover(Result<(Vec<ReleaseCard>, Value), String>),
    Details(i64, ReleaseCard, Result<Value, String>),
    Voiceovers(i64, Result<(Vec<Voiceover>, Value), String>),
    Sources(i64, i64, Result<(Vec<EpisodeSource>, Value), String>),
    EpisodeList(i64, i64, i64, Result<(Vec<EpisodeItem>, Value), String>),
    ApiCheck(Result<Value, String>),
    ImageLoaded(String, Result<Vec<u8>, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceoverFilter {
    All,
    Voiceovers,
    Subtitles,
}

#[derive(Clone)]
struct DetailUi {
    poster: gtk::Picture,
    title: gtk::Label,
    subtitle: gtk::Label,
    chips: gtk::Label,
    description: gtk::Label,
    facts: gtk::Label,
}

#[derive(Clone)]
struct ChooserUi {
    voiceover_list: gtk::ListBox,
    source_list: gtk::ListBox,
    episode_list: gtk::ListBox,
    episode_info: gtk::Label,
    play_button: gtk::Button,
    filter: Rc<Cell<VoiceoverFilter>>,
}

type ImageWaiters = Rc<RefCell<HashMap<String, Vec<gtk::Picture>>>>;
type ImageCache = Rc<RefCell<HashMap<String, gtk::gdk::Texture>>>;

pub fn build_ui(app: &adw::Application) {
    let (tx, rx) = mpsc::channel::<UiMessage>();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APP_NAME)
        .default_width(1420)
        .default_height(900)
        .build();

    let cards = Rc::new(RefCell::new(Vec::<ReleaseCard>::new()));
    let latest_raw = Rc::new(RefCell::new(None::<Value>));

    let detail_ui = Rc::new(RefCell::new(None::<DetailUi>));
    let detail_card = Rc::new(RefCell::new(None::<ReleaseCard>));
    let current_detail_release = Rc::new(Cell::new(None::<i64>));

    let chooser_ui = Rc::new(RefCell::new(None::<ChooserUi>));
    let voiceovers = Rc::new(RefCell::new(Vec::<Voiceover>::new()));
    let visible_voiceovers = Rc::new(RefCell::new(Vec::<Voiceover>::new()));
    let sources = Rc::new(RefCell::new(Vec::<EpisodeSource>::new()));
    let episodes = Rc::new(RefCell::new(Vec::<EpisodeItem>::new()));
    let selected_episode = Rc::new(RefCell::new(None::<EpisodeItem>));
    let current_release = Rc::new(Cell::new(None::<i64>));
    let current_voiceover = Rc::new(Cell::new(None::<i64>));
    let current_source = Rc::new(Cell::new(None::<i64>));

    let image_waiters: ImageWaiters = Rc::new(RefCell::new(HashMap::new()));
    let image_cache: ImageCache = Rc::new(RefCell::new(HashMap::new()));

    let root = gtk::Box::new(Orientation::Vertical, 0);

    // Phone-inspired desktop header: search is the primary control, with one
    // icon-only settings/tools button on the right.
    let header = gtk::HeaderBar::new();
    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search anime…")
        .width_request(620)
        .build();
    header.pack_start(&search_entry);

    let menu_button = gtk::MenuButton::builder()
        .icon_name("preferences-system-symbolic")
        .tooltip_text("Settings and developer tools")
        .build();
    header.pack_end(&menu_button);
    root.append(&header);

    // Secondary content navigation. Filter is intentionally UI-only in 0.4.x.
    let category_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .build();
    let categories = gtk::Box::new(Orientation::Horizontal, 8);
    set_margins(&categories, 10);
    let filter_button = icon_label_button("Filter", "view-filter-symbolic");
    filter_button.set_tooltip_text(Some("Filter controls are reserved for a later release."));
    let my_tab = gtk::ToggleButton::with_label("My tab");
    let anime_tab = gtk::ToggleButton::with_label("Anime");
    let donghua_tab = gtk::ToggleButton::with_label("Donghua");
    let latest_tab = gtk::ToggleButton::with_label("Latest");
    anime_tab.set_active(true);
    my_tab.set_group(Some(&anime_tab));
    donghua_tab.set_group(Some(&anime_tab));
    latest_tab.set_group(Some(&anime_tab));
    for button in [&my_tab, &anime_tab, &donghua_tab, &latest_tab] {
        button.add_css_class("pill");
    }
    filter_button.add_css_class("pill");
    categories.append(&filter_button);
    categories.append(&my_tab);
    categories.append(&anime_tab);
    categories.append(&donghua_tab);
    categories.append(&latest_tab);
    category_scroll.set_child(Some(&categories));
    root.append(&category_scroll);

    // Responsive poster grid, replacing the old text/debug split view.
    let release_grid = gtk::FlowBox::new();
    release_grid.set_selection_mode(gtk::SelectionMode::None);
    release_grid.set_activate_on_single_click(true);
    release_grid.set_homogeneous(false);
    release_grid.set_column_spacing(16);
    release_grid.set_row_spacing(20);
    release_grid.set_min_children_per_line(4);
    release_grid.set_max_children_per_line(6);
    release_grid.set_valign(gtk::Align::Start);
    set_margins(&release_grid, 16);

    let content_scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&release_grid)
        .build();
    root.append(&content_scroll);

    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_selectable(true);
    status.add_css_class("dim-label");
    set_margins(&status, 8);
    refresh_status(&status);

    // Fixed bottom navigation, intentionally matching the Android information
    // architecture. Home is functional in 0.4.x; the remaining destinations
    // are visible structure for upcoming versions.
    let bottom = gtk::Box::new(Orientation::Horizontal, 6);
    bottom.set_halign(gtk::Align::Center);
    bottom.add_css_class("toolbar");
    set_margins(&bottom, 8);
    let home_nav = nav_button("Home", "go-home-symbolic", true);
    let browse_nav = nav_button("Browse", "find-location-symbolic", false);
    let bookmarks_nav = nav_button("Bookmarks", "bookmark-new-symbolic", false);
    let feed_nav = nav_button("Feed", "view-list-symbolic", false);
    let profile_nav = nav_button("Profile", "avatar-default-symbolic", false);
    bottom.append(&home_nav);
    bottom.append(&browse_nav);
    bottom.append(&bookmarks_nav);
    bottom.append(&feed_nav);
    bottom.append(&profile_nav);

    root.append(&status);
    root.append(&gtk::Separator::new(Orientation::Horizontal));
    root.append(&bottom);

    // One application window, multiple purpose-specific pages. Details and
    // playback selection replace the home content instead of opening extra
    // utility windows. The actual video renderer remains a dedicated player
    // window for now.
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.add_named(&root, Some("home"));
    stack.set_visible_child_name("home");
    window.set_content(Some(&stack));

    install_message_pump(
        rx,
        tx.clone(),
        &release_grid,
        &status,
        &cards,
        &latest_raw,
        &detail_ui,
        &detail_card,
        &current_detail_release,
        &chooser_ui,
        &voiceovers,
        &visible_voiceovers,
        &sources,
        &episodes,
        &selected_episode,
        &current_release,
        &current_voiceover,
        &current_source,
        &image_waiters,
        &image_cache,
    );

    // Search.
    {
        let tx = tx.clone();
        let entry = search_entry.clone();
        let status = status.clone();
        search_entry.connect_activate(move |_| {
            let query = entry.text().trim().to_owned();
            if query.is_empty() {
                status.set_text("Enter a search query first.");
                return;
            }
            status.set_text("Searching…");
            let tx = tx.clone();
            run_api(move |api| {
                let result = api
                    .search_releases(&query, 0)
                    .map(|json| (extract_release_cards(&json), json))
                    .map_err(|err| err.to_string());
                let _ = tx.send(UiMessage::Search(result));
            });
        });
    }

    // Clicking a poster opens a dedicated desktop detail window. Playback is
    // deliberately not embedded into this page; the Play CTA opens its own
    // voiceover/source/episode chooser, matching the requested Android flow.
    {
        let parent = window.clone();
        let stack = stack.clone();
        let cards = cards.clone();
        let tx = tx.clone();
        let detail_ui = detail_ui.clone();
        let detail_card = detail_card.clone();
        let current_detail_release = current_detail_release.clone();
        let chooser_ui = chooser_ui.clone();
        let voiceovers = voiceovers.clone();
        let visible_voiceovers = visible_voiceovers.clone();
        let sources = sources.clone();
        let episodes = episodes.clone();
        let selected_episode = selected_episode.clone();
        let current_release = current_release.clone();
        let current_voiceover = current_voiceover.clone();
        let current_source = current_source.clone();
        let status = status.clone();
        let image_waiters = image_waiters.clone();
        let image_cache = image_cache.clone();

        release_grid.connect_child_activated(move |_, child| {
            let Some(card) = cards.borrow().get(child.index() as usize).cloned() else {
                return;
            };
            open_detail_page(
                &parent,
                &stack,
                card,
                &tx,
                &detail_ui,
                &detail_card,
                &current_detail_release,
                &chooser_ui,
                &voiceovers,
                &visible_voiceovers,
                &sources,
                &episodes,
                &selected_episode,
                &current_release,
                &current_voiceover,
                &current_source,
                &status,
                &image_waiters,
                &image_cache,
            );
        });
    }

    // Settings + separate Developer Tools section inside the icon popover.
    build_tools_popover(
        &menu_button,
        &window,
        &status,
        tx.clone(),
        latest_raw.clone(),
    );

    // Non-home navigation is deliberately structural only for the 0.4.x UI
    // milestone. It exists now so future feature work won't require another
    // shell redesign.
    for (button, label) in [
        (browse_nav, "Browse"),
        (bookmarks_nav, "Bookmarks"),
        (feed_nav, "Feed"),
        (profile_nav, "Profile"),
    ] {
        let status = status.clone();
        button.connect_clicked(move |_| {
            status.set_text(&format!("{label} is part of the new navigation shell and will be wired in a later version."));
        });
    }

    for (tab, label) in [
        (my_tab, "My tab"),
        (donghua_tab, "Donghua"),
        (latest_tab, "Latest"),
    ] {
        let status = status.clone();
        tab.connect_toggled(move |button| {
            if button.is_active() {
                status.set_text(&format!("{label} tab is reserved in 0.4.x; the current grid remains loaded."));
            }
        });
    }

    // Initial home/discover content.
    {
        let tx = tx.clone();
        let status = status.clone();
        status.set_text("Loading home content…");
        run_api(move |api| {
            let result = api
                .discover_interesting()
                .map(|json| (extract_release_cards(&json), json))
                .map_err(|err| err.to_string());
            let _ = tx.send(UiMessage::Discover(result));
        });
    }

    window.present();
}

#[allow(clippy::too_many_arguments)]
fn install_message_pump(
    rx: Receiver<UiMessage>,
    tx: Sender<UiMessage>,
    release_grid: &gtk::FlowBox,
    status: &gtk::Label,
    cards: &Rc<RefCell<Vec<ReleaseCard>>>,
    latest_raw: &Rc<RefCell<Option<Value>>>,
    detail_ui: &Rc<RefCell<Option<DetailUi>>>,
    detail_card: &Rc<RefCell<Option<ReleaseCard>>>,
    current_detail_release: &Rc<Cell<Option<i64>>>,
    chooser_ui: &Rc<RefCell<Option<ChooserUi>>>,
    voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    visible_voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    sources: &Rc<RefCell<Vec<EpisodeSource>>>,
    episodes: &Rc<RefCell<Vec<EpisodeItem>>>,
    selected_episode: &Rc<RefCell<Option<EpisodeItem>>>,
    current_release: &Rc<Cell<Option<i64>>>,
    current_voiceover: &Rc<Cell<Option<i64>>>,
    current_source: &Rc<Cell<Option<i64>>>,
    image_waiters: &ImageWaiters,
    image_cache: &ImageCache,
) {
    let release_grid = release_grid.clone();
    let status = status.clone();
    let cards = cards.clone();
    let latest_raw = latest_raw.clone();
    let detail_ui = detail_ui.clone();
    let detail_card = detail_card.clone();
    let current_detail_release = current_detail_release.clone();
    let chooser_ui = chooser_ui.clone();
    let voiceovers = voiceovers.clone();
    let visible_voiceovers = visible_voiceovers.clone();
    let sources = sources.clone();
    let episodes = episodes.clone();
    let selected_episode = selected_episode.clone();
    let current_release = current_release.clone();
    let current_voiceover = current_voiceover.clone();
    let current_source = current_source.clone();
    let image_waiters = image_waiters.clone();
    let image_cache = image_cache.clone();

    glib::timeout_add_local(Duration::from_millis(60), move || {
        while let Ok(message) = rx.try_recv() {
            match message {
                UiMessage::Search(result) => match result {
                    Ok((items, raw)) => {
                        *latest_raw.borrow_mut() = Some(raw);
                        populate_release_grid(
                            &release_grid,
                            &cards,
                            items,
                            &tx,
                            &image_waiters,
                            &image_cache,
                        );
                        status.set_text(&format!(
                            "Search complete: {} releases found.",
                            cards.borrow().len()
                        ));
                    }
                    Err(err) => status.set_text(&format!("Search failed: {err}")),
                },
                UiMessage::Discover(result) => match result {
                    Ok((items, raw)) => {
                        *latest_raw.borrow_mut() = Some(raw);
                        populate_release_grid(
                            &release_grid,
                            &cards,
                            items,
                            &tx,
                            &image_waiters,
                            &image_cache,
                        );
                        status.set_text(&format!(
                            "Home loaded: {} releases found.",
                            cards.borrow().len()
                        ));
                    }
                    Err(err) => status.set_text(&format!("Home request failed: {err}")),
                },
                UiMessage::Details(id, fallback, result) => {
                    if current_detail_release.get() != Some(id) {
                        continue;
                    }
                    match result {
                        Ok(raw) => {
                            *latest_raw.borrow_mut() = Some(raw.clone());
                            let merged = extract_release_detail(&raw, id)
                                .map(|newer| fallback.clone().merge_prefer(newer))
                                .unwrap_or(fallback);
                            *detail_card.borrow_mut() = Some(merged.clone());
                            if let Some(ui) = detail_ui.borrow().as_ref() {
                                update_detail_ui(ui, &merged);
                                bind_remote_image(
                                    merged.poster_url.as_deref(),
                                    &ui.poster,
                                    &tx,
                                    &image_waiters,
                                    &image_cache,
                                );
                            }
                            status.set_text(&format!("Loaded release details for {}.", merged.title));
                        }
                        Err(err) => {
                            if let Some(ui) = detail_ui.borrow().as_ref() {
                                ui.facts.set_text(&format!(
                                    "Could not load extended release metadata: {err}

The card data is still usable."
                                ));
                            }
                            status.set_text(&format!("Release request failed: {err}"));
                        },
                    }
                }
                UiMessage::Voiceovers(id, result) => {
                    if current_release.get() != Some(id) {
                        continue;
                    }
                    match result {
                        Ok((items, raw)) => {
                            *latest_raw.borrow_mut() = Some(raw);
                            *voiceovers.borrow_mut() = items;
                            let count = voiceovers.borrow().len();
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                populate_voiceover_filter(
                                    &ui.voiceover_list,
                                    &voiceovers,
                                    &visible_voiceovers,
                                    ui.filter.get(),
                                    &tx,
                                    &image_waiters,
                                    &image_cache,
                                );
                                if count == 0 {
                                    ui.episode_info.set_text(
                                        "No voiceovers were returned for this release. If this came from Home/Discover, verify that the card carries the real release ID.",
                                    );
                                } else {
                                    ui.episode_info.set_text("Select a voiceover or subtitle track.");
                                }
                            }
                            status.set_text(&format!(
                                "Loaded {count} voiceover/subtitle options."
                            ));
                        }
                        Err(err) => {
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                ui.episode_info.set_text(&format!("Voiceover request failed: {err}"));
                            }
                            status.set_text(&format!("Voiceover request failed: {err}"));
                        },
                    }
                }
                UiMessage::Sources(release_id, voiceover_id, result) => {
                    if current_release.get() != Some(release_id)
                        || current_voiceover.get() != Some(voiceover_id)
                    {
                        continue;
                    }
                    match result {
                        Ok((items, raw)) => {
                            *latest_raw.borrow_mut() = Some(raw);
                            *sources.borrow_mut() = items;
                            let count = sources.borrow().len();
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                populate_sources(&ui.source_list, &sources);
                                if count == 0 {
                                    ui.episode_info.set_text("This voiceover returned no player sources.");
                                } else if count == 1 {
                                    if let Some(row) = ui.source_list.row_at_index(0) {
                                        ui.source_list.select_row(Some(&row));
                                    }
                                } else {
                                    ui.episode_info.set_text("Select a player source.");
                                }
                            }
                            status.set_text(&format!("Loaded {count} player sources."));
                        }
                        Err(err) => {
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                ui.episode_info.set_text(&format!("Source request failed: {err}"));
                            }
                            status.set_text(&format!("Source request failed: {err}"));
                        },
                    }
                }
                UiMessage::EpisodeList(release_id, voiceover_id, source_id, result) => {
                    if current_release.get() != Some(release_id)
                        || current_voiceover.get() != Some(voiceover_id)
                        || current_source.get() != Some(source_id)
                    {
                        continue;
                    }
                    match result {
                        Ok((items, raw)) => {
                            *latest_raw.borrow_mut() = Some(raw);
                            *episodes.borrow_mut() = items;
                            *selected_episode.borrow_mut() = None;
                            let count = episodes.borrow().len();
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                populate_episodes(&ui.episode_list, &episodes);
                                ui.play_button.set_sensitive(false);
                                ui.play_button.set_label("Play episode");
                                if count == 0 {
                                    ui.episode_info.set_text("This player source returned no episodes.");
                                } else {
                                    ui.episode_info.set_text("Select an episode.");
                                }
                            }
                            status.set_text(&format!("Loaded {count} episodes."));
                        }
                        Err(err) => {
                            if let Some(ui) = chooser_ui.borrow().as_ref() {
                                ui.episode_info.set_text(&format!("Episode request failed: {err}"));
                            }
                            status.set_text(&format!("Episode request failed: {err}"));
                        },
                    }
                }
                UiMessage::ApiCheck(result) => match result {
                    Ok(raw) => {
                        *latest_raw.borrow_mut() = Some(raw);
                        status.set_text("API configuration endpoint responded successfully.");
                    }
                    Err(err) => status.set_text(&format!("API check failed: {err}")),
                },
                UiMessage::ImageLoaded(url, result) => {
                    let waiters = image_waiters.borrow_mut().remove(&url).unwrap_or_default();
                    match result {
                        Ok(data) => {
                            let bytes = glib::Bytes::from_owned(data);
                            match gtk::gdk::Texture::from_bytes(&bytes) {
                                Ok(texture) => {
                                    image_cache.borrow_mut().insert(url, texture.clone());
                                    for picture in waiters {
                                        picture.set_paintable(Some(&texture));
                                    }
                                }
                                Err(err) => log::debug!("could not decode remote image: {err}"),
                            }
                        }
                        Err(err) => log::debug!("remote image request failed: {err}"),
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

fn build_tools_popover(
    menu_button: &gtk::MenuButton,
    parent: &adw::ApplicationWindow,
    status: &gtk::Label,
    tx: Sender<UiMessage>,
    latest_raw: Rc<RefCell<Option<Value>>>,
) {
    let popover = gtk::Popover::new();
    let box_ = gtk::Box::new(Orientation::Vertical, 6);
    set_margins(&box_, 10);

    let settings = icon_label_button("Settings", "preferences-system-symbolic");
    settings.set_halign(gtk::Align::Fill);
    box_.append(&settings);
    box_.append(&gtk::Separator::new(Orientation::Horizontal));

    let dev_label = gtk::Label::new(Some("Developer tools"));
    dev_label.set_xalign(0.0);
    dev_label.add_css_class("heading");
    box_.append(&dev_label);

    let api = icon_label_button("API check", "network-server-symbolic");
    let play_url = icon_label_button("Play URL", "media-playback-start-symbolic");
    let raw = icon_label_button("Raw API inspector", "text-x-generic-symbolic");
    let media = icon_label_button("Media diagnostics", "utilities-system-monitor-symbolic");
    for button in [&api, &play_url, &raw, &media] {
        button.set_halign(gtk::Align::Fill);
        box_.append(button);
    }

    popover.set_child(Some(&box_));
    menu_button.set_popover(Some(&popover));

    {
        let parent = parent.clone();
        let status = status.clone();
        let popover = popover.clone();
        settings.connect_clicked(move |_| {
            popover.popdown();
            show_settings(&parent, &status);
        });
    }

    {
        let status = status.clone();
        let popover = popover.clone();
        let tx_outer = tx.clone();
        api.connect_clicked(move |_| {
            popover.popdown();
            status.set_text("Checking API configuration endpoint…");
            let tx = tx_outer.clone();
            run_api(move |api| {
                let result = api.config_toggles().map_err(|err| err.to_string());
                let _ = tx.send(UiMessage::ApiCheck(result));
            });
        });
    }

    {
        let parent = parent.clone();
        let status = status.clone();
        let popover = popover.clone();
        play_url.connect_clicked(move |_| {
            popover.popdown();
            show_play_url(&parent, &status);
        });
    }

    {
        let parent = parent.clone();
        let raw_value = latest_raw.clone();
        let popover = popover.clone();
        raw.connect_clicked(move |_| {
            popover.popdown();
            show_raw_api(&parent, raw_value.borrow().as_ref());
        });
    }

    {
        let parent = parent.clone();
        let popover = popover.clone();
        media.connect_clicked(move |_| {
            popover.popdown();
            show_text_window(&parent, "Media diagnostics", &media_check_report(), 820, 680);
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn open_detail_page(
    parent: &adw::ApplicationWindow,
    stack: &gtk::Stack,
    card: ReleaseCard,
    tx: &Sender<UiMessage>,
    detail_ui: &Rc<RefCell<Option<DetailUi>>>,
    detail_card: &Rc<RefCell<Option<ReleaseCard>>>,
    current_detail_release: &Rc<Cell<Option<i64>>>,
    chooser_ui: &Rc<RefCell<Option<ChooserUi>>>,
    voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    visible_voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    sources: &Rc<RefCell<Vec<EpisodeSource>>>,
    episodes: &Rc<RefCell<Vec<EpisodeItem>>>,
    selected_episode: &Rc<RefCell<Option<EpisodeItem>>>,
    current_release: &Rc<Cell<Option<i64>>>,
    current_voiceover: &Rc<Cell<Option<i64>>>,
    current_source: &Rc<Cell<Option<i64>>>,
    status: &gtk::Label,
    image_waiters: &ImageWaiters,
    image_cache: &ImageCache,
) {
    current_detail_release.set(Some(card.id));
    *detail_card.borrow_mut() = Some(card.clone());

    if let Some(old) = stack.child_by_name("detail") {
        stack.remove(&old);
    }

    let page = gtk::Box::new(Orientation::Vertical, 0);
    let header = gtk::HeaderBar::new();
    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text("Back to home")
        .build();
    header.pack_start(&back);
    let header_title = gtk::Label::new(Some("Release details"));
    header_title.add_css_class("heading");
    header.set_title_widget(Some(&header_title));
    page.append(&header);

    // Paned keeps the artwork column bounded even when the source image has a
    // large intrinsic resolution. The old GtkBox layout allowed wide artwork
    // to consume most of the window.
    let content = gtk::Paned::new(Orientation::Horizontal);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_position(350);
    content.set_resize_start_child(false);
    content.set_shrink_start_child(true);
    content.set_resize_end_child(true);
    content.set_shrink_end_child(false);

    let left = gtk::Box::new(Orientation::Vertical, 12);
    left.set_size_request(330, -1);
    left.set_hexpand(false);
    left.set_valign(gtk::Align::Start);
    set_margins(&left, 22);

    let poster = gtk::Picture::new();
    poster.set_size_request(300, 430);
    poster.set_can_shrink(true);
    poster.set_content_fit(gtk::ContentFit::Cover);
    poster.set_halign(gtk::Align::Center);
    poster.set_valign(gtk::Align::Start);
    poster.add_css_class("card");
    left.append(&poster);

    let id_label = gtk::Label::new(Some(&format!("Release ID {}", card.id)));
    id_label.set_xalign(0.0);
    id_label.add_css_class("dim-label");
    left.append(&id_label);
    content.set_start_child(Some(&left));

    let right_outer = gtk::Box::new(Orientation::Vertical, 14);
    right_outer.set_hexpand(true);
    right_outer.set_valign(gtk::Align::Start);
    set_margins(&right_outer, 24);

    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("title-1");
    right_outer.append(&title);

    let subtitle = gtk::Label::new(None);
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class("dim-label");
    right_outer.append(&subtitle);

    let chips = gtk::Label::new(None);
    chips.set_xalign(0.0);
    chips.set_wrap(true);
    right_outer.append(&chips);

    let action_row = gtk::Box::new(Orientation::Horizontal, 10);
    let play = icon_label_button("Play", "media-playback-start-symbolic");
    play.add_css_class("suggested-action");
    play.add_css_class("pill");
    let bookmark = icon_label_button("Bookmark", "bookmark-new-symbolic");
    bookmark.add_css_class("pill");
    bookmark.set_tooltip_text(Some("Bookmark integration is planned for a later release."));
    action_row.append(&play);
    action_row.append(&bookmark);
    right_outer.append(&action_row);

    let description_heading = gtk::Label::new(Some("Description"));
    description_heading.set_xalign(0.0);
    description_heading.add_css_class("title-3");
    right_outer.append(&description_heading);

    let description = gtk::Label::new(None);
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.set_selectable(true);
    right_outer.append(&description);

    let facts_heading = gtk::Label::new(Some("Details"));
    facts_heading.set_xalign(0.0);
    facts_heading.add_css_class("title-3");
    right_outer.append(&facts_heading);

    let facts = gtk::Label::new(None);
    facts.set_xalign(0.0);
    facts.set_wrap(true);
    facts.set_selectable(true);
    right_outer.append(&facts);

    let right_scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&right_outer)
        .build();
    content.set_end_child(Some(&right_scroll));
    page.append(&content);

    let ui = DetailUi {
        poster: poster.clone(),
        title,
        subtitle,
        chips,
        description,
        facts,
    };
    update_detail_ui(&ui, &card);
    bind_remote_image(
        card.poster_url.as_deref(),
        &poster,
        tx,
        image_waiters,
        image_cache,
    );
    *detail_ui.borrow_mut() = Some(ui);

    stack.add_named(&page, Some("detail"));
    stack.set_visible_child_name("detail");

    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("home"));
    }

    {
        let parent = parent.clone();
        let stack = stack.clone();
        let detail_card = detail_card.clone();
        let tx = tx.clone();
        let chooser_ui = chooser_ui.clone();
        let voiceovers = voiceovers.clone();
        let visible_voiceovers = visible_voiceovers.clone();
        let sources = sources.clone();
        let episodes = episodes.clone();
        let selected_episode = selected_episode.clone();
        let current_release = current_release.clone();
        let current_voiceover = current_voiceover.clone();
        let current_source = current_source.clone();
        let status = status.clone();
        let image_waiters = image_waiters.clone();
        let image_cache = image_cache.clone();
        play.connect_clicked(move |_| {
            let Some(card) = detail_card.borrow().clone() else {
                status.set_text("Release details are not available yet.");
                return;
            };
            open_playback_chooser(
                &parent,
                &stack,
                card,
                &tx,
                &chooser_ui,
                &voiceovers,
                &visible_voiceovers,
                &sources,
                &episodes,
                &selected_episode,
                &current_release,
                &current_voiceover,
                &current_source,
                &status,
                &image_waiters,
                &image_cache,
            );
        });
    }

    // The page is usable immediately from search/discover metadata; a richer
    // release object is merged in when /release/{id} finishes.
    {
        let tx = tx.clone();
        let fallback = card.clone();
        run_api(move |api| {
            let result = api.release(fallback.id).map_err(|err| err.to_string());
            let _ = tx.send(UiMessage::Details(fallback.id, fallback, result));
        });
    }
}

fn update_detail_ui(ui: &DetailUi, card: &ReleaseCard) {
    ui.title.set_text(&card.title);
    ui.subtitle
        .set_text(card.subtitle.as_deref().unwrap_or(""));

    let mut chips = Vec::new();
    if let Some(year) = card.year {
        chips.push(year.to_string());
    }
    if let Some(age) = &card.age_rating {
        chips.push(age.clone());
    }
    if let Some(status) = &card.status {
        chips.push(status.clone());
    }
    if let Some(rating) = card.rating {
        chips.push(format!("★ {rating:.1}"));
    }
    ui.chips.set_text(&chips.join("   •   "));

    ui.description.set_text(
        card.description
            .as_deref()
            .unwrap_or("No description returned by the current API response."),
    );

    let mut facts = Vec::new();
    if let Some(country) = &card.country {
        facts.push(format!("Country: {country}"));
    }
    if let Some(season) = &card.season {
        facts.push(format!("Season: {season}"));
    }
    if let Some(studio) = &card.studio {
        facts.push(format!("Studio: {studio}"));
    }
    match (card.episodes_released, card.episodes_total) {
        (Some(current), Some(total)) => facts.push(format!("Episodes: {current} / {total}")),
        (Some(current), None) => facts.push(format!("Episodes released: {current}")),
        (None, Some(total)) => facts.push(format!("Episodes total: {total}")),
        _ => {}
    }
    if let Some(duration) = card.duration_minutes {
        facts.push(format!("Duration: ~{duration} min."));
    }
    if !card.genres.is_empty() {
        facts.push(format!("Genres: {}", card.genres.join(", ")));
    }
    if facts.is_empty() {
        facts.push("Additional release metadata is not present in this API response.".to_string());
    }
    ui.facts.set_text(&facts.join("\n"));
}

#[allow(clippy::too_many_arguments)]
fn open_playback_chooser(
    parent: &adw::ApplicationWindow,
    stack: &gtk::Stack,
    card: ReleaseCard,
    tx: &Sender<UiMessage>,
    chooser_ui: &Rc<RefCell<Option<ChooserUi>>>,
    voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    visible_voiceovers: &Rc<RefCell<Vec<Voiceover>>>,
    sources: &Rc<RefCell<Vec<EpisodeSource>>>,
    episodes: &Rc<RefCell<Vec<EpisodeItem>>>,
    selected_episode: &Rc<RefCell<Option<EpisodeItem>>>,
    current_release: &Rc<Cell<Option<i64>>>,
    current_voiceover: &Rc<Cell<Option<i64>>>,
    current_source: &Rc<Cell<Option<i64>>>,
    status: &gtk::Label,
    image_waiters: &ImageWaiters,
    image_cache: &ImageCache,
) {
    if let Some(old) = stack.child_by_name("chooser") {
        stack.remove(&old);
    }

    current_release.set(Some(card.id));
    current_voiceover.set(None);
    current_source.set(None);
    voiceovers.borrow_mut().clear();
    visible_voiceovers.borrow_mut().clear();
    sources.borrow_mut().clear();
    episodes.borrow_mut().clear();
    *selected_episode.borrow_mut() = None;

    let page = gtk::Box::new(Orientation::Vertical, 0);
    let header = gtk::HeaderBar::new();
    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text("Back to release details")
        .build();
    header.pack_start(&back);
    let title = gtk::Label::new(Some("Choose voiceover and episode"));
    title.add_css_class("heading");
    header.set_title_widget(Some(&title));
    page.append(&header);

    let filter_row = gtk::Box::new(Orientation::Horizontal, 8);
    set_margins(&filter_row, 14);
    let all = gtk::ToggleButton::with_label("All");
    let dubs = gtk::ToggleButton::with_label("Voiceovers");
    let subs = gtk::ToggleButton::with_label("Subtitles");
    all.set_active(true);
    dubs.set_group(Some(&all));
    subs.set_group(Some(&all));
    for button in [&all, &dubs, &subs] {
        button.add_css_class("pill");
        filter_row.append(button);
    }
    page.append(&filter_row);

    let body = gtk::Box::new(Orientation::Horizontal, 12);
    body.set_vexpand(true);
    set_margins(&body, 12);

    let voiceover_list = gtk::ListBox::new();
    voiceover_list.set_selection_mode(gtk::SelectionMode::Single);
    let source_list = gtk::ListBox::new();
    source_list.set_selection_mode(gtk::SelectionMode::Single);
    let episode_list = gtk::ListBox::new();
    episode_list.set_selection_mode(gtk::SelectionMode::Single);

    body.append(&chooser_column("Voiceover / subtitles", &voiceover_list, 390));
    body.append(&chooser_column("Player source", &source_list, 250));
    body.append(&chooser_column("Episodes", &episode_list, 330));
    page.append(&body);

    let footer = gtk::Box::new(Orientation::Vertical, 8);
    set_margins(&footer, 12);
    let episode_info = gtk::Label::new(Some("Loading voiceovers…"));
    episode_info.set_xalign(0.0);
    episode_info.set_wrap(true);
    episode_info.add_css_class("dim-label");
    footer.append(&episode_info);
    let play_button = icon_label_button("Play episode", "media-playback-start-symbolic");
    play_button.add_css_class("suggested-action");
    play_button.add_css_class("pill");
    play_button.set_sensitive(false);
    play_button.set_halign(gtk::Align::Start);
    footer.append(&play_button);
    page.append(&gtk::Separator::new(Orientation::Horizontal));
    page.append(&footer);
    let filter = Rc::new(Cell::new(VoiceoverFilter::All));
    *chooser_ui.borrow_mut() = Some(ChooserUi {
        voiceover_list: voiceover_list.clone(),
        source_list: source_list.clone(),
        episode_list: episode_list.clone(),
        episode_info: episode_info.clone(),
        play_button: play_button.clone(),
        filter: filter.clone(),
    });

    stack.add_named(&page, Some("chooser"));
    stack.set_visible_child_name("chooser");
    {
        let stack = stack.clone();
        back.connect_clicked(move |_| stack.set_visible_child_name("detail"));
    }

    // Voiceover type filter chips.
    for (button, mode) in [
        (all, VoiceoverFilter::All),
        (dubs, VoiceoverFilter::Voiceovers),
        (subs, VoiceoverFilter::Subtitles),
    ] {
        let all_voiceovers = voiceovers.clone();
        let visible = visible_voiceovers.clone();
        let list = voiceover_list.clone();
        let source_list_for_filter = source_list.clone();
        let episode_list_for_filter = episode_list.clone();
        let sources_for_filter = sources.clone();
        let episodes_for_filter = episodes.clone();
        let selected_for_filter = selected_episode.clone();
        let current_voiceover_for_filter = current_voiceover.clone();
        let current_source_for_filter = current_source.clone();
        let play_for_filter = play_button.clone();
        let info_for_filter = episode_info.clone();
        let filter_state = filter.clone();
        let tx = tx.clone();
        let image_waiters = image_waiters.clone();
        let image_cache = image_cache.clone();
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            filter_state.set(mode);
            current_voiceover_for_filter.set(None);
            current_source_for_filter.set(None);
            sources_for_filter.borrow_mut().clear();
            episodes_for_filter.borrow_mut().clear();
            *selected_for_filter.borrow_mut() = None;
            clear_list(&source_list_for_filter);
            clear_list(&episode_list_for_filter);
            play_for_filter.set_sensitive(false);
            play_for_filter.set_label("Play episode");
            info_for_filter.set_text("Select a voiceover.");
            populate_voiceover_filter(
                &list,
                &all_voiceovers,
                &visible,
                mode,
                &tx,
                &image_waiters,
                &image_cache,
            );
        });
    }

    // Voiceover -> source.
    {
        let tx = tx.clone();
        let visible_voiceovers = visible_voiceovers.clone();
        let sources = sources.clone();
        let episodes = episodes.clone();
        let selected_episode = selected_episode.clone();
        let current_release = current_release.clone();
        let current_voiceover = current_voiceover.clone();
        let current_source = current_source.clone();
        let source_list = source_list.clone();
        let episode_list = episode_list.clone();
        let episode_info = episode_info.clone();
        let play_button = play_button.clone();
        let status = status.clone();

        voiceover_list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let Some(release_id) = current_release.get() else { return };
            let Some(voiceover) = visible_voiceovers.borrow().get(row.index() as usize).cloned()
            else {
                return;
            };

            current_voiceover.set(Some(voiceover.id));
            current_source.set(None);
            sources.borrow_mut().clear();
            episodes.borrow_mut().clear();
            *selected_episode.borrow_mut() = None;
            clear_list(&source_list);
            clear_list(&episode_list);
            play_button.set_sensitive(false);
            play_button.set_label("Play episode");
            episode_info.set_text(&format!("Loading player sources for {}…", voiceover.name));
            status.set_text(&format!("Loading sources for {}…", voiceover.name));

            let tx = tx.clone();
            run_api(move |api| {
                let result = api
                    .episode_sources(release_id, voiceover.id)
                    .map(|json| (extract_episode_sources(&json), json))
                    .map_err(|err| err.to_string());
                let _ = tx.send(UiMessage::Sources(release_id, voiceover.id, result));
            });
        });
    }

    // Source -> episodes.
    {
        let tx = tx.clone();
        let sources = sources.clone();
        let episodes = episodes.clone();
        let selected_episode = selected_episode.clone();
        let current_release = current_release.clone();
        let current_voiceover = current_voiceover.clone();
        let current_source = current_source.clone();
        let episode_list = episode_list.clone();
        let episode_info = episode_info.clone();
        let play_button = play_button.clone();
        let status = status.clone();

        source_list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let (Some(release_id), Some(voiceover_id)) =
                (current_release.get(), current_voiceover.get())
            else {
                return;
            };
            let Some(source) = sources.borrow().get(row.index() as usize).cloned() else {
                return;
            };

            current_source.set(Some(source.id));
            episodes.borrow_mut().clear();
            *selected_episode.borrow_mut() = None;
            clear_list(&episode_list);
            play_button.set_sensitive(false);
            play_button.set_label("Play episode");
            episode_info.set_text(&format!("Loading episodes from {}…", source.name));
            status.set_text(&format!("Loading episodes from {}…", source.name));

            let tx = tx.clone();
            run_api(move |api| {
                let result = api
                    .episode_list(release_id, voiceover_id, source.id)
                    .map(|json| (extract_episode_items(&json), json))
                    .map_err(|err| err.to_string());
                let _ = tx.send(UiMessage::EpisodeList(
                    release_id,
                    voiceover_id,
                    source.id,
                    result,
                ));
            });
        });
    }

    // Episode -> smart playback route.
    {
        let episodes = episodes.clone();
        let selected_episode = selected_episode.clone();
        let play_button = play_button.clone();
        let episode_info = episode_info.clone();
        episode_list.connect_row_selected(move |_, row| {
            let Some(row) = row else {
                *selected_episode.borrow_mut() = None;
                play_button.set_sensitive(false);
                play_button.set_label("Play episode");
                episode_info.set_text("Select an episode.");
                return;
            };
            let Some(episode) = episodes.borrow().get(row.index() as usize).cloned() else {
                return;
            };

            play_button.set_sensitive(episode.url.is_some());
            if let Some(url) = episode.url.as_deref() {
                match playback_route(url, episode.iframe) {
                    Ok(PlaybackRoute::Mpv) => play_button.set_label("Play in mpv"),
                    Ok(PlaybackRoute::EmbeddedWeb) => play_button.set_label("Play in app"),
                    Err(_) => play_button.set_label("Play episode"),
                }
            }
            episode_info.set_text(&episode_summary(&episode));
            *selected_episode.borrow_mut() = Some(episode);
        });
    }

    {
        let parent = parent.clone();
        let selected_episode = selected_episode.clone();
        let status = status.clone();
        play_button.connect_clicked(move |_| {
            play_selected_episode(&parent, &selected_episode, &status);
        });
    }

    {
        let parent = parent.clone();
        let selected_episode = selected_episode.clone();
        let status = status.clone();
        episode_list.connect_row_activated(move |_, _| {
            play_selected_episode(&parent, &selected_episode, &status);
        });
    }

    // Load voiceovers after the chooser exists, so icons can stream into it.
    {
        let tx = tx.clone();
        status.set_text(&format!("Loading voiceovers for {}…", card.title));
        run_api(move |api| {
            let result = api
                .episode_voiceovers(card.id)
                .map(|json| (extract_voiceovers(&json), json))
                .map_err(|err| err.to_string());
            let _ = tx.send(UiMessage::Voiceovers(card.id, result));
        });
    }

}

fn populate_release_grid(
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<Vec<ReleaseCard>>>,
    items: Vec<ReleaseCard>,
    tx: &Sender<UiMessage>,
    image_waiters: &ImageWaiters,
    image_cache: &ImageCache,
) {
    clear_flowbox(flow);
    *state.borrow_mut() = items;

    for card in state.borrow().iter() {
        let child = gtk::FlowBoxChild::new();
        child.set_halign(gtk::Align::Center);
        child.set_hexpand(false);
        let card_box = gtk::Box::new(Orientation::Vertical, 7);
        card_box.set_size_request(216, -1);
        card_box.set_halign(gtk::Align::Center);
        card_box.set_hexpand(false);
        card_box.add_css_class("card");
        set_margins(&card_box, 6);

        let picture = gtk::Picture::new();
        picture.set_size_request(204, 288);
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_halign(gtk::Align::Center);
        picture.set_hexpand(false);
        picture.set_alternative_text(Some(&card.title));
        card_box.append(&picture);
        bind_remote_image(
            card.poster_url.as_deref(),
            &picture,
            tx,
            image_waiters,
            image_cache,
        );

        let title = gtk::Label::new(Some(&card.title));
        title.set_xalign(0.0);
        title.set_wrap(true);
        title.set_max_width_chars(24);
        title.add_css_class("heading");
        card_box.append(&title);

        let mut meta = Vec::new();
        if let Some(year) = card.year {
            meta.push(year.to_string());
        }
        match (card.episodes_released, card.episodes_total) {
            (Some(current), Some(total)) => meta.push(format!("{current}/{total} ep")),
            (Some(current), None) => meta.push(format!("{current} ep")),
            (None, Some(total)) => meta.push(format!("{total} ep")),
            _ => {}
        }
        if let Some(rating) = card.rating {
            meta.push(format!("★ {rating:.1}"));
        }
        let meta_label = gtk::Label::new(Some(&meta.join("  •  ")));
        meta_label.set_xalign(0.0);
        meta_label.add_css_class("dim-label");
        card_box.append(&meta_label);

        child.set_child(Some(&card_box));
        flow.append(&child);
    }
}

fn populate_voiceover_filter(
    list: &gtk::ListBox,
    all: &Rc<RefCell<Vec<Voiceover>>>,
    visible: &Rc<RefCell<Vec<Voiceover>>>,
    filter: VoiceoverFilter,
    tx: &Sender<UiMessage>,
    image_waiters: &ImageWaiters,
    image_cache: &ImageCache,
) {
    clear_list(list);
    let filtered = all
        .borrow()
        .iter()
        .filter(|item| match filter {
            VoiceoverFilter::All => true,
            VoiceoverFilter::Voiceovers => !item.is_sub,
            VoiceoverFilter::Subtitles => item.is_sub,
        })
        .cloned()
        .collect::<Vec<_>>();
    *visible.borrow_mut() = filtered;

    for item in visible.borrow().iter() {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(Orientation::Horizontal, 12);
        set_margins(&box_, 10);

        let icon = gtk::Picture::new();
        icon.set_size_request(64, 64);
        icon.set_can_shrink(true);
        icon.set_content_fit(gtk::ContentFit::Cover);
        icon.add_css_class("card");
        bind_remote_image(
            item.icon_url.as_deref(),
            &icon,
            tx,
            image_waiters,
            image_cache,
        );
        box_.append(&icon);

        let text = gtk::Box::new(Orientation::Vertical, 3);
        text.set_hexpand(true);
        let name = gtk::Label::new(Some(&item.name));
        name.set_xalign(0.0);
        name.set_wrap(true);
        name.add_css_class("heading");
        text.append(&name);

        let mut meta = Vec::new();
        if let Some(count) = item.episodes_count {
            meta.push(format!("{count} episodes"));
        }
        if item.is_sub {
            meta.push("subtitles".to_string());
        }
        let meta = gtk::Label::new(Some(&meta.join("  •  ")));
        meta.set_xalign(0.0);
        meta.add_css_class("dim-label");
        text.append(&meta);
        box_.append(&text);

        if let Some(views) = item.view_count {
            let views = gtk::Label::new(Some(&format!("{}  ◉", human_count(views))));
            views.set_valign(gtk::Align::Center);
            views.add_css_class("dim-label");
            box_.append(&views);
        }

        row.set_child(Some(&box_));
        list.append(&row);
    }
}

fn populate_sources(list: &gtk::ListBox, state: &Rc<RefCell<Vec<EpisodeSource>>>) {
    clear_list(list);
    for item in state.borrow().iter() {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(Orientation::Vertical, 3);
        set_margins(&box_, 10);
        let label = gtk::Label::new(Some(&item.name));
        label.set_xalign(0.0);
        label.add_css_class("heading");
        box_.append(&label);
        let id = gtk::Label::new(Some(&format!("Source ID {}", item.id)));
        id.set_xalign(0.0);
        id.add_css_class("dim-label");
        box_.append(&id);
        row.set_child(Some(&box_));
        list.append(&row);
    }
}

fn populate_episodes(list: &gtk::ListBox, state: &Rc<RefCell<Vec<EpisodeItem>>>) {
    clear_list(list);
    for episode in state.borrow().iter() {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(Orientation::Horizontal, 10);
        set_margins(&box_, 10);
        let marker = if episode.is_watched { "✓" } else { "▶" };
        let number = gtk::Label::new(Some(&format!("{marker} {}", episode.position)));
        number.add_css_class("heading");
        box_.append(&number);
        let name = gtk::Label::new(Some(&episode.name));
        name.set_xalign(0.0);
        name.set_wrap(true);
        name.set_hexpand(true);
        box_.append(&name);
        row.set_child(Some(&box_));
        list.append(&row);
    }
}

fn chooser_column(title: &str, list: &gtk::ListBox, min_width: i32) -> gtk::Frame {
    let scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .min_content_width(min_width)
        .min_content_height(420)
        .child(list)
        .build();
    let frame = gtk::Frame::new(Some(title));
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    frame.set_child(Some(&scroll));
    frame
}

fn episode_summary(episode: &EpisodeItem) -> String {
    let watched = if episode.is_watched { " • watched" } else { "" };
    match episode.url.as_deref() {
        Some(url) => {
            let route = match playback_route(url, episode.iframe) {
                Ok(PlaybackRoute::Mpv) => "direct media → mpv",
                Ok(PlaybackRoute::EmbeddedWeb) if episode.iframe => {
                    "iframe/provider → native WebKit player"
                }
                Ok(PlaybackRoute::EmbeddedWeb) => "provider page → native WebKit player",
                Err(_) => "invalid/unsupported player URL",
            };
            format!(
                "Episode {} • {} • {route}{watched}",
                episode.position, episode.name
            )
        }
        None => format!(
            "Episode {} • {} • no player URL returned{watched}",
            episode.position, episode.name
        ),
    }
}

fn play_selected_episode(
    parent: &adw::ApplicationWindow,
    selected: &Rc<RefCell<Option<EpisodeItem>>>,
    status: &gtk::Label,
) {
    let Some(episode) = selected.borrow().clone() else {
        status.set_text("Select an episode first.");
        return;
    };
    let Some(url) = episode.url.as_deref() else {
        status.set_text("This episode does not contain a playable URL.");
        return;
    };

    let title = format!("{} — Episode {}", episode.name, episode.position);
    match playback_route(url, episode.iframe) {
        Ok(PlaybackRoute::Mpv) => match MpvBackend::play(url) {
            Ok(()) => status.set_text("Opened direct media stream in mpv."),
            Err(err) => status.set_text(&format!("mpv error: {err}")),
        },
        Ok(PlaybackRoute::EmbeddedWeb) => match EmbeddedWebBackend::open(parent, url, &title) {
            Ok(()) => status.set_text("Opened provider in the native WebKitGTK player."),
            Err(err) => status.set_text(&format!("Web player error: {err}")),
        },
        Err(err) => status.set_text(&format!("Player URL error: {err}")),
    }
}

fn bind_remote_image(
    url: Option<&str>,
    picture: &gtk::Picture,
    tx: &Sender<UiMessage>,
    waiters: &ImageWaiters,
    cache: &ImageCache,
) {
    let Some(url) = url.filter(|url| url.starts_with("https://") || url.starts_with("http://")) else {
        return;
    };

    if let Some(texture) = cache.borrow().get(url).cloned() {
        picture.set_paintable(Some(&texture));
        return;
    }

    let should_download = {
        let mut pending = waiters.borrow_mut();
        let first = !pending.contains_key(url);
        pending
            .entry(url.to_string())
            .or_default()
            .push(picture.clone());
        first
    };

    if should_download {
        let tx = tx.clone();
        let url = url.to_string();
        thread::spawn(move || {
            let result = download_image(&url);
            let _ = tx.send(UiMessage::ImageLoaded(url, result));
        });
    }
}

fn download_image(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("Anixart-Arch/{} (+Linux GTK)", APP_VERSION))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get(url)
        .send()
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    if response.content_length().is_some_and(|size| size > IMAGE_LIMIT) {
        return Err("image exceeds 12 MiB limit".to_string());
    }
    let data = response.bytes().map_err(|err| err.to_string())?;
    if data.len() as u64 > IMAGE_LIMIT {
        return Err("image exceeds 12 MiB limit".to_string());
    }
    Ok(data.to_vec())
}

fn show_settings(parent: &adw::ApplicationWindow, status: &gtk::Label) {
    let config = AppConfig::load().unwrap_or_default();
    let dialog = gtk::Window::builder()
        .title("Anixart Arch settings")
        .transient_for(parent)
        .modal(true)
        .default_width(620)
        .build();
    let box_ = gtk::Box::new(Orientation::Vertical, 12);
    set_margins(&box_, 20);

    let heading = gtk::Label::new(Some("Connection"));
    heading.set_xalign(0.0);
    heading.add_css_class("title-2");
    box_.append(&heading);

    let api_label = gtk::Label::new(Some("API base URL"));
    api_label.set_xalign(0.0);
    let api_entry = gtk::Entry::new();
    api_entry.set_text(&config.base_url);

    let token_label = gtk::Label::new(Some("API token (optional; stored mode 0600)"));
    token_label.set_xalign(0.0);
    let token_entry = gtk::PasswordEntry::new();
    token_entry.set_show_peek_icon(true);
    token_entry.set_text(config.token.as_deref().unwrap_or_default());

    let save = gtk::Button::with_label("Save settings");
    save.add_css_class("suggested-action");
    box_.append(&api_label);
    box_.append(&api_entry);
    box_.append(&token_label);
    box_.append(&token_entry);
    box_.append(&save);
    dialog.set_child(Some(&box_));

    let dialog_close = dialog.clone();
    let status = status.clone();
    save.connect_clicked(move |_| {
        let mut updated = AppConfig::load().unwrap_or_default();
        updated.base_url = api_entry.text().trim().trim_end_matches('/').to_owned();
        let token = token_entry.text().trim().to_owned();
        updated.token = (!token.is_empty()).then_some(token);
        match updated.save() {
            Ok(()) => {
                status.set_text(&format!("Settings saved to {}.", config_path().display()));
                dialog_close.close();
            }
            Err(err) => status.set_text(&format!("Could not save settings: {err}")),
        }
    });

    dialog.present();
}

fn show_play_url(parent: &adw::ApplicationWindow, status: &gtk::Label) {
    let dialog = gtk::Window::builder()
        .title("Open media/provider URL")
        .transient_for(parent)
        .modal(true)
        .default_width(680)
        .build();
    let box_ = gtk::Box::new(Orientation::Vertical, 10);
    set_margins(&box_, 16);
    let hint = gtk::Label::new(Some(
        "Developer tool: enter an HTTP(S) media/provider URL. It is passed directly to mpv without a shell.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://…"));
    let play = gtk::Button::with_label("Open in mpv");
    box_.append(&hint);
    box_.append(&entry);
    box_.append(&play);
    dialog.set_child(Some(&box_));

    let dialog_close = dialog.clone();
    let status = status.clone();
    play.connect_clicked(move |_| match MpvBackend::play(entry.text().as_str()) {
        Ok(()) => {
            status.set_text("Opened URL in mpv.");
            dialog_close.close();
        }
        Err(err) => status.set_text(&format!("Player error: {err}")),
    });
    dialog.present();
}

fn show_raw_api(parent: &adw::ApplicationWindow, raw: Option<&Value>) {
    let text = raw
        .map(pretty)
        .unwrap_or_else(|| "No API response has been captured yet.".to_string());
    show_text_window(parent, "Raw API inspector", &text, 900, 720);
}

fn show_text_window(
    parent: &adw::ApplicationWindow,
    title: &str,
    text: &str,
    width: i32,
    height: i32,
) {
    let window = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(false)
        .default_width(width)
        .default_height(height)
        .build();
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    view.buffer().set_text(text);
    let scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&view)
        .build();
    window.set_child(Some(&scroll));
    window.present();
}

fn icon_label_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    let box_ = gtk::Box::new(Orientation::Horizontal, 7);
    box_.set_halign(gtk::Align::Center);
    box_.append(&gtk::Image::from_icon_name(icon_name));
    box_.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&box_));
    button
}

fn nav_button(label: &str, icon_name: &str, active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_size_request(126, 58);
    let box_ = gtk::Box::new(Orientation::Vertical, 2);
    box_.set_halign(gtk::Align::Center);
    box_.append(&gtk::Image::from_icon_name(icon_name));
    box_.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&box_));
    button.add_css_class("flat");
    if active {
        button.add_css_class("suggested-action");
    }
    button
}

fn human_count(value: i64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{}K", value / 1_000)
    } else {
        value.to_string()
    }
}

fn run_api(task: impl FnOnce(ApiClient) + Send + 'static) {
    thread::spawn(move || {
        let config = AppConfig::load().unwrap_or_default();
        match ApiClient::from_config(&config) {
            Ok(api) => task(api),
            Err(err) => log::error!("cannot initialize API client: {err}"),
        }
    });
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn clear_flowbox(flow: &gtk::FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

fn refresh_status(label: &gtk::Label) {
    let config = AppConfig::load().unwrap_or_default();
    let auth = if config.has_token() {
        "token configured"
    } else {
        "no token"
    };
    label.set_text(&format!(
        "Anixart Arch {APP_VERSION} • {} • {}",
        config.base_url, auth
    ));
}

fn set_margins(widget: &impl IsA<gtk::Widget>, margin: i32) {
    widget.set_margin_top(margin);
    widget.set_margin_bottom(margin);
    widget.set_margin_start(margin);
    widget.set_margin_end(margin);
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
