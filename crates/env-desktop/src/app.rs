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
        ToggleAllValues,
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
            let qa =
                crate::config::app_env("QA_FLOATING").as_deref() == Some(std::ffi::OsStr::new("1"));
            let bindings: Vec<_> = shortcut_table(qa)
                .into_iter()
                .map(|shortcut| shortcut.binding(cx))
                .collect();
            cx.bind_keys(bindings);
            if cfg!(target_os = "macos") {
                // AppKit funnels termination through the native quit guard.
                cx.on_action(|_: &Quit, cx| cx.quit());
            }
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
            if cfg!(target_os = "macos")
                && let Some(titlebar) = &mut options.titlebar
            {
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
                    if !cfg!(target_os = "macos") {
                        route_quit_through_the_window(handle, view.downgrade(), cx);
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

/// One entry of the application keymap. `secondary-` resolves to Cmd on macOS
/// and Ctrl on Windows and Linux.
pub(crate) struct Shortcut {
    pub(crate) keys: &'static str,
    pub(crate) context: Option<&'static str>,
    action: Box<dyn Action>,
}

impl Shortcut {
    fn new(keys: &'static str, action: impl Action, context: Option<&'static str>) -> Self {
        Self {
            keys,
            context,
            action: Box::new(action),
        }
    }

    fn binding(self, cx: &App) -> KeyBinding {
        let predicate = self.context.map(|context| {
            std::rc::Rc::new(KeyBindingContextPredicate::parse(context).expect("valid key context"))
        });
        KeyBinding::load(self.keys, self.action, predicate, false, None, cx)
            .expect("valid shortcut")
    }
}

/// Every application shortcut. The QA-only resize binding is opt-in.
pub(crate) fn shortcut_table(qa: bool) -> Vec<Shortcut> {
    const TABS: [&str; 9] = [
        "secondary-1",
        "secondary-2",
        "secondary-3",
        "secondary-4",
        "secondary-5",
        "secondary-6",
        "secondary-7",
        "secondary-8",
        "secondary-9",
    ];
    let mut table = vec![
        Shortcut::new("secondary-o", OpenFile, None),
        Shortcut::new("secondary-shift-o", OpenProject, None),
        Shortcut::new("secondary-,", OpenSettings, None),
        Shortcut::new("secondary-k", ToggleCommandPalette, None),
        Shortcut::new("secondary-n", NewFile, None),
        Shortcut::new("secondary-s", Save, None),
        Shortcut::new("secondary-shift-s", SaveAs, None),
        Shortcut::new("secondary-z", UndoDocument, None),
        Shortcut::new("secondary-shift-z", RedoDocument, None),
        Shortcut::new("secondary-q", Quit, None),
        Shortcut::new("secondary-w", CloseTab, None),
        Shortcut::new("secondary-w", CloseWindow, Some("Settings")),
        Shortcut::new("secondary-shift-w", CloseWindow, None),
        Shortcut::new("secondary-shift-c", CopyDocument, None),
        Shortcut::new("secondary-shift-h", ToggleAllValues, None),
        Shortcut::new("secondary-f", FindKey, None),
        Shortcut::new("secondary-g", FindNext, None),
        Shortcut::new("secondary-shift-g", FindPrevious, None),
        Shortcut::new("ctrl-tab", NextTab, None),
        Shortcut::new("ctrl-shift-tab", PreviousTab, None),
        Shortcut::new("tab", NextField, Some("EnvDocumentRow")),
        Shortcut::new("shift-tab", PreviousField, Some("EnvDocumentRow")),
        Shortcut::new("tab", NextField, Some("EnvDocumentRow > Input")),
        Shortcut::new("shift-tab", PreviousField, Some("EnvDocumentRow > Input")),
        Shortcut::new(
            "space",
            gpui_kit::base::actions::Confirm { secondary: false },
            Some("VariableMenu > PopupMenu"),
        ),
        Shortcut::new(
            "tab",
            gpui_kit::base::actions::SelectDown,
            Some("VariableMenu > PopupMenu"),
        ),
        Shortcut::new(
            "shift-tab",
            gpui_kit::base::actions::SelectUp,
            Some("VariableMenu > PopupMenu"),
        ),
    ];
    table.extend(
        TABS.into_iter()
            .enumerate()
            .map(|(index, keys)| Shortcut::new(keys, SelectTabAt(index), None)),
    );
    if qa {
        table.push(Shortcut::new("secondary-alt-9", QaResize, None));
    }
    table
}

/// Off macOS nothing intercepts termination, so Quit asks the main window to
/// close through the same unsaved-change flow as its close button. Once that
/// window is gone the app ends even if Settings is still open.
fn route_quit_through_the_window(
    handle: AnyWindowHandle,
    view: WeakEntity<crate::ui::AppWindow>,
    cx: &mut App,
) {
    cx.on_action(move |_: &Quit, cx| {
        let view = view.clone();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                let _ = view.update(cx, |this, cx| this.request_close(window, cx));
            });
        });
    });
    cx.on_window_closed(move |cx, _| {
        if !cx
            .windows()
            .iter()
            .any(|open| open.window_id() == handle.window_id())
        {
            cx.quit();
        }
    })
    .detach();
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

#[cfg(test)]
mod tests {
    use super::shortcut_table;
    use gpui_kit::{Keystroke, OperatingSystem};
    use std::collections::HashSet;

    const OSES: [OperatingSystem; 3] = [
        OperatingSystem::Mac,
        OperatingSystem::Windows,
        OperatingSystem::Linux,
    ];

    fn resolved(os: OperatingSystem, qa: bool) -> Vec<(Keystroke, Option<&'static str>)> {
        shortcut_table(qa)
            .into_iter()
            .map(|shortcut| {
                (
                    Keystroke::parse_with_os(shortcut.keys, os)
                        .unwrap_or_else(|_| panic!("unparsable shortcut {}", shortcut.keys)),
                    shortcut.context,
                )
            })
            .collect()
    }

    #[test]
    fn shortcuts_use_ctrl_off_macos_and_never_the_windows_key() {
        for os in [OperatingSystem::Windows, OperatingSystem::Linux] {
            for (keystroke, context) in resolved(os, true) {
                assert!(
                    !keystroke.modifiers.platform,
                    "{keystroke:?} {context:?} needs the platform key on {os:?}"
                );
            }
            let open = Keystroke::parse("ctrl-o").unwrap();
            assert!(resolved(os, false).contains(&(open, None)));
        }
    }

    #[test]
    fn macos_keeps_every_previous_command_binding() {
        let previous: [(&str, Option<&str>); 25] = [
            ("cmd-o", None),
            ("cmd-shift-o", None),
            ("cmd-,", None),
            ("cmd-k", None),
            ("cmd-n", None),
            ("cmd-s", None),
            ("cmd-shift-s", None),
            ("cmd-z", None),
            ("cmd-shift-z", None),
            ("cmd-q", None),
            ("cmd-w", None),
            ("cmd-w", Some("Settings")),
            ("cmd-shift-w", None),
            ("cmd-shift-c", None),
            ("cmd-f", None),
            ("cmd-g", None),
            ("cmd-shift-g", None),
            ("ctrl-tab", None),
            ("ctrl-shift-tab", None),
            ("tab", Some("EnvDocumentRow")),
            ("shift-tab", Some("EnvDocumentRow")),
            ("tab", Some("EnvDocumentRow > Input")),
            ("shift-tab", Some("EnvDocumentRow > Input")),
            ("space", Some("VariableMenu > PopupMenu")),
            ("tab", Some("VariableMenu > PopupMenu")),
        ];
        let mut expected: Vec<_> = previous
            .iter()
            .map(|(keys, context)| (Keystroke::parse(keys).unwrap(), *context))
            .collect();
        expected.push((
            Keystroke::parse("shift-tab").unwrap(),
            Some("VariableMenu > PopupMenu"),
        ));
        for digit in 1..=9 {
            let keys: &'static str = Box::leak(format!("cmd-{digit}").into_boxed_str());
            expected.push((Keystroke::parse(keys).unwrap(), None));
        }
        expected.push((Keystroke::parse("cmd-shift-h").unwrap(), None));
        expected.push((Keystroke::parse("cmd-alt-9").unwrap(), None));
        let actual = resolved(OperatingSystem::Mac, true);
        for binding in &expected {
            assert!(
                actual.contains(binding),
                "missing macOS binding {binding:?}"
            );
        }
        assert_eq!(actual.len(), expected.len());
    }

    #[test]
    fn no_two_shortcuts_share_a_keystroke_and_context_on_any_platform() {
        for os in OSES {
            let mut seen = HashSet::new();
            for binding in resolved(os, true) {
                assert!(
                    seen.insert(binding.clone()),
                    "{binding:?} repeats on {os:?}"
                );
            }
        }
    }

    #[test]
    fn toggle_all_values_is_bound_once_to_secondary_shift_h_without_collisions() {
        let bound: Vec<_> = shortcut_table(true)
            .into_iter()
            .filter(|shortcut| shortcut.action.name() == "dotvar::ToggleAllValues")
            .map(|shortcut| (shortcut.keys, shortcut.context))
            .collect();
        assert_eq!(bound, [("secondary-shift-h", None)]);
        for (os, expected) in [
            (OperatingSystem::Mac, "cmd-shift-h"),
            (OperatingSystem::Windows, "ctrl-shift-h"),
            (OperatingSystem::Linux, "ctrl-shift-h"),
        ] {
            let keystroke = Keystroke::parse(expected).unwrap();
            let bindings = resolved(os, true);
            assert_eq!(
                bindings.iter().filter(|(k, _)| *k == keystroke).count(),
                1,
                "{expected} on {os:?}"
            );
            assert!(bindings.contains(&(keystroke, None)));
        }
    }

    #[test]
    fn tab_cycling_stays_on_ctrl_tab_and_does_not_collide_with_ctrl_digits() {
        for os in OSES {
            let bindings = resolved(os, true);
            for keys in ["ctrl-tab", "ctrl-shift-tab"] {
                let keystroke = Keystroke::parse(keys).unwrap();
                assert_eq!(
                    bindings.iter().filter(|(k, _)| *k == keystroke).count(),
                    1,
                    "{keys} on {os:?}"
                );
            }
        }
    }
}
