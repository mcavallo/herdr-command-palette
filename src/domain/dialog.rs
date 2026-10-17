//! What the confirmation dialog says for a row the settings' `confirm_ids`
//! lists, or that must ask (Herdr's forced close asks): a title and, for
//! closes, a detail on two lines, label then counts.
//!
//! Texts copy Herdr's own close dialog (`open_close_confirmation` and
//! `request_tab_close` in `src/client/shell/overlay_input.rs`, v0.9.3):
//!
//! - `close_workspace`: `Close workspace?` / `<label>` / `N panes`, or
//!   `Close worktree group?` / `<label>` / `W workspaces, N panes` when
//!   the close takes the workspace's worktree group with it. Herdr puts
//!   label and counts on one line (`<label> — N panes`); the palette
//!   gives each its own, so a long label is cut without losing the
//!   counts.
//! - `close_tab` on its workspace's last tab: the same dialog for that
//!   workspace. On any other tab: `Close tab?`, no detail.
//! - `close_pane` on the only pane of a worktree group's workspace: the
//!   group's dialog (Herdr asks after `pane.close` answers
//!   `confirmation_required`). On any other pane: as any other row.
//! - Any other row: `<row title>?`, no detail.
//!
//! Without a snapshot, or when the target isn't in it, there is no
//! detail: the title alone.
//!
//! The title is a [`DialogLine`]: the row title that may be cut with `…`,
//! then the suffix (`?`) that always shows. The detail is a
//! [`DialogDetail`]: the label and the counts.
//!
//! Drawing the dialog is [`layout::dialog`](crate::domain::layout::dialog()).

use crate::domain::commands::Builtin;
use crate::domain::config::UiConfig;
use crate::domain::herdr::{InvocationContext, Snapshot, Workspace};
use crate::domain::requests;
use crate::domain::row::{Kind, Row};

/// Title for a workspace close (Herdr's).
const CLOSE_WORKSPACE: &str = "Close workspace?";

/// Title for a workspace close that also closes its linked worktree
/// workspaces (Herdr's).
const CLOSE_WORKTREE_GROUP: &str = "Close worktree group?";

/// Title for closing a tab that isn't its workspace's last one.
const CLOSE_TAB: &str = "Close tab?";

/// What the dialog says: a title and an optional detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogText {
    /// The question, e.g. `Close workspace?`.
    pub title: DialogLine,
    /// What the close takes with it, e.g. `api` / `3 panes`; `None` when
    /// there is nothing to add or nothing known.
    pub detail: Option<DialogDetail>,
}

/// What a close takes with it, one line each: the workspace's label and
/// the counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogDetail {
    /// The workspace's label, e.g. `api`.
    pub label: String,
    /// What closes, e.g. `3 panes` or `2 workspaces, 5 panes`.
    pub counts: String,
}

impl DialogDetail {
    /// A detail of `label` and `counts`.
    #[must_use]
    pub fn new(label: impl Into<String>, counts: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            counts: counts.into(),
        }
    }
}

impl DialogText {
    /// A dialog with only a title.
    fn title_only(title: DialogLine) -> Self {
        Self {
            title,
            detail: None,
        }
    }
}

/// The dialog's title line: [`text`](Self::text), cut with `…` when the
/// line is too long, then [`suffix`](Self::suffix), which always shows
/// so the dialog still reads as a question: `<row title…>?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogLine {
    /// The part that may be cut: a row title, or the whole line.
    pub text: String,
    /// What follows it, never cut while it fits: `?`, or nothing.
    pub suffix: String,
}

impl DialogLine {
    /// A line with no suffix.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            suffix: String::new(),
        }
    }

    /// The same line ending with `suffix`.
    #[must_use]
    pub fn with_suffix(self, suffix: impl Into<String>) -> Self {
        Self {
            suffix: suffix.into(),
            ..self
        }
    }
}

/// The dialog text for `row`, with targets from `context` and `snapshot`
/// as running the row would pick them ([`requests::targets`]).
/// `snapshot` is `None` when it couldn't be read.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::dialog::{self, DialogLine, DialogText};
/// use herdr_command_palette::domain::herdr::InvocationContext;
/// use herdr_command_palette::domain::row::{Kind, Row};
///
/// let row = Row::new(Kind::Builtin, "zoom", "Zoom pane");
///
/// assert_eq!(
///     dialog::text(&row, &InvocationContext::default(), None),
///     DialogText { title: DialogLine::new("Zoom pane").with_suffix("?"), detail: None }
/// );
/// ```
#[must_use]
pub fn text(row: &Row, context: &InvocationContext, snapshot: Option<&Snapshot>) -> DialogText {
    let by_title = || DialogText::title_only(DialogLine::new(row.full_title()).with_suffix("?"));
    match builtin(row) {
        Some(Builtin::CloseWorkspace) => close_workspace(context, snapshot),
        Some(Builtin::CloseTab) => close_tab(context, snapshot),
        Some(Builtin::ClosePane) => close_pane(context, snapshot).unwrap_or_else(by_title),
        _ => by_title(),
    }
}

/// The built-in `row` runs, if it is one.
fn builtin(row: &Row) -> Option<Builtin> {
    match row.kind {
        Kind::Builtin => row.payload.parse().ok(),
        _ => None,
    }
}

/// Whether `row` must ask even when the settings' `confirm_ids` doesn't list
/// it, with Herdr's `confirm_close` on:
///
/// - `close_tab` on its workspace's last tab: Herdr's own keybind asks
///   there (`request_tab_close`).
/// - `close_pane` on the only pane of a worktree group's workspace:
///   Herdr refuses that `pane.close` unasked (`confirmation_required`)
///   and asks itself. (On a plain workspace's only pane Herdr's keybind
///   doesn't ask, so neither does the palette.)
///
/// `false` without a snapshot.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::config::UiConfig;
/// use herdr_command_palette::domain::dialog;
/// use herdr_command_palette::domain::herdr::InvocationContext;
/// use herdr_command_palette::domain::row::{Kind, Row};
///
/// let row = Row::new(Kind::Builtin, "close_tab", "Close tab");
///
/// assert!(!dialog::is_required(&row, &InvocationContext::default(), None, UiConfig::default()));
/// ```
#[must_use]
pub fn is_required(
    row: &Row,
    context: &InvocationContext,
    snapshot: Option<&Snapshot>,
    ui: UiConfig,
) -> bool {
    let (Some(builtin), Some(snapshot)) = (builtin(row), snapshot) else {
        return false;
    };
    let targets = requests::targets(context, snapshot);
    match builtin {
        Builtin::CloseTab => {
            ui.confirm_close()
                && targets
                    .tab_id
                    .and_then(|tab_id| snapshot.workspace_closed_by_tab(tab_id))
                    .is_some()
        }
        _ => requests::group_closed_by(builtin, snapshot, targets, ui).is_some(),
    }
}

/// `close_workspace`: the target workspace's dialog.
fn close_workspace(context: &InvocationContext, snapshot: Option<&Snapshot>) -> DialogText {
    let found = snapshot.and_then(|snapshot| {
        let id = requests::targets(context, snapshot).workspace_id?;
        let workspace = snapshot.workspaces.iter().find(|w| w.workspace_id == id)?;
        Some(workspace_dialog(snapshot, workspace))
    });
    found.unwrap_or_else(|| DialogText::title_only(DialogLine::new(CLOSE_WORKSPACE)))
}

/// `close_tab`: the workspace's dialog when the target tab is its last
/// one (closing it closes the workspace), else `Close tab?`.
fn close_tab(context: &InvocationContext, snapshot: Option<&Snapshot>) -> DialogText {
    let found = snapshot.and_then(|snapshot| {
        let id = requests::targets(context, snapshot).tab_id?;
        let workspace = snapshot.workspace_closed_by_tab(id)?;
        Some(workspace_dialog(snapshot, workspace))
    });
    found.unwrap_or_else(|| DialogText::title_only(DialogLine::new(CLOSE_TAB)))
}

/// `close_pane`: the worktree group's dialog when the target pane is the
/// only one of a group's workspace (closing it closes the group); `None`
/// otherwise, for the row title.
///
/// Independent of `[ui] confirm_close`: with it off, the plain
/// `pane.close` that [`requests::group_closed_by`] leads to still closes
/// the whole group (Herdr `close_selected_workspace`, `src/app/actions.rs`,
/// v0.9.3), so the dialog says what happens either way.
fn close_pane(context: &InvocationContext, snapshot: Option<&Snapshot>) -> Option<DialogText> {
    let snapshot = snapshot?;
    let id = requests::targets(context, snapshot).pane_id?;
    let workspace = snapshot.group_closed_by_pane(id)?;
    Some(workspace_dialog(snapshot, workspace))
}

/// The dialog for closing `workspace`: alone, or with its worktree group
/// when it is a main checkout with linked worktree workspaces open
/// (Herdr's `open_close_confirmation`).
fn workspace_dialog(snapshot: &Snapshot, workspace: &Workspace) -> DialogText {
    let group = snapshot.close_group(workspace);
    let pane_count = snapshot
        .panes
        .iter()
        .filter(|pane| {
            group
                .iter()
                .any(|member| member.workspace_id == pane.workspace_id)
        })
        .count();
    let panes = if pane_count == 1 {
        "1 pane".to_owned()
    } else {
        format!("{pane_count} panes")
    };
    let closes_group = group.len() > 1;
    let (title, counts) = if closes_group {
        (
            CLOSE_WORKTREE_GROUP,
            format!("{} workspaces, {panes}", group.len()),
        )
    } else {
        (CLOSE_WORKSPACE, panes)
    };
    DialogText {
        title: DialogLine::new(title),
        detail: Some(DialogDetail::new(&workspace.label, counts)),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::fixtures::{
        goto_snapshot, group_with_one_pane, in_tab, on_pane, with_second_tab, worktree_snapshot,
    };

    use super::*;

    fn close_workspace_row() -> Row {
        Row::new(Kind::Builtin, "close_workspace", "Close workspace")
    }

    fn close_tab_row() -> Row {
        Row::new(Kind::Builtin, "close_tab", "Close tab")
    }

    fn in_workspace(workspace_id: &str) -> InvocationContext {
        InvocationContext {
            workspace_id: Some(workspace_id.into()),
            ..InvocationContext::default()
        }
    }

    /// A close dialog: `title`, then `label` and `counts`.
    fn dialog(title: &str, label: &str, counts: &str) -> DialogText {
        DialogText {
            title: DialogLine::new(title),
            detail: Some(DialogDetail::new(label, counts)),
        }
    }

    fn title_only(title: &str) -> DialogText {
        DialogText::title_only(DialogLine::new(title))
    }

    #[test]
    fn close_workspace_counts_its_panes() {
        let snapshot = goto_snapshot();

        let text = text(&close_workspace_row(), &in_workspace("w1"), Some(&snapshot));

        assert_eq!(text, dialog("Close workspace?", "dotfiles", "3 panes"));
    }

    #[test]
    fn close_workspace_says_one_pane_in_the_singular() {
        let snapshot = worktree_snapshot();

        let text = text(&close_workspace_row(), &in_workspace("w3"), Some(&snapshot));

        assert_eq!(text, dialog("Close workspace?", "notes", "1 pane"));
    }

    #[test]
    fn close_workspace_targets_the_focused_workspace_without_a_context() {
        let mut snapshot = goto_snapshot();
        snapshot.focused_workspace_id = Some("w2".into());

        let text = text(
            &close_workspace_row(),
            &InvocationContext::default(),
            Some(&snapshot),
        );

        assert_eq!(text, dialog("Close workspace?", "api", "2 panes"));
    }

    #[test]
    fn close_workspace_of_a_main_checkout_closes_its_worktree_group() {
        let snapshot = worktree_snapshot();

        let text = text(&close_workspace_row(), &in_workspace("w1"), Some(&snapshot));

        assert_eq!(
            text,
            dialog("Close worktree group?", "repo", "2 workspaces, 3 panes")
        );
    }

    #[test]
    fn close_workspace_of_a_linked_worktree_closes_only_it() {
        let snapshot = worktree_snapshot();

        let text = text(&close_workspace_row(), &in_workspace("w2"), Some(&snapshot));

        assert_eq!(text, dialog("Close workspace?", "repo-fix", "1 pane"));
    }

    #[test]
    fn close_workspace_of_a_main_checkout_alone_is_not_a_group() {
        let mut snapshot = worktree_snapshot();
        snapshot.workspaces.retain(|w| w.workspace_id != "w2");

        let text = text(&close_workspace_row(), &in_workspace("w1"), Some(&snapshot));

        assert_eq!(text, dialog("Close workspace?", "repo", "2 panes"));
    }

    #[test]
    fn close_workspace_without_a_snapshot_is_the_title_alone() {
        let text = text(&close_workspace_row(), &in_workspace("w1"), None);

        assert_eq!(text, title_only("Close workspace?"));
    }

    #[test]
    fn close_workspace_of_an_unknown_or_no_workspace_is_the_title_alone() {
        let snapshot = goto_snapshot();

        let unknown = text(&close_workspace_row(), &in_workspace("w9"), Some(&snapshot));
        let none = text(
            &close_workspace_row(),
            &InvocationContext::default(),
            Some(&snapshot),
        );

        assert_eq!(unknown, title_only("Close workspace?"));
        assert_eq!(none, title_only("Close workspace?"));
    }

    #[test]
    fn close_tab_on_the_last_tab_asks_about_its_workspace() {
        let snapshot = goto_snapshot();

        let text = text(&close_tab_row(), &in_tab("w1:t1"), Some(&snapshot));

        assert_eq!(text, dialog("Close workspace?", "dotfiles", "3 panes"));
    }

    #[test]
    fn close_tab_on_the_last_tab_of_a_main_checkout_asks_about_the_group() {
        let snapshot = worktree_snapshot();

        let text = text(&close_tab_row(), &in_tab("w1:t1"), Some(&snapshot));

        assert_eq!(
            text,
            dialog("Close worktree group?", "repo", "2 workspaces, 3 panes")
        );
    }

    #[test]
    fn close_tab_on_another_tab_asks_only_about_the_tab() {
        let snapshot = goto_snapshot();

        let text = text(&close_tab_row(), &in_tab("w2:t2"), Some(&snapshot));

        assert_eq!(text, title_only("Close tab?"));
    }

    #[test]
    fn close_tab_targets_the_focused_tab_without_a_context() {
        let mut snapshot = goto_snapshot();
        snapshot.focused_tab_id = Some("w1:t1".into());

        let text = text(
            &close_tab_row(),
            &InvocationContext::default(),
            Some(&snapshot),
        );

        assert_eq!(text, dialog("Close workspace?", "dotfiles", "3 panes"));
    }

    #[test]
    fn close_tab_without_a_snapshot_or_a_known_tab_is_the_title_alone() {
        let mut snapshot = goto_snapshot();

        let none = text(&close_tab_row(), &in_tab("w1:t1"), None);
        let unknown = text(&close_tab_row(), &in_tab("w9:t1"), Some(&snapshot));
        snapshot.workspaces.clear();
        let orphan = text(&close_tab_row(), &in_tab("w1:t1"), Some(&snapshot));

        assert_eq!(none, title_only("Close tab?"));
        assert_eq!(unknown, title_only("Close tab?"));
        assert_eq!(orphan, title_only("Close tab?"));
    }

    #[test]
    fn close_tab_on_a_workspaces_last_tab_must_ask_with_confirm_close() {
        let snapshot = with_second_tab(worktree_snapshot(), "w3");
        let on = UiConfig::default();
        let off = UiConfig::with_confirm_close(false);
        let required =
            |row: &Row, tab: &str, ui| is_required(row, &in_tab(tab), Some(&snapshot), ui);

        // A worktree group's, a linked worktree's last tab.
        assert!(required(&close_tab_row(), "w1:t1", on));
        assert!(required(&close_tab_row(), "w2:t1", on));
        assert!(!required(&close_tab_row(), "w1:t1", off));
        assert!(!required(&close_tab_row(), "w2:t1", off));
        // Not the workspace's last tab.
        assert!(!required(&close_tab_row(), "w3:t1", on));
        // Other rows.
        assert!(!required(&close_workspace_row(), "w1:t1", on));
        assert!(!required(
            &Row::new(Kind::Plugin, "close_tab", "x"),
            "w1:t1",
            on
        ));
        assert!(!is_required(&close_tab_row(), &in_tab("w1:t1"), None, on));
    }

    fn close_pane_row() -> Row {
        Row::new(Kind::Builtin, "close_pane", "Close pane")
    }

    #[test]
    fn close_pane_on_a_worktree_groups_only_pane_asks_about_the_group() {
        let snapshot = group_with_one_pane();

        let text = text(&close_pane_row(), &on_pane("w1:p1"), Some(&snapshot));

        assert_eq!(
            text,
            dialog("Close worktree group?", "repo", "2 workspaces, 2 panes")
        );
    }

    #[test]
    fn close_pane_on_other_panes_asks_with_its_title() {
        let snapshot = worktree_snapshot();
        let by_title = title_only_with_suffix("Close pane");

        // One of two panes, a plain workspace's only pane, an unknown pane.
        for pane in ["w1:p1", "w3:p1", "w9:p1"] {
            assert_eq!(
                text(&close_pane_row(), &on_pane(pane), Some(&snapshot)),
                by_title
            );
        }
        assert_eq!(text(&close_pane_row(), &on_pane("w1:p1"), None), by_title);
    }

    #[test]
    fn close_pane_on_a_worktree_groups_only_pane_must_ask_with_confirm_close() {
        let snapshot = group_with_one_pane();
        let required =
            |pane: &str, ui| is_required(&close_pane_row(), &on_pane(pane), Some(&snapshot), ui);

        assert!(required("w1:p1", UiConfig::default()));
        assert!(!required("w1:p1", UiConfig::with_confirm_close(false)));
        // A linked worktree's or a plain workspace's only pane.
        assert!(!required("w2:p1", UiConfig::default()));
        assert!(!required("w3:p1", UiConfig::default()));
        assert!(!required("w9:p1", UiConfig::default()));
    }

    fn title_only_with_suffix(title: &str) -> DialogText {
        DialogText::title_only(DialogLine::new(title).with_suffix("?"))
    }

    #[test]
    fn other_rows_ask_with_their_title() {
        let snapshot = goto_snapshot();
        let custom = Row::new(Kind::Custom, "0", "Deploy");
        let plugin = Row::new(Kind::Plugin, "close_workspace", "Close workspace (plugin)");

        assert_eq!(
            text(&custom, &InvocationContext::default(), Some(&snapshot)),
            DialogText::title_only(DialogLine::new("Deploy").with_suffix("?"))
        );
        assert_eq!(
            text(&plugin, &in_workspace("w1"), Some(&snapshot)),
            DialogText::title_only(DialogLine::new("Close workspace (plugin)").with_suffix("?"))
        );
    }
}
