//! The composer's name field: a single-line text input over a quick-add [`Draft`], drawing recognised
//! projects, dates and reminders as inline blocks, with `#project` autocomplete.
//!
//! Adapted from GPUI's `examples/input.rs`. Editing goes through the draft, so backspacing over a block turns
//! it back into literal text instead of deleting a character.

use std::ops::Range;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ZonedDateTime;
use ostracon_core::dates::DateParser;
use ostracon_core::quickadd::{self, BlockValue, Draft};
use ostracon_core::{Project, ProjectId};

use super::theme::{self, TASK_TEXT};

gpui_kit::actions!(
    quick_add,
    [
        Backspace,
        DeleteForward,
        DeleteWordBackward,
        DeleteToStart,
        Left,
        Right,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
        SuggestionUp,
        SuggestionDown,
        AcceptSuggestion,
    ]
);

const CONTEXT: &str = "QuickAdd";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", DeleteForward, c),
        KeyBinding::new("alt-backspace", DeleteWordBackward, c),
        KeyBinding::new("cmd-backspace", DeleteToStart, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-left", Home, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("cmd-right", End, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("cmd-shift-left", SelectHome, c),
        KeyBinding::new("cmd-shift-right", SelectEnd, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("escape", Cancel, c),
        KeyBinding::new("up", SuggestionUp, c),
        KeyBinding::new("down", SuggestionDown, c),
        KeyBinding::new("tab", AcceptSuggestion, c),
    ]);
}

pub enum QuickAddEvent {
    Changed,
    Submit,
    Cancel,
}

impl EventEmitter<QuickAddEvent> for QuickAddInput {}

/// Project autocomplete while a `#query` is being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Autocomplete {
    query_range: Range<usize>,
    suggestions: Vec<ProjectId>,
    highlighted: usize,
}

pub struct QuickAddInput {
    focus: FocusHandle,
    draft: Draft,
    projects: Vec<Project>,
    clock: fn() -> ZonedDateTime,
    placeholder: SharedString,
    selection: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    autocomplete: Option<Autocomplete>,
    /// Esc dismissed suggestions for the query starting here; typing elsewhere brings them back.
    dismissed_query: Option<usize>,
    /// Whether recognised words become blocks. Off for fields like a subtask name.
    recognise: bool,
    text_size: Pixels,
    line_height: Pixels,
    scroll_x: Pixels,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
}

impl QuickAddInput {
    pub fn new(
        draft: Draft,
        projects: Vec<Project>,
        placeholder: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let end = draft.len();
        QuickAddInput {
            focus: cx.focus_handle(),
            draft,
            projects,
            clock: ZonedDateTime::now,
            placeholder: placeholder.to_owned().into(),
            selection: end..end,
            reversed: false,
            marked: None,
            autocomplete: None,
            dismissed_query: None,
            recognise: true,
            text_size: px(TASK_TEXT),
            line_height: px(24.),
            scroll_x: px(0.),
            last_layout: None,
            last_bounds: None,
            selecting: false,
        }
    }

    /// Text bigger or smaller than a task's in the list, such as the overview's title.
    pub fn with_text_size(mut self, size: Pixels, line_height: Pixels) -> Self {
        self.text_size = size;
        self.line_height = line_height;
        self
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    pub fn set_draft(&mut self, draft: Draft, cx: &mut Context<Self>) {
        self.draft = draft;
        let end = self.draft.len();
        self.selection = end..end;
        self.marked = None;
        self.autocomplete = None;
        cx.emit(QuickAddEvent::Changed);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_draft(Draft::new(), cx);
    }

    pub fn set_projects(&mut self, projects: Vec<Project>) {
        self.projects = projects;
    }

    pub fn is_suggesting(&self) -> bool {
        self.autocomplete.is_some()
    }

    /// Changes the draft from outside, such as when a chip picks a date and the date block must go.
    pub fn update_draft(&mut self, change: impl FnOnce(&mut Draft), cx: &mut Context<Self>) {
        change(&mut self.draft);
        let len = self.draft.len();
        self.selection = self.selection.start.min(len)..self.selection.end.min(len);
        self.refresh(cx);
    }

    fn dates(&self) -> DateParser {
        DateParser::new((self.clock)())
    }

    fn text(&self) -> String {
        self.draft.plain_text()
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    /// Applies an edit and recognises what it finished.
    fn edit(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.draft.edit(range.clone(), text);
        let cursor = range.start + text.len();
        self.selection = cursor..cursor;
        self.reversed = false;
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.recognise {
            let dates = self.dates();
            self.draft.convert(&self.projects, Some(&dates), false);
        }
        self.update_autocomplete();
        cx.emit(QuickAddEvent::Changed);
        cx.notify();
    }

    fn update_autocomplete(&mut self) {
        let query = self
            .selection
            .is_empty()
            .then(|| self.draft.project_query(self.cursor()))
            .flatten();
        self.autocomplete = query.and_then(|(range, query)| {
            if self.dismissed_query == Some(range.start) {
                return None;
            }
            let suggestions: Vec<ProjectId> = quickadd::suggestions(&query, &self.projects)
                .into_iter()
                .map(|p| p.id)
                .take(8)
                .collect();
            if suggestions.is_empty() {
                return None;
            }
            let highlighted = self
                .autocomplete
                .as_ref()
                .filter(|a| a.query_range.start == range.start)
                .map_or(0, |a| a.highlighted.min(suggestions.len() - 1));
            Some(Autocomplete {
                query_range: range,
                suggestions,
                highlighted,
            })
        });
        if self.autocomplete.is_none()
            && query_start(&self.draft, self.cursor()) != self.dismissed_query
        {
            self.dismissed_query = None;
        }
    }

    fn accept_suggestion(&mut self, index: Option<usize>, cx: &mut Context<Self>) -> bool {
        let Some(autocomplete) = self.autocomplete.take() else {
            return false;
        };
        let id = autocomplete.suggestions[index.unwrap_or(autocomplete.highlighted)];
        let Some(project) = self.projects.iter().find(|p| p.id == id).cloned() else {
            return false;
        };
        let cursor = self.draft.complete_project(self.cursor(), &project);
        self.selection = cursor..cursor;
        self.refresh(cx);
        true
    }

    // Actions

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            let cursor = self.draft.backspace(self.cursor());
            self.selection = cursor..cursor;
            self.refresh(cx);
        } else {
            self.edit(self.selection.clone(), "", cx);
        }
    }

    fn delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            let cursor = self.draft.delete_forward(self.cursor());
            self.selection = cursor..cursor;
            self.refresh(cx);
        } else {
            self.edit(self.selection.clone(), "", cx);
        }
    }

    fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = previous_word(&self.text(), self.cursor());
        self.edit(start..self.selection.end.max(self.cursor()), "", cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(0..self.selection.end, "", cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selection.is_empty() {
            previous_char(&self.text(), self.cursor())
        } else {
            self.selection.start
        };
        self.move_to(to, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let to = if self.selection.is_empty() {
            next_char(&self.text(), self.cursor())
        } else {
            self.selection.end
        };
        self.move_to(to, cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(previous_word(&self.text(), self.cursor()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(next_word(&self.text(), self.cursor()), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_char(&self.text(), self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_char(&self.text(), self.cursor()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(previous_word(&self.text(), self.cursor()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_word(&self.text(), self.cursor()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selection = 0..self.draft.len();
        self.reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.draft.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.draft.len(), cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit(self.selection.clone(), &text.replace(['\n', '\r'], " "), cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.text()[self.selection.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.text()[self.selection.clone()].to_string(),
            ));
            self.edit(self.selection.clone(), "", cx);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if !self.accept_suggestion(None, cx) {
            cx.emit(QuickAddEvent::Submit);
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(autocomplete) = self.autocomplete.take() {
            self.dismissed_query = Some(autocomplete.query_range.start);
            cx.notify();
        } else {
            cx.emit(QuickAddEvent::Cancel);
        }
    }

    fn suggestion_up(&mut self, _: &SuggestionUp, _: &mut Window, cx: &mut Context<Self>) {
        match &mut self.autocomplete {
            Some(a) => {
                a.highlighted = a
                    .highlighted
                    .checked_sub(1)
                    .unwrap_or(a.suggestions.len() - 1)
            }
            None => cx.propagate(),
        }
        cx.notify();
    }

    fn suggestion_down(&mut self, _: &SuggestionDown, _: &mut Window, cx: &mut Context<Self>) {
        match &mut self.autocomplete {
            Some(a) => a.highlighted = (a.highlighted + 1) % a.suggestions.len(),
            None => cx.propagate(),
        }
        cx.notify();
    }

    fn accept(&mut self, _: &AcceptSuggestion, _: &mut Window, cx: &mut Context<Self>) {
        if !self.accept_suggestion(None, cx) {
            cx.propagate();
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selection = offset..offset;
        self.reversed = false;
        self.update_autocomplete();
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.reversed = !self.reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        self.update_autocomplete();
        cx.notify();
    }

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if self.draft.is_empty() {
            return 0;
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let index = self.index_for_position(event.position);
        if event.click_count >= 2 {
            let text = self.text();
            self.selection = previous_word(&text, index)..next_word(&text, index);
            self.reversed = false;
            cx.notify();
        } else if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    // UTF-16 conversion for the platform's input handler.

    fn utf16_offset(&self, offset: usize) -> usize {
        self.text()[..offset.min(self.draft.len())]
            .encode_utf16()
            .count()
    }

    fn utf8_offset(&self, offset: usize) -> usize {
        let text = self.text();
        let mut utf16 = 0;
        for (index, c) in text.char_indices() {
            if utf16 >= offset {
                return index;
            }
            utf16 += c.len_utf16();
        }
        text.len()
    }

    fn utf8_range(&self, range: &Range<usize>) -> Range<usize> {
        self.utf8_offset(range.start)..self.utf8_offset(range.end)
    }

    fn utf16_range(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_offset(range.start)..self.utf16_offset(range.end)
    }

    /// The suggestion list, drawn under the field by the composer.
    pub fn render_suggestions(
        &self,
        palette: &theme::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let autocomplete = self.autocomplete.as_ref()?;
        let items = autocomplete
            .suggestions
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let project = self.projects.iter().find(|p| p.id == *id)?;
                let highlighted = index == autocomplete.highlighted;
                Some(
                    div()
                        .id(("suggestion", index))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .cursor_pointer()
                        .when(highlighted, |d| d.bg(palette.sidebar_hover))
                        .hover(|d| d.bg(palette.sidebar_hover))
                        .child(
                            div()
                                .size(px(9.))
                                .rounded_full()
                                .bg(theme::project_color(project.color)),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .text_color(palette.text)
                                .child(project.name.clone()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.accept_suggestion(Some(index), cx);
                                cx.stop_propagation();
                            }),
                        ),
                )
            });
        Some(
            div()
                .flex()
                .flex_col()
                .p_1()
                .w(px(260.))
                .rounded_lg()
                .border_1()
                .border_color(palette.card_border)
                .bg(palette.card)
                .shadow_lg()
                .children(items)
                .into_any_element(),
        )
    }
}

fn query_start(draft: &Draft, cursor: usize) -> Option<usize> {
    draft.project_query(cursor).map(|(range, _)| range.start)
}

fn previous_char(text: &str, offset: usize) -> usize {
    text[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i)
}

fn next_char(text: &str, offset: usize) -> usize {
    text[offset..]
        .chars()
        .next()
        .map_or(offset, |c| offset + c.len_utf8())
}

fn previous_word(text: &str, offset: usize) -> usize {
    let before = &text[..offset];
    let trimmed = before.trim_end();
    trimmed.rfind(char::is_whitespace).map_or(0, |i| {
        i + trimmed[i..].chars().next().map_or(1, char::len_utf8)
    })
}

fn next_word(text: &str, offset: usize) -> usize {
    let after = &text[offset..];
    let start = after.len() - after.trim_start().len();
    after[start..]
        .find(char::is_whitespace)
        .map_or(text.len(), |i| offset + start + i)
}

impl Focusable for QuickAddInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for QuickAddInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.utf8_range(&range);
        actual.replace(self.utf16_range(&range));
        Some(self.text()[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.utf16_range(&self.selection),
            reversed: self.reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection = self.utf8_range(&range);
        self.reversed = false;
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.utf16_offset(self.draft.len()))
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.utf16_range(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.utf8_range(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.marked = None;
        self.edit(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.utf8_range(&r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        // Text being composed (IME) is held as plain text and not recognised until it is committed.
        self.draft.edit(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let start16 = self.utf16_offset(range.start);
        self.selection = match selected {
            Some(sel) => self.utf8_offset(start16 + sel.start)..self.utf8_offset(start16 + sel.end),
            None => range.start + text.len()..range.start + text.len(),
        };
        self.reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.utf8_range(&range);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start) - self.scroll_x,
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end) - self.scroll_x,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let local = bounds.localize(&point)?;
        Some(self.utf16_offset(layout.closest_index_for_x(local.x + self.scroll_x)))
    }
}

impl Render for QuickAddInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete_forward))
            .on_action(cx.listener(Self::delete_word_backward))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::suggestion_up))
            .on_action(cx.listener(Self::suggestion_down))
            .on_action(cx.listener(Self::accept))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .text_size(self.text_size)
            .line_height(self.line_height)
            .child(TextElement { input: cx.entity() })
    }
}

struct TextElement {
    input: Entity<QuickAddInput>,
}

struct Prepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    blocks: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let palette = theme::palette(cx);
        let input = self.input.read(cx);
        let text = input.text();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let base = TextRun {
            len: 0,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let (display, runs, block_ranges) = if text.is_empty() {
            let placeholder = input.placeholder.to_string();
            let run = TextRun {
                len: placeholder.len(),
                color: palette.faint,
                ..base.clone()
            };
            (placeholder, vec![run], Vec::new())
        } else {
            let mut runs = Vec::new();
            let mut block_ranges = Vec::new();
            let mut offset = 0;
            for segment in input.draft.segments() {
                let (len, color, background) = match segment {
                    quickadd::Segment::Text { text, .. } => (text.len(), style.color, None),
                    quickadd::Segment::Block(block) => {
                        let (fg, bg) = match block.value {
                            BlockValue::Project(id) => {
                                let color = input
                                    .projects
                                    .iter()
                                    .find(|p| p.id == id)
                                    .map_or(palette.accent, |p| theme::project_color(p.color));
                                (color, theme::mix(palette.page, color, 0.16))
                            }
                            BlockValue::Due(_) => (palette.today, palette.block_due),
                            BlockValue::Reminder(_) => (palette.tomorrow, palette.block_reminder),
                        };
                        (block.text.len(), fg, Some(bg))
                    }
                };
                if let Some(background) = background {
                    block_ranges.push((offset..offset + len, background));
                }
                runs.push(TextRun {
                    len,
                    color,
                    ..base.clone()
                });
                offset += len;
            }
            if let Some(marked) = &input.marked {
                // Underline text being composed.
                runs = split_runs(runs, marked.clone(), |run| {
                    run.underline = Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    });
                });
            }
            (text.clone(), runs, block_ranges)
        };

        let line = window
            .text_system()
            .shape_line(display.into(), font_size, &runs, None);
        let cursor = input.cursor();
        let cursor_x = if text.is_empty() {
            px(0.)
        } else {
            line.x_for_index(cursor)
        };
        // Keep the cursor in view.
        let width = bounds.size.width;
        let mut scroll = input.scroll_x;
        if cursor_x - scroll > width - px(4.) {
            scroll = cursor_x - width + px(4.);
        } else if cursor_x < scroll {
            scroll = cursor_x;
        }
        scroll = scroll.max(px(0.));
        let x = |index: usize| bounds.left() + line.x_for_index(index) - scroll;

        let blocks = block_ranges
            .into_iter()
            .map(|(range, color)| {
                let corners = Bounds::from_corners(
                    point(x(range.start) - px(2.), bounds.top() + px(1.)),
                    point(x(range.end) + px(2.), bounds.bottom() - px(1.)),
                );
                fill(corners, color).corner_radii(px(4.))
            })
            .collect();
        let (selection, cursor) = if input.selection.is_empty() {
            let caret = Bounds::new(
                point(bounds.left() + cursor_x - scroll, bounds.top() + px(2.)),
                size(px(1.5), bounds.size.height - px(4.)),
            );
            (None, Some(fill(caret, palette.accent)))
        } else {
            let selected = Bounds::from_corners(
                point(x(input.selection.start), bounds.top()),
                point(x(input.selection.end), bounds.bottom()),
            );
            (Some(fill(selected, palette.accent.opacity(0.22))), None)
        };
        self.input.update(cx, |input, _| input.scroll_x = scroll);
        Prepaint {
            line: Some(line),
            cursor,
            selection,
            blocks,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        let scroll = self.input.read(cx).scroll_x;
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for block in prepaint.blocks.drain(..) {
                window.paint_quad(block);
            }
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            if let Some(line) = prepaint.line.as_ref() {
                let origin = point(bounds.origin.x - scroll, bounds.origin.y);
                let _ = line.paint(
                    origin,
                    window.line_height(),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if focus.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        let line = prepaint.line.take();
        self.input.update(cx, |input, _| {
            input.last_layout = line;
            input.last_bounds = Some(bounds);
        });
    }
}

/// Splits `runs` at `range` and applies `style` to the runs inside it.
fn split_runs(
    runs: Vec<TextRun>,
    range: Range<usize>,
    style: impl Fn(&mut TextRun),
) -> Vec<TextRun> {
    let mut result = Vec::new();
    let mut offset = 0;
    for run in runs {
        let (start, end) = (offset, offset + run.len);
        offset = end;
        let cuts = [
            start,
            range.start.clamp(start, end),
            range.end.clamp(start, end),
            end,
        ];
        for pair in cuts.windows(2) {
            if pair[1] > pair[0] {
                let mut piece = TextRun {
                    len: pair[1] - pair[0],
                    ..run.clone()
                };
                if pair[0] >= range.start && pair[1] <= range.end {
                    style(&mut piece);
                }
                result.push(piece);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{next_word, previous_word};

    #[test]
    fn word_boundaries() {
        let text = "Pay the rent";
        assert_eq!(previous_word(text, text.len()), 8);
        assert_eq!(previous_word(text, 8), 4);
        assert_eq!(previous_word(text, 3), 0);
        assert_eq!(next_word(text, 0), 3);
        assert_eq!(next_word(text, 3), 7);
    }
}
