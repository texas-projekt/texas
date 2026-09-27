use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    event::EventPropagation,
    reactive::{
        Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
    },
    style::{AlignItems, CursorStyle, Display},
    views::{Decorators, label, stack, svg},
};
use texas_core::{
    buffer::rope_text::RopeText,
    mode::{Mode, VisualMode},
};

use crate::{
    app::clickable_icon,
    command::TexasWorkbenchCommand,
    config::{TexasConfig, color::TexasColor, icon::TexasIcons},
    editor::EditorData,
    listener::Listener,
    palette::kind::PaletteKind,
    panel::position::PanelContainerPosition,
    source_control::SourceControlData,
    window_tab::WindowTabData,
};

pub fn status(
    window_tab_data: Rc<WindowTabData>,
    source_control: SourceControlData,
    workbench_command: Listener<TexasWorkbenchCommand>,
    status_height: RwSignal<f64>,
    _config: ReadSignal<Arc<TexasConfig>>,
) -> impl View {
    let config = window_tab_data.common.config;
    let i18n = window_tab_data.common.i18n.clone();
    let editor = window_tab_data.main_split.active_editor;
    let panel = window_tab_data.panel.clone();
    let palette = window_tab_data.palette.clone();
    let branch = source_control.branch;
    let file_diffs = source_control.file_diffs;
    let branch = move || {
        format!(
            "{}{}",
            branch.get(),
            if file_diffs.with(|diffs| diffs.is_empty()) {
                ""
            } else {
                "*"
            }
        )
    };

    let mode = create_memo(move |_| window_tab_data.mode());
    let pointer_down = floem::reactive::create_rw_signal(false);

    let mode_i18n = i18n.clone();
    stack((
        stack((
            label(move || match mode.get() {
                Mode::Normal => mode_i18n.text("keymap.normal"),
                Mode::Insert => mode_i18n.text("keymap.insert"),
                Mode::Visual(mode) => match mode {
                    VisualMode::Normal => mode_i18n.text("keymap.visual"),
                    VisualMode::Linewise => mode_i18n.text("keymap.visual-line"),
                    VisualMode::Blockwise => mode_i18n.text("keymap.visual-block"),
                },
                Mode::Terminal => mode_i18n.text("keymap.terminal"),
            })
            .style(move |s| {
                let config = config.get();
                let display = if config.core.modal {
                    Display::Flex
                } else {
                    Display::None
                };

                let (bg, fg) = match mode.get() {
                    Mode::Normal => (
                        TexasColor::STATUS_MODAL_NORMAL_BACKGROUND,
                        TexasColor::STATUS_MODAL_NORMAL_FOREGROUND,
                    ),
                    Mode::Insert => (
                        TexasColor::STATUS_MODAL_INSERT_BACKGROUND,
                        TexasColor::STATUS_MODAL_INSERT_FOREGROUND,
                    ),
                    Mode::Visual(_) => (
                        TexasColor::STATUS_MODAL_VISUAL_BACKGROUND,
                        TexasColor::STATUS_MODAL_VISUAL_FOREGROUND,
                    ),
                    Mode::Terminal => (
                        TexasColor::STATUS_MODAL_TERMINAL_BACKGROUND,
                        TexasColor::STATUS_MODAL_TERMINAL_FOREGROUND,
                    ),
                };

                let bg = config.color(bg);
                let fg = config.color(fg);

                s.display(display)
                    .padding_horiz(10.0)
                    .color(fg)
                    .background(bg)
                    .height_pct(100.0)
                    .align_items(Some(AlignItems::Center))
                    .selectable(false)
            }),
            stack((
                svg(move || config.get().ui_svg(TexasIcons::SCM)).style(move |s| {
                    let config = config.get();
                    let icon_size = config.ui.icon_size() as f32;
                    s.size(icon_size, icon_size)
                        .color(config.color(TexasColor::TEXAS_ICON_ACTIVE))
                }),
                label(branch).style(move |s| {
                    s.margin_left(10.0)
                        .color(config.get().color(TexasColor::STATUS_FOREGROUND))
                        .selectable(false)
                }),
            ))
            .style(move |s| {
                s.display(if branch().is_empty() {
                    Display::None
                } else {
                    Display::Flex
                })
                .height_pct(100.0)
                .padding_horiz(10.0)
                .align_items(Some(AlignItems::Center))
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(TexasColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
            .on_event_cont(floem::event::EventListener::PointerDown, move |_| {
                pointer_down.set(true);
            })
            .on_event(
                floem::event::EventListener::PointerUp,
                move |_| {
                    if pointer_down.get() {
                        workbench_command
                            .send(TexasWorkbenchCommand::PaletteSCMReferences);
                    }
                    pointer_down.set(false);
                    EventPropagation::Continue
                },
            ),
        ))
        .style(|s| {
            s.height_pct(100.0)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0_f32)
                .items_center()
        }),
        stack((
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Left, true)
                        {
                            TexasIcons::SIDEBAR_LEFT
                        } else {
                            TexasIcons::SIDEBAR_LEFT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Left)
                    },
                    || false,
                    || false,
                    i18n.text_signal("panel.toggle-left"),
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel.is_container_shown(
                            &PanelContainerPosition::Bottom,
                            true,
                        ) {
                            TexasIcons::LAYOUT_PANEL
                        } else {
                            TexasIcons::LAYOUT_PANEL_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel
                            .toggle_container_visual(&PanelContainerPosition::Bottom)
                    },
                    || false,
                    || false,
                    i18n.text_signal("panel.toggle-bottom"),
                    config,
                )
            },
            {
                let panel = panel.clone();
                let icon = {
                    let panel = panel.clone();
                    move || {
                        if panel
                            .is_container_shown(&PanelContainerPosition::Right, true)
                        {
                            TexasIcons::SIDEBAR_RIGHT
                        } else {
                            TexasIcons::SIDEBAR_RIGHT_OFF
                        }
                    }
                };
                clickable_icon(
                    icon,
                    move || {
                        panel.toggle_container_visual(&PanelContainerPosition::Right)
                    },
                    || false,
                    || false,
                    i18n.text_signal("panel.toggle-right"),
                    config,
                )
            },
        ))
        .style(move |s| {
            s.height_pct(100.0)
                .items_center()
                .color(config.get().color(TexasColor::STATUS_FOREGROUND))
        }),
        stack({
            let palette_clone = palette.clone();
            let cursor_i18n = i18n.clone();
            let line_count_i18n = i18n.clone();
            let line_count_info = status_text(
                config,
                editor,
                move || {
                    if let Some(editor) = editor.get() {
                        let line_count = editor
                            .doc_signal()
                            .get()
                            .buffer
                            .with(|buffer| buffer.last_line() + 1);
                        line_count_i18n.text_with_args(
                            "status.total-lines",
                            "{count} lines",
                            &[("count", &line_count.to_string())],
                        )
                    } else {
                        String::new()
                    }
                },
                false,
            );
            let cursor_info = status_text(
                config,
                editor,
                move || {
                    if let Some(editor) = editor.get() {
                        let mut status = String::new();
                        let cursor = editor.cursor().get();
                        if let Some((line, column, character)) = editor
                            .doc_signal()
                            .get()
                            .buffer
                            .with(|buffer| cursor.get_line_col_char(buffer))
                        {
                            status = cursor_i18n.text_with_args(
                                "status.cursor-position",
                                "Ln {line}, Col {column}, Char {char}",
                                &[
                                    ("line", &(line + 1).to_string()),
                                    ("column", &(column + 1).to_string()),
                                    ("char", &character.to_string()),
                                ],
                            );
                        }
                        if let Some(selection) = cursor.get_selection() {
                            let selection_range = selection.0.abs_diff(selection.1);

                            if selection.0 != selection.1 {
                                status = cursor_i18n.text_with_args(
                                    "status.selected",
                                    "{base} ({count} selected)",
                                    &[
                                        ("base", &status),
                                        ("count", &selection_range.to_string()),
                                    ],
                                );
                            }
                        }
                        let selection_count = cursor.get_selection_count();
                        if selection_count > 1 {
                            status = cursor_i18n.text_with_args(
                                "status.selections",
                                "{base} {count} selections",
                                &[
                                    ("base", &status),
                                    ("count", &selection_count.to_string()),
                                ],
                            );
                        }
                        return status;
                    }
                    String::new()
                },
                true,
            )
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Line);
            });
            let palette_clone = palette.clone();
            let line_ending_info = status_text(
                config,
                editor,
                move || {
                    if let Some(editor) = editor.get() {
                        let doc = editor.doc_signal().get();
                        doc.buffer.with(|b| b.line_ending()).as_str()
                    } else {
                        ""
                    }
                },
                true,
            )
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::LineEnding);
            });
            let palette_clone = palette.clone();
            let language_i18n = i18n.clone();
            let language_info = status_text(
                config,
                editor,
                move || {
                    if let Some(editor) = editor.get() {
                        let doc = editor.doc_signal().get();
                        doc.syntax().with(|s| s.language.name()).to_string()
                    } else {
                        language_i18n.text("status.unknown-language")
                    }
                },
                true,
            )
            .on_click_stop(move |_| {
                palette_clone.run(PaletteKind::Language);
            });
            (
                line_count_info,
                cursor_info,
                line_ending_info,
                language_info,
            )
        })
        .style(|s| {
            s.height_pct(100.0)
                .flex_basis(0.0)
                .flex_grow(1.0_f32)
                .justify_end()
        }),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != status_height.get_untracked() {
            status_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.border_top(1.0)
            .border_color(config.color(TexasColor::TEXAS_BORDER))
            .background(config.color(TexasColor::STATUS_BACKGROUND))
            .flex_basis(config.ui.status_height() as f32)
            .flex_grow(0.0_f32)
            .flex_shrink(0.0_f32)
            .items_center()
    })
    .debug_name("Status/Bottom Bar")
}

fn status_text<S: std::fmt::Display + 'static>(
    config: ReadSignal<Arc<TexasConfig>>,
    editor: Memo<Option<EditorData>>,
    text: impl Fn() -> S + 'static,
    interactive: bool,
) -> impl View {
    label(text).style(move |s| {
        let config = config.get();
        let display = if editor
            .get()
            .map(|editor| {
                editor.doc_signal().get().content.with(|c| {
                    use crate::doc::DocContent;
                    matches!(c, DocContent::File { .. } | DocContent::Scratch { .. })
                })
            })
            .unwrap_or(false)
        {
            Display::Flex
        } else {
            Display::None
        };

        s.display(display)
            .height_full()
            .padding_horiz(10.0)
            .items_center()
            .color(config.color(TexasColor::STATUS_FOREGROUND))
            .apply_if(interactive, |s| {
                s.hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(TexasColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
            .selectable(false)
    })
}
