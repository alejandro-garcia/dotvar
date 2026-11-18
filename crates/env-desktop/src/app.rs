use crate::i18n::tr;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
use std::path::PathBuf;

pub(crate) const BUNDLE_ID: &str = "com.dotvar.app";

actions!(
    dotvar,
    [
        OpenFile,
        OpenProject,
        OpenSettings,
        ToggleCommandPalette,
        NewFile,
        Save,
        SaveAs,
        UndoDocument,
        RedoDocument,
        AddVariable,
        Quit,
        CloseWindow,
        CloseTab,
        RemoveProject,
        LocateProject,
        MoveTabLeft,
        MoveTabRight,
        ResetCatalog,
        CopyDocument,
        FindKey,
        FindNext,
        FindPrevious,
        NextTab,
        PreviousTab,
        QaResize,
        NextField,
        PreviousField
    ]
);

#[derive(Clone, Action, PartialEq, Eq)]
#[action(namespace = dotvar, no_json)]
pub(crate) struct SelectTabAt(pub usize);

/// Native entry point. An optional path is explicit user input, never a fixture default.
pub fn run(initial_path: Option<PathBuf>) {
    let started = std::time::Instant::now();
    let preferences = crate::settings::AppSettings::load();
    let (file_opens, pending_file_opens) = async_channel::unbounded::<Vec<PathBuf>>();
    application_with_file_open_events(file_opens)
        .with_assets(crate::design::Assets)
        .run(move |cx| {
            init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFile, None),
                KeyBinding::new("cmd-shift-o", OpenProject, None),
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("cmd-k", ToggleCommandPalette, None),
                KeyBinding::new("cmd-n", NewFile, None),
                KeyBinding::new("cmd-s", Save, None),
                KeyBinding::new("cmd-shift-s", SaveAs, None),
                KeyBinding::new("cmd-z", UndoDocument, None),
                KeyBinding::new("cmd-shift-z", RedoDocument, None),
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-w", CloseTab, None),
                KeyBinding::new("cmd-w", CloseWindow, Some("Settings")),
                KeyBinding::new("cmd-shift-w", CloseWindow, None),
                KeyBinding::new("cmd-shift-c", CopyDocument, None),
                KeyBinding::new("cmd-f", FindKey, None),
                KeyBinding::new("cmd-g", FindNext, None),
                KeyBinding::new("cmd-shift-g", FindPrevious, None),
                KeyBinding::new("ctrl-tab", NextTab, None),
                KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
                KeyBinding::new("tab", NextField, Some("EnvDocumentRow")),
                KeyBinding::new("shift-tab", PreviousField, Some("EnvDocumentRow")),
                KeyBinding::new("tab", NextField, Some("EnvDocumentRow > Input")),
                KeyBinding::new("shift-tab", PreviousField, Some("EnvDocumentRow > Input")),
                KeyBinding::new(
                    "space",
                    gpui_kit::base::actions::Confirm { secondary: false },
                    Some("VariableMenu > PopupMenu"),
                ),
                KeyBinding::new(
                    "tab",
                    gpui_kit::base::actions::SelectDown,
                    Some("VariableMenu > PopupMenu"),
                ),
                KeyBinding::new(
                    "shift-tab",
                    gpui_kit::base::actions::SelectUp,
                    Some("VariableMenu > PopupMenu"),
                ),
            ]);
            cx.bind_keys(
                [
                    "cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-5", "cmd-6", "cmd-7", "cmd-8", "cmd-9",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, key)| KeyBinding::new(key, SelectTabAt(index), None)),
            );
            if crate::config::app_env("QA_FLOATING").as_deref() == Some(std::ffi::OsStr::new("1")) {
                cx.bind_keys([KeyBinding::new("cmd-alt-9", QaResize, None)]);
            }
            cx.on_action(|_: &Quit, cx| cx.quit());
            set_menus(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    crate::platform::allow_quit();
                    cx.quit();
                }
            })
            .detach();
            let mut options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(832.)),
                    cx,
                ))),
                window_min_size: Some(size(px(900.), px(600.))),
                app_id: Some(BUNDLE_ID.into()),
                ..TitleBar::window_options()
            };
            if let Some(titlebar) = &mut options.titlebar {
                titlebar.traffic_light_position = Some(point(px(10.), px(12.)));
            }
            let preferences = cx.new(|_| preferences);
            match open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    crate::ui::AppWindow::new(
                        initial_path,
                        started,
                        preferences.clone(),
                        window,
                        cx,
                    )
                })
            }) {
                Ok((handle, view)) => {
                    crate::settings::register(handle, &view, preferences, cx);
                    let weak = view.downgrade();
                    let async_cx = cx.to_async();
                    let guard = crate::platform::install_quit_guard(move || {
                        let async_cx = async_cx.clone();
                        let weak = weak.clone();
                        async_cx.update(move |cx| {
                            cx.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    window.activate_window();
                                    let _ =
                                        weak.update(cx, |this, cx| this.request_close(window, cx));
                                });
                            })
                        });
                    });
                    if let Err(error) = guard {
                        eprintln!("{}", tr!("app.quit_guard_failed", error = error));
                        crate::platform::allow_quit();
                        cx.quit();
                        return;
                    }
                    let weak = view.downgrade();
                    cx.spawn(async move |cx| {
                        while let Ok(paths) = pending_file_opens.recv().await {
                            let opened = cx.update(|cx| {
                                handle.update(cx, |_, window, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.open_external_files(paths, window, cx);
                                    });
                                    window.activate_window();
                                })
                            });
                            if opened.is_err() {
                                break;
                            }
                        }
                    })
                    .detach();
                    if crate::config::app_env("QA_FLOATING").as_deref()
                        == Some(std::ffi::OsStr::new("1"))
                    {
                        crate::platform::keep_qa_window_visible();
                    }
                    cx.activate(true);
                }
                Err(_) => {
                    eprintln!("{}", tr!("app.window_failed"));
                    cx.quit();
                }
            }
        });
}

fn application_with_file_open_events(
    file_opens: async_channel::Sender<Vec<PathBuf>>,
) -> Application {
    let app = application();
    // Finder can deliver an open event before the main window is ready.
    app.on_open_urls(move |urls| {
        let paths: Vec<_> = urls
            .into_iter()
            .filter_map(|url| url::Url::parse(&url).ok())
            .filter(|url| url.scheme() == "file")
            .filter_map(|url| url.to_file_path().ok())
            .filter(|path| crate::model::is_env_file(path))
            .collect();
        if !paths.is_empty() {
            let _ = file_opens.try_send(paths);
        }
    });
    app
}

pub(crate) fn set_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Dotvar".into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("settings.menu"), OpenSettings),
                MenuItem::separator(),
                MenuItem::action(tr!("app.quit"), Quit),
            ],
        },
        Menu {
            name: tr!("menu.file").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("file.create"), NewFile),
                MenuItem::action(tr!("file.open"), OpenFile),
                MenuItem::action(tr!("project.add_action"), OpenProject),
                MenuItem::action(tr!("project.find"), ToggleCommandPalette),
                MenuItem::separator(),
                MenuItem::action(tr!("file.save"), Save),
                MenuItem::action(tr!("file.save_as"), SaveAs),
                MenuItem::separator(),
                MenuItem::action(tr!("tab.close"), CloseTab),
                MenuItem::action(tr!("window.close"), CloseWindow),
                MenuItem::action(tr!("tab.move_left"), MoveTabLeft),
                MenuItem::action(tr!("tab.move_right"), MoveTabRight),
                MenuItem::action(tr!("tab.next"), NextTab),
                MenuItem::action(tr!("tab.previous"), PreviousTab),
            ],
        },
        Menu {
            name: tr!("menu.edit").into(),
            disabled: false,
            items: vec![
                MenuItem::action(tr!("action.undo"), UndoDocument),
                MenuItem::action(tr!("action.redo"), RedoDocument),
                MenuItem::separator(),
                MenuItem::action(tr!("variable.add_action"), AddVariable),
                MenuItem::separator(),
                MenuItem::action(tr!("editor.copy_document"), CopyDocument),
                MenuItem::action(tr!("variable.find"), FindKey),
                MenuItem::action(tr!("search.next_match"), FindNext),
                MenuItem::action(tr!("search.previous_match"), FindPrevious),
            ],
        },
    ]);
}
