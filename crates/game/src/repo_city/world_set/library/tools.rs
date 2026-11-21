use super::*;

pub(super) fn register(app: &mut App, _args: &[String]) {
    tablet_display::register(app);
    app.init_resource::<city_menu::Menu>()
        .init_resource::<file_picker::FilePicker>()
        .init_resource::<listing::find::Search>()
        .init_resource::<reading::Settings>()
        .init_resource::<guide::Tour>()
        .init_resource::<Nearest>()
        .init_resource::<file_facade::ChangesOnly>()
        .init_resource::<trace_import::Trace>()
        .init_resource::<review_party::Scene>()
        .configure_sets(Update, review_party::ProjectRoom.after(seeing::share))
        .insert_resource(city_boards::Placement {
            transforms: [-2.1, 2.1].map(|x| Transform::from_xyz(x, 2.35, -11.42)),
            size: Vec2::new(3.8, 2.26),
        })
        .add_plugins(city_boards::Plugin)
        .add_systems(PostUpdate, forms::apply.before(bevy::ui::UiSystems::Layout))
        .add_plugins(review_party::Plugin);
    markdown_preview::register(app);
    source_editor::register(app);
    #[cfg(target_arch = "wasm32")]
    app.init_resource::<browser_touch::Touch>()
        .add_systems(
            First,
            browser_touch::viewport.before(bevy::picking::PickingSystems::Input),
        )
        .add_systems(Update, reading::apply.before(room::draw));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut settings = reading::Settings::load();
        if _args.iter().any(|arg| arg == "--show-source") {
            settings.show_all_source = true;
        }
        app.insert_resource(settings)
            .add_plugins(desktop_splash::Plugin)
            .init_resource::<focus_context::Publisher>()
            .add_systems(Update, seeing::publish.after(seeing::update))
            .init_resource::<file_links::Links>()
            .add_systems(Update, file_links::update.before(review_tablet::update))
            .init_resource::<review_notes::ReviewUi>()
            .init_resource::<review_tablet::Tablet>()
            .init_resource::<terminal::Terminal>()
            .init_resource::<markdown_preview::Request>()
            .init_resource::<markdown_preview::ReadQueue>()
            .init_resource::<image_preview::Request>()
            .init_resource::<live_worktree::Live>()
            .add_systems(Update, terminal::update.after(review_tablet::update))
            .add_systems(PostUpdate, synchronize_cursor)
            .add_plugins((
                city_audio::Plugin,
                agent_session::Plugin,
                agent_timeline::Plugin,
                agent_navigation::Plugin,
            ));
    }
}

pub(super) fn requests(
    keys: Res<ButtonInput<KeyCode>>,
    library: Res<Library>,
    map: Res<map::Map>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    mut review: ResMut<review_notes::Review>,
    #[cfg(not(target_arch = "wasm32"))] picker: Res<repository_picker::RepositoryPicker>,
    #[cfg(not(target_arch = "wasm32"))] mut preview: ResMut<markdown_preview::Request>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if picker.blocks_controls() {
        return;
    }
    if map.open() || review.blocked || review.tablet_open || library.input.is_some() {
        return;
    }
    let modifier = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ]);
    if modifier {
        return;
    }
    if let Some(slot) = review_notes::SLOTS
        .iter()
        .position(|code| keys.just_pressed(*code))
    {
        review.select(slot);
    }
    let notes = keys.just_pressed(KeyCode::KeyN)
        || keys.just_pressed(KeyCode::KeyJ)
            && (library.reader.is_none()
                || keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    if notes {
        review.request = Some(review_notes::Panel::Aimed);
    } else if keys.just_pressed(KeyCode::KeyU) && review.can_paint {
        review.request = Some(review_notes::Panel::Paints);
    } else if keys.just_pressed(KeyCode::KeyP)
        && let Some(file) = repo.0.files.get(explorer.selected)
    {
        let range = library.reader.as_ref().map_or(0..SCROLL_ROWS, |reader| {
            reader.page(explorer.highlighted.len())
        });
        #[cfg(not(target_arch = "wasm32"))]
        if file.after.is_some() || file.before.is_some() {
            preview.0 = Some((explorer.selected, explorer.before, range, false));
        }
        #[cfg(target_arch = "wasm32")]
        markdown_preview::open_browser(&file.path, explorer.before, range.start, range.end, false);
    }
}

pub(super) fn destinations(
    repo: Res<Repo>,
    mut library: ResMut<Library>,
    mut review: ResMut<review_notes::Review>,
    mut party: ResMut<review_party::Party>,
    #[cfg(not(target_arch = "wasm32"))] mut picker: ResMut<file_picker::FilePicker>,
    #[cfg(not(target_arch = "wasm32"))] mut agent: ResMut<agent_navigation::Navigation>,
    #[cfg(not(target_arch = "wasm32"))] mut links: ResMut<file_links::Links>,
) {
    let source = |destination| (destination, listing::Face::Source);
    let cue = review_notes::take_destination(Some(&mut review))
        .map(source)
        .or_else(|| party.take_destination(&repo.0));
    #[cfg(not(target_arch = "wasm32"))]
    let cue = cue
        .or_else(|| agent.take_destination(&repo.0).map(source))
        .or_else(|| links.take_destination().map(source));
    if let Some((cue, face)) = cue
        && cue.revision == repo.0.revision
        && let Some(index) = repo
            .0
            .files
            .iter()
            .position(|file| file.path == cue.anchor.path)
    {
        let file = &repo.0.files[index];
        if (if cue.anchor.before {
            &file.before
        } else {
            &file.after
        })
        .is_some()
        {
            library.destination = Some(Destination {
                index,
                before: cue.anchor.before,
                line: cue.anchor.start.max(1),
                end: Some(cue.anchor.end.max(cue.anchor.start)),
                face,
            });
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let file = picker.destination.take();
    #[cfg(target_arch = "wasm32")]
    let file = {
        let path = browser_file_destination();
        repo.0.files.iter().position(|file| file.path == path)
    };
    if let Some(index) = file
        && let Some(file) = repo.0.files.get(index)
    {
        library.destination = Some(Destination {
            index,
            before: file.after.is_none(),
            line: 1,
            end: None,
            face: listing::Face::Source,
        });
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = takeFileDestination)]
    fn browser_file_destination() -> String;
}
