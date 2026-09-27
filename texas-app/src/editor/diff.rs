use std::{
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{self, AtomicBool},
    },
};

use super::{EditorData, EditorViewKind};
use crate::{
    config::{color::TexasColor, icon::TexasIcons},
    doc::{Doc, DocContent},
    editor_tab::{EditorTabChild, EditorTabData},
    id::{DiffEditorId, EditorTabId},
    main_split::{Editors, MainSplitData},
    window_tab::CommonData,
};
use floem::{
    View,
    event::{Event, EventListener},
    ext_event::create_ext_action,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, clip, dyn_stack, editor::id::EditorId, empty, label, stack, svg,
    },
};
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};
use texas_core::buffer::{
    diff::{DiffExpand, DiffLines, expand_diff_lines, rope_diff_cancellable},
    rope_text::RopeText,
};
use texas_rpc::{buffer::BufferId, proxy::ProxyResponse};

#[derive(Clone)]
pub struct DiffInfo {
    pub is_right: bool,
    pub changes: Vec<DiffLines>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiffEditorInfo {
    pub left_content: DocContent,
    pub right_content: DocContent,
}

impl DiffEditorInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> DiffEditorData {
        let cx = data.scope.create_child();

        let diff_editor_id = DiffEditorId::next();

        let new_doc = {
            let data = data.clone();
            let common = data.common.clone();
            move |content: &DocContent| match content {
                DocContent::File { path, .. } => {
                    let (doc, _) = data.get_doc(path.clone(), None);
                    doc
                }
                DocContent::Local => {
                    Rc::new(Doc::new_local(cx, data.editors, common.clone()))
                }
                DocContent::History(history) => {
                    let doc = Doc::new_history(
                        cx,
                        content.clone(),
                        data.editors,
                        common.clone(),
                    );
                    let doc = Rc::new(doc);

                    {
                        let doc = doc.clone();
                        let send = create_ext_action(cx, move |result| {
                            if let Ok(ProxyResponse::BufferHeadResponse {
                                content,
                                ..
                            }) = result
                            {
                                doc.init_content(Rope::from(content));
                            }
                        });
                        common.proxy.get_buffer_head(
                            history.path.clone(),
                            move |result| {
                                send(result);
                            },
                        );
                    }

                    doc
                }
                DocContent::Scratch { name, .. } => {
                    let doc_content = DocContent::Scratch {
                        id: BufferId::next(),
                        name: name.to_string(),
                    };
                    let doc = Doc::new_content(
                        cx,
                        doc_content,
                        data.editors,
                        common.clone(),
                    );
                    let doc = Rc::new(doc);
                    data.scratch_docs.update(|scratch_docs| {
                        scratch_docs.insert(name.to_string(), doc.clone());
                    });
                    doc
                }
            }
        };

        let left_doc = new_doc(&self.left_content);
        let right_doc = new_doc(&self.right_content);
        let diff_editor_data = DiffEditorData::new(
            cx,
            diff_editor_id,
            editor_tab_id,
            (left_doc, right_doc),
            data.editors,
            data.common.clone(),
            data.editor_tabs,
        );

        data.diff_editors.update(|diff_editors| {
            diff_editors.insert(diff_editor_id, diff_editor_data.clone());
        });

        diff_editor_data
    }
}

#[derive(Clone)]
pub struct DiffEditorData {
    pub id: DiffEditorId,
    pub editor_tab_id: RwSignal<EditorTabId>,
    pub scope: Scope,
    pub left: EditorData,
    pub right: EditorData,
    pub confirmed: RwSignal<bool>,
    pub focus_right: RwSignal<bool>,
    editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    selected: Memo<bool>,
}

pub(crate) fn selected_diff_memo(
    cx: Scope,
    editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    editor_tab_id: EditorTabId,
    diff_editor_id: DiffEditorId,
) -> Memo<bool> {
    cx.create_memo(move |_| {
        editor_tabs.with(|editor_tabs| {
            let Some(editor_tab) = editor_tabs.get(&editor_tab_id) else {
                return false;
            };
            editor_tab.with(|editor_tab| {
                matches!(
                    editor_tab
                        .children
                        .get(editor_tab.active)
                        .map(|(_, _, child)| child),
                    Some(EditorTabChild::DiffEditor(id)) if *id == diff_editor_id
                )
            })
        })
    })
}

impl DiffEditorData {
    pub fn new(
        cx: Scope,
        id: DiffEditorId,
        editor_tab_id: EditorTabId,
        docs: (Rc<Doc>, Rc<Doc>),
        editors: Editors,
        common: Rc<CommonData>,
        editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    ) -> Self {
        let cx = cx.create_child();
        let confirmed = cx.create_rw_signal(false);
        let selected = selected_diff_memo(cx, editor_tabs, editor_tab_id, id);

        // TODO: ensure that left/right are cleaned up
        let [left, right] = [docs.0, docs.1].map(|doc| {
            editors.make_from_doc(
                cx,
                doc,
                None,
                Some((editor_tab_id, id)),
                Some(confirmed),
                common.clone(),
            )
        });

        // Diff editors must not render their documents as normal editors while the
        // asynchronous diff is being computed. The normal screen-line path may scan
        // the entire document to determine wrapped-line counts.
        left.kind.set(EditorViewKind::Diff(DiffInfo {
            is_right: false,
            changes: Vec::new(),
        }));
        right.kind.set(EditorViewKind::Diff(DiffInfo {
            is_right: true,
            changes: Vec::new(),
        }));

        let data = Self {
            id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            scope: cx,
            left,
            right,
            confirmed,
            focus_right: cx.create_rw_signal(true),
            editor_tabs,
            selected,
        };

        data.listen_diff_changes();

        data
    }

    pub fn diff_editor_info(&self) -> DiffEditorInfo {
        DiffEditorInfo {
            left_content: self.left.doc().content.get_untracked(),
            right_content: self.right.doc().content.get_untracked(),
        }
    }

    pub fn copy(
        &self,
        cx: Scope,
        editor_tab_id: EditorTabId,
        diff_editor_id: EditorId,
        editors: Editors,
    ) -> Self {
        let cx = cx.create_child();
        let confirmed = cx.create_rw_signal(true);
        let selected =
            selected_diff_memo(cx, self.editor_tabs, editor_tab_id, diff_editor_id);

        let [left, right] = [&self.left, &self.right].map(|editor_data| {
            editors
                .make_copy(
                    editor_data.id(),
                    cx,
                    None,
                    Some((editor_tab_id, diff_editor_id)),
                    Some(confirmed),
                )
                .unwrap()
        });

        left.kind.set(EditorViewKind::Diff(DiffInfo {
            is_right: false,
            changes: Vec::new(),
        }));
        right.kind.set(EditorViewKind::Diff(DiffInfo {
            is_right: true,
            changes: Vec::new(),
        }));

        let diff_editor = DiffEditorData {
            scope: cx,
            id: diff_editor_id,
            editor_tab_id: cx.create_rw_signal(editor_tab_id),
            focus_right: cx.create_rw_signal(true),
            left,
            right,
            confirmed,
            editor_tabs: self.editor_tabs,
            selected,
        };

        diff_editor.listen_diff_changes();
        diff_editor
    }

    fn listen_diff_changes(&self) {
        let cx = self.scope;
        let active_diff = Arc::new(Mutex::new(None::<Arc<AtomicBool>>));
        let selected = self.selected;

        let left = self.left.clone();
        let left_doc_rev = {
            let left = left.clone();
            cx.create_memo(move |_| {
                let doc = left.doc_signal().get();
                (doc.content.get(), doc.buffer.with(|b| b.rev()))
            })
        };

        let right = self.right.clone();
        let right_doc_rev = {
            let right = right.clone();
            cx.create_memo(move |_| {
                let doc = right.doc_signal().get();
                (doc.content.get(), doc.buffer.with(|b| b.rev()))
            })
        };

        cx.create_effect(move |_| {
            if !selected.get() {
                if let Ok(mut active) = active_diff.lock() {
                    if let Some(previous) = active.take() {
                        previous.store(true, atomic::Ordering::Release);
                    }
                }
                return;
            }

            let (_, left_rev) = left_doc_rev.get();
            let (left_editor_view, left_doc) = (left.kind, left.doc());
            let (left_atomic_rev, left_rope) =
                left_doc.buffer.with_untracked(|buffer| {
                    (buffer.atomic_rev(), buffer.text().clone())
                });

            let (_, right_rev) = right_doc_rev.get();
            let (right_editor_view, right_doc) = (right.kind, right.doc());
            let (right_atomic_rev, right_rope) =
                right_doc.buffer.with_untracked(|buffer| {
                    (buffer.atomic_rev(), buffer.text().clone())
                });

            let cancelled = Arc::new(AtomicBool::new(false));
            if let Ok(mut active) = active_diff.lock() {
                if let Some(previous) = active.replace(cancelled.clone()) {
                    previous.store(true, atomic::Ordering::Release);
                }
            }

            let send = {
                let right_atomic_rev = right_atomic_rev.clone();
                let left_atomic_rev = left_atomic_rev.clone();
                let cancelled = cancelled.clone();
                create_ext_action(cx, move |changes: Option<Vec<DiffLines>>| {
                    let changes = if let Some(changes) = changes {
                        changes
                    } else {
                        return;
                    };

                    if !selected.get_untracked() {
                        return;
                    }
                    if cancelled.load(atomic::Ordering::Acquire) {
                        return;
                    }
                    if left_atomic_rev.load(atomic::Ordering::Acquire) != left_rev {
                        return;
                    }

                    if right_atomic_rev.load(atomic::Ordering::Acquire) != right_rev
                    {
                        return;
                    }

                    left_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: false,
                        changes: changes.clone(),
                    }));
                    right_editor_view.set(EditorViewKind::Diff(DiffInfo {
                        is_right: true,
                        changes,
                    }));
                })
            };

            rayon::spawn(move || {
                let changes = rope_diff_cancellable(
                    left_rope,
                    right_rope,
                    right_rev,
                    right_atomic_rev.clone(),
                    cancelled,
                    Some(3),
                );
                send(changes);
            });
        });
    }
}

#[derive(Clone, PartialEq)]
struct DiffShowMoreSection {
    left_actual_line: usize,
    right_actual_line: usize,
    skip_start: usize,
    lines: usize,
}

pub fn diff_show_more_section_view(
    left_editor: &EditorData,
    right_editor: &EditorData,
) -> impl View + use<> {
    let left_editor_view = left_editor.kind;
    let right_editor_view = right_editor.kind;
    let right_screen_lines = right_editor.screen_lines();
    let right_scroll_delta = right_editor.editor.scroll_delta;
    let viewport = right_editor.viewport();
    let config = right_editor.common.config;
    let i18n = right_editor.common.i18n.clone();

    let each_fn = move || {
        let editor_view = right_editor_view.get();

        if let EditorViewKind::Diff(diff_info) = editor_view {
            diff_info
                .changes
                .iter()
                .filter_map(|change| {
                    let DiffLines::Both(info) = change else {
                        return None;
                    };

                    let skip = info.skip.as_ref()?;

                    Some(DiffShowMoreSection {
                        left_actual_line: info.left.start,
                        right_actual_line: info.right.start,
                        skip_start: skip.start,
                        lines: skip.len(),
                    })
                })
                .collect()
        } else {
            Vec::new()
        }
    };

    let key_fn = move |section: &DiffShowMoreSection| {
        (
            section.right_actual_line + section.skip_start,
            section.lines,
        )
    };

    let view_i18n = i18n.clone();
    let view_fn = move |section: DiffShowMoreSection| {
        let i18n = view_i18n.clone();
        stack((
            label({
                let hidden_i18n = i18n.clone();
                move || {
                    format!(
                        "{} {}",
                        section.lines,
                        hidden_i18n.text("common.hidden-lines")
                    )
                }
            })
            .style(move |s| {
                let config = config.get();
                s.padding_horiz(8.0)
                    .color(config.color(TexasColor::PANEL_FOREGROUND_DIM))
                    .selectable(false)
            }),
            stack((
                svg(move || config.get().ui_svg(TexasIcons::FOLD)).style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(config.color(TexasColor::EDITOR_FOREGROUND))
                }),
                label(i18n.text_signal("common.expand-all"))
                    .style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::All,
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::All,
                            true,
                        );
                    }
                });
            })
            .style(move |s| {
                let hover_background =
                    config.get().color(TexasColor::PANEL_HOVERED_BACKGROUND);
                s.margin_left(10.0)
                    .padding_horiz(6.0)
                    .height_pct(100.0)
                    .items_center()
                    .border_radius(6.0)
                    .hover(move |s| {
                        s.cursor(CursorStyle::Pointer).background(hover_background)
                    })
            }),
            stack((
                svg(move || config.get().ui_svg(TexasIcons::FOLD_UP)).style(
                    move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .color(config.color(TexasColor::EDITOR_FOREGROUND))
                    },
                ),
                label(i18n.text_signal("common.expand-up"))
                    .style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::Up(10),
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::Up(10),
                            true,
                        );
                    }
                });
            })
            .style(move |s| {
                let hover_background =
                    config.get().color(TexasColor::PANEL_HOVERED_BACKGROUND);
                s.margin_left(10.0)
                    .padding_horiz(6.0)
                    .height_pct(100.0)
                    .items_center()
                    .border_radius(6.0)
                    .hover(move |s| {
                        s.cursor(CursorStyle::Pointer).background(hover_background)
                    })
            }),
            stack((
                svg(move || config.get().ui_svg(TexasIcons::FOLD_DOWN)).style(
                    move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .color(config.color(TexasColor::EDITOR_FOREGROUND))
                    },
                ),
                label(i18n.text_signal("common.expand-down"))
                    .style(|s| s.margin_left(6.0)),
            ))
            .on_event_stop(EventListener::PointerDown, move |_| {})
            .on_click_stop(move |_event| {
                left_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.left_actual_line,
                            DiffExpand::Down(10),
                            false,
                        );
                    }
                });
                right_editor_view.update(|editor_view| {
                    if let EditorViewKind::Diff(diff_info) = editor_view {
                        expand_diff_lines(
                            &mut diff_info.changes,
                            section.right_actual_line,
                            DiffExpand::Down(10),
                            true,
                        );
                    }
                });
            })
            .style(move |s| {
                let hover_background =
                    config.get().color(TexasColor::PANEL_HOVERED_BACKGROUND);
                s.margin_left(10.0)
                    .padding_horiz(6.0)
                    .height_pct(100.0)
                    .items_center()
                    .border_radius(6.0)
                    .hover(move |s| {
                        s.cursor(CursorStyle::Pointer).background(hover_background)
                    })
            }),
        ))
        .on_event_cont(EventListener::PointerWheel, move |event| {
            if let Event::PointerWheel(event) = event {
                right_scroll_delta.set(event.delta);
            }
        })
        .style(move |s| {
            let right_screen_lines = right_screen_lines.get();

            let mut right_line = section.right_actual_line + section.skip_start;
            let is_before_skip = if right_line > 0 {
                right_line -= 1;
                true
            } else {
                right_line += section.lines;
                false
            };

            let Some(line_info) = right_screen_lines.info_for_line(right_line)
            else {
                return s.hide();
            };

            let config = config.get();
            let line_height = config.editor.line_height();

            let mut y = line_info.y - viewport.get().y0;

            if is_before_skip {
                y += line_height as f64
            } else {
                y -= line_height as f64
            }

            s.absolute()
                .width_pct(100.0)
                .height(line_height as f32)
                .justify_center()
                .items_center()
                .background(config.color(TexasColor::EDITOR_BACKGROUND))
                .border_top(1.0)
                .border_bottom(1.0)
                .border_color(config.color(TexasColor::TEXAS_BORDER))
                .margin_top(y)
                .pointer_events_auto()
                .hover(|s| s.cursor(CursorStyle::Default))
        })
    };

    stack((
        empty().style(move |s| {
            s.height(config.get().editor.line_height() as f32 + 1.0)
        }),
        clip(
            dyn_stack(each_fn, key_fn, view_fn)
                .style(|s| s.flex_col().size_pct(100.0, 100.0)),
        )
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .style(|s| {
        s.absolute()
            .flex_col()
            .size_pct(100.0, 100.0)
            .pointer_events_none()
    })
    .debug_name("Diff Show More Section")
}

#[cfg(test)]
mod tests {
    use floem::{
        peniko::kurbo::{Point, Rect},
        reactive::{Scope, SignalGet, SignalUpdate},
    };

    use super::selected_diff_memo;
    use crate::{
        editor_tab::{EditorTabChild, EditorTabData},
        id::{DiffEditorId, EditorTabId, SplitId},
    };

    #[test]
    fn selected_diff_memo_tracks_the_active_child() {
        let cx = Scope::new();
        let editor_tab_id = EditorTabId::next();
        let first_diff_id = DiffEditorId::next();
        let second_diff_id = DiffEditorId::next();
        let editor_tab = cx.create_rw_signal(EditorTabData {
            scope: cx,
            split: SplitId::next(),
            editor_tab_id,
            active: 0,
            children: [first_diff_id, second_diff_id]
                .into_iter()
                .map(|id| {
                    (
                        cx.create_rw_signal(0),
                        cx.create_rw_signal(Rect::ZERO),
                        EditorTabChild::DiffEditor(id),
                    )
                })
                .collect(),
            window_origin: Point::ZERO,
            layout_rect: Rect::ZERO,
            locations: cx.create_rw_signal(im::Vector::new()),
            current_location: cx.create_rw_signal(0),
        });
        let editor_tabs = cx.create_rw_signal(im::HashMap::new());
        editor_tabs.update(|tabs| {
            tabs.insert(editor_tab_id, editor_tab);
        });

        let selected =
            selected_diff_memo(cx, editor_tabs, editor_tab_id, first_diff_id);
        assert!(selected.get());

        editor_tab.update(|tab| tab.active = 1);
        assert!(!selected.get());
    }
}
