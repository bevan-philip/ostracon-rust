//! Actions and their key bindings.

use gpui_kit::*;

gpui_kit::actions!(
    ostracon,
    [
        Quit,
        NewTask,
        NewSubtask,
        NewProject,
        ToggleShowCompleted,
        OpenOverview,
        OpenSettings,
        Undo,
        Project1,
        Project2,
        Project3,
        Project4,
        Project5,
        Project6,
        Project7,
        Project8,
        Project9,
        SelectNext,
        SelectPrevious,
        ExtendNext,
        ExtendPrevious,
        CollapseOrParent,
        Expand,
        Complete,
        Edit,
        Indent,
        Outdent,
        MoveUp,
        MoveDown,
        Delete,
        Escape,
        Confirm,
    ]
);

/// Plain keys in the list must not fire while typing in a field.
const LIST: &str = "TaskList && !QuickAdd && !Input";

pub fn bind_keys(cx: &mut App) {
    let bindings = [
        key("secondary-q", Quit, None, cx),
        key("secondary-n", NewTask, None, cx),
        key("alt-secondary-n", NewSubtask, None, cx),
        key("shift-secondary-n", NewProject, None, cx),
        key("shift-secondary-c", ToggleShowCompleted, None, cx),
        key("secondary-o", OpenOverview, None, cx),
        key("secondary-,", OpenSettings, None, cx),
        key("secondary-1", Project1, None, cx),
        key("secondary-2", Project2, None, cx),
        key("secondary-3", Project3, None, cx),
        key("secondary-4", Project4, None, cx),
        key("secondary-5", Project5, None, cx),
        key("secondary-6", Project6, None, cx),
        key("secondary-7", Project7, None, cx),
        key("secondary-8", Project8, None, cx),
        key("secondary-9", Project9, None, cx),
        key("a", NewTask, Some(LIST), cx),
        key("q", NewTask, Some(LIST), cx),
        key("shift-a", NewSubtask, Some(LIST), cx),
        key("secondary-z", Undo, Some(LIST), cx),
        key("down", SelectNext, Some(LIST), cx),
        key("j", SelectNext, Some(LIST), cx),
        key("up", SelectPrevious, Some(LIST), cx),
        key("k", SelectPrevious, Some(LIST), cx),
        key("shift-down", ExtendNext, Some(LIST), cx),
        key("shift-up", ExtendPrevious, Some(LIST), cx),
        key("left", CollapseOrParent, Some(LIST), cx),
        key("right", Expand, Some(LIST), cx),
        key("space", Complete, Some(LIST), cx),
        key("secondary-enter", Complete, Some(LIST), cx),
        key("enter", Edit, Some(LIST), cx),
        key("secondary-e", Edit, Some(LIST), cx),
        key("tab", Indent, Some(LIST), cx),
        key("secondary-]", Indent, Some(LIST), cx),
        key("shift-tab", Outdent, Some(LIST), cx),
        key("secondary-[", Outdent, Some(LIST), cx),
        key("alt-secondary-up", MoveUp, Some(LIST), cx),
        key("alt-secondary-down", MoveDown, Some(LIST), cx),
        key("backspace", Delete, Some(LIST), cx),
        key("delete", Delete, Some(LIST), cx),
        key("escape", Escape, Some(LIST), cx),
        // Modal layers: the overview steps between tasks and edits its own, dialogs confirm and cancel.
        key("escape", Escape, Some("Modal"), cx),
        key("up", SelectPrevious, Some("Overview && !Input"), cx),
        key("down", SelectNext, Some("Overview && !Input"), cx),
        key("enter", Edit, Some("Overview && !Input"), cx),
        key("secondary-e", Edit, Some("Overview && !Input"), cx),
        key("enter", Confirm, Some("Dialog && !Input"), cx),
    ];
    cx.bind_keys(bindings);
    crate::ui::quick_add::bind_keys(cx);
}

/// A key binding for the running OS. `secondary` is Cmd on macOS and Ctrl elsewhere; `KeyBinding::new` can't
/// resolve it, as it doesn't know the OS.
pub fn key(keys: &str, action: impl Action, context: Option<&str>, cx: &App) -> KeyBinding {
    let context = context.map(|c| KeyBindingContextPredicate::parse(c).unwrap().into());
    KeyBinding::load(keys, Box::new(action), context, false, None, cx).unwrap()
}

/// How a shortcut written for [`key`] reads on this OS: `shift-secondary-c` is ⇧⌘C on macOS, Ctrl+Shift+C
/// elsewhere.
pub fn shortcut(keys: &str, cx: &App) -> String {
    let stroke = Keystroke::parse_with_os(keys, cx.platform_info().os).unwrap();
    gpui_kit::component::kbd::Kbd::format(&stroke)
}
