//! Ostracon: a local, Todoist-style to-do list.

use gpui_kit::*;
use ostracon_app::platform::Notifier;
use ostracon_app::state::AppState;
use ostracon_app::ui::actions::{self, *};
use ostracon_app::ui::workspace::Workspace;
use ostracon_core::Repository;
use ostracon_core::paths;

/// Must match `CFBundleIdentifier` in `scripts/bundle-macos.sh`.
const BUNDLE_ID: &str = "uk.bphilip.ostracon.app";

fn main() {
    let location = paths::database_location();
    let (repo, startup_error) = match Repository::open(&location.path) {
        Ok(repo) => (repo, None),
        Err(e) => (
            Repository::in_memory().expect("an in-memory SQLite database"),
            Some(format!(
                "Couldn't open your saved tasks ({e}). Changes this session won't be saved."
            )),
        ),
    };
    // A database from OSTRACON_DATABASE, such as a test one, mustn't touch the real one's backups.
    let settings = (!location.overridden && startup_error.is_none()).then(paths::settings_path);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::sync_system_appearance(None, cx);
            ostracon_app::ui::theme::install_font(cx);
            cx.set_app_identity(BUNDLE_ID, "Ostracon");
            actions::bind_keys(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(vec![
                Menu {
                    name: "Ostracon".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("Settings…", OpenSettings),
                        MenuItem::separator(),
                        MenuItem::action("Quit Ostracon", Quit),
                    ],
                },
                Menu {
                    name: "File".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("New Task", NewTask),
                        MenuItem::action("New Sub-task", NewSubtask),
                        MenuItem::action("New Project", NewProject),
                    ],
                },
                Menu {
                    name: "Edit".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("Undo Completion", Undo),
                        MenuItem::separator(),
                        MenuItem::os_action("Cut", ostracon_app::ui::quick_add::Cut, OsAction::Cut),
                        MenuItem::os_action(
                            "Copy",
                            ostracon_app::ui::quick_add::Copy,
                            OsAction::Copy,
                        ),
                        MenuItem::os_action(
                            "Paste",
                            ostracon_app::ui::quick_add::Paste,
                            OsAction::Paste,
                        ),
                        MenuItem::os_action(
                            "Select All",
                            ostracon_app::ui::quick_add::SelectAll,
                            OsAction::SelectAll,
                        ),
                    ],
                },
                Menu {
                    name: "Task".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("Complete", Complete),
                        MenuItem::action("Edit", Edit),
                        MenuItem::action("Open", OpenOverview),
                        MenuItem::separator(),
                        MenuItem::action("Indent", Indent),
                        MenuItem::action("Outdent", Outdent),
                        MenuItem::action("Move Up", MoveUp),
                        MenuItem::action("Move Down", MoveDown),
                        MenuItem::separator(),
                        MenuItem::action("Delete…", Delete),
                    ],
                },
                Menu {
                    name: "View".into(),
                    disabled: false,
                    items: vec![MenuItem::action(
                        "Show or Hide Completed",
                        ToggleShowCompleted,
                    )],
                },
            ]);

            let mut state = AppState::new(repo, settings);
            state.error = startup_error;
            let notifier = Notifier::new();
            let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Ostracon".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.), px(14.))),
                }),
                window_min_size: Some(size(px(720.), px(480.))),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Workspace::new(state, notifier, window, cx))
            })
            .expect("failed to open the window");
            cx.activate(true);
        });
}
