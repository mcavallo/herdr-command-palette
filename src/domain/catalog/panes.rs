//! Jump to rows: one per pane in Herdr's order, titled with
//! the Go to name from [`jump_to`] after a workspace column.
//!
//! fzf can't show group headers, so every row starts with its workspace
//! ([`Row::title_column`], dropped by the minimal layout),
//! padded to a common width (the longest name, at most
//! `WORKSPACE_COLUMN_MAX` columns; longer names are cut with `…` and stay
//! searchable through the row's hidden search text).

use std::path::{Path, PathBuf};

use crate::domain::herdr::{Agent, Pane, Snapshot};
use crate::domain::jump_to;
use crate::domain::layout;
use crate::domain::non_empty;
use crate::domain::row::{FooterLine, Kind, Row};
use crate::domain::tags;

/// Widest the workspace column gets, in terminal columns.
const WORKSPACE_COLUMN_MAX: usize = 20;

/// One row per pane, workspace by workspace in Herdr's order.
///
/// Title: the Go to name, after the workspace column
/// ([`Row::full_title`]: `<workspace column>  <Go to name>`). Shortcut: agent kind, else
/// `terminal` (no live status: it would go stale while the popup is
/// open), left-aligned in a right column as wide as the
/// widest kind. Footer ([`Row::footer`]): the pane's location,
/// parts `[<workspace>, <tab>, <pane id>]` (see `location`), then the cwd
/// as its second line, the home directory shown as `~`.
/// Hidden search text: the full workspace name when the column cut it.
#[must_use]
pub fn rows(snapshot: &Snapshot, home: Option<&Path>) -> Vec<Row> {
    let labels = jump_to::pane_labels(snapshot);
    // Without tags, as the row will show them: the column is
    // measured on what is left, so stripping one later can't shift it.
    let workspace_names: Vec<String> = snapshot
        .workspaces
        .iter()
        .map(|workspace| tags::strip(&workspace.label))
        .collect();
    let column = workspace_names
        .iter()
        .map(|name| layout::width(name))
        .max()
        .unwrap_or(0)
        .min(WORKSPACE_COLUMN_MAX);
    // A home that isn't UTF-8 can't prefix a (UTF-8) cwd anyway.
    let home = home.and_then(Path::to_str);

    let mut rows = Vec::new();
    for (workspace, workspace_name) in snapshot.workspaces.iter().zip(&workspace_names) {
        let workspace_name = workspace_name.as_str();
        let shown = layout::fit(workspace_name, column);
        let panes = snapshot
            .panes
            .iter()
            .filter(|pane| pane.workspace_id == workspace.workspace_id);
        for pane in panes {
            let id = pane.pane_id.as_str();
            // Every pane of a listed tab has a label; fall back to the id
            // for a pane whose tab is missing from the snapshot.
            let label = labels.get(id).map_or(id, String::as_str);
            let kind = agent_kind(snapshot, pane).unwrap_or(jump_to::NO_AGENT);
            let cwd = with_tilde(pane.working_dir().unwrap_or_default(), home);
            let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == pane.tab_id);
            let tab_label = tab.map(|tab| tab.label.as_str());
            let mut row = Row::new(Kind::Pane, id, label)
                .with_title_column(shown.as_str())
                .with_shortcut(kind)
                .with_footer(vec![
                    location(workspace_name, tab_label, id),
                    FooterLine::from_text(cwd),
                ]);
            if layout::width(workspace_name) > column {
                row = row.with_search_extra(workspace_name);
            }
            rows.push(row);
        }
    }
    // One right column for every row: names are cut at the
    // same column whatever the row's agent kind, and the kinds start
    // there, so the gap after every name is the same.
    let kinds = rows
        .iter()
        .map(|row| layout::width(&row.shortcut))
        .max()
        .unwrap_or(0);
    for row in &mut rows {
        row.right_width = kinds;
    }
    rows
}

/// Where a pane lives, Jump to's footer line 1: the parts `[<workspace>,
/// <tab label>, <pane id>]`, shown as `api / logs / w1:p3`. The tab
/// label is the one Herdr has (an unnamed tab's is its
/// number); a tab missing from the snapshot, or with an empty label,
/// drops its part. The pane's own name isn't repeated: the row shows it.
/// The footer cuts a long one workspace first ([`layout::footer`]).
fn location(workspace: &str, tab: Option<&str>, pane_id: &str) -> FooterLine {
    match non_empty(tab) {
        Some(tab) => FooterLine::from_parts([workspace, tab, pane_id]),
        None => FooterLine::from_parts([workspace, pane_id]),
    }
}

/// The kind of agent running in `pane`, if any.
fn agent_kind<'a>(snapshot: &'a Snapshot, pane: &Pane) -> Option<&'a str> {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane.pane_id)
        .and_then(Agent::kind)
}

/// `cwd` with a leading home directory replaced by `~` (also used for the
/// settings path on the palette's error screen).
pub(crate) fn with_tilde(cwd: &str, home: Option<&str>) -> String {
    if let Some(home) = non_empty(home) {
        if cwd == home {
            return "~".to_owned();
        }
        if let Some(rest) = cwd.strip_prefix(home).filter(|rest| rest.starts_with('/')) {
            return format!("~{rest}");
        }
    }
    cwd.to_owned()
}

/// The path a footer's `cwd` (from [`with_tilde`]) stands for: a leading
/// `~` back to the home directory. Herdr's cwds are absolute, so only
/// [`with_tilde`] starts one with `~`. Without a home, `cwd` as is.
pub(crate) fn without_tilde(cwd: &str, home: Option<&Path>) -> PathBuf {
    let rest = match cwd.strip_prefix('~') {
        Some("") => Some(""),
        Some(rest) => rest.strip_prefix('/'),
        None => None,
    };
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(cwd),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::fixtures::goto_snapshot;
    use crate::domain::layout::{FooterMode, LayoutMode};
    use crate::domain::row::Category;

    use super::*;

    #[test]
    fn builds_one_row_per_pane_with_go_to_names() {
        let rows = rows(&goto_snapshot(), None);

        assert_eq!(
            rows[2],
            Row::new(Kind::Pane, "w1:p3", "fix · 3")
                .with_title_column("dotfiles")
                .with_shortcut("claude")
                .with_right_width(8)
                .with_footer(vec![
                    FooterLine::from_parts(["dotfiles", "1", "w1:p3"]),
                    FooterLine::from_text("")
                ])
        );
        let ids: Vec<_> = rows.iter().map(|row| row.payload.as_str()).collect();
        assert_eq!(ids, ["w1:p1", "w1:p2", "w1:p3", "w2:p1", "w2:p2"]);
        assert_eq!(rows[0].shortcut, "terminal");
        assert_eq!(rows[3].full_title(), "api       codex · 1");
    }

    #[test]
    fn cuts_long_workspace_names_and_keeps_them_searchable() {
        let mut snapshot = goto_snapshot();
        snapshot.workspaces[0].label = "a-very-long-workspace-name".into();

        let rows = rows(&snapshot, None);

        assert_eq!(rows[0].full_title(), "a-very-long-workspa…  terminal · 1");
        assert_eq!(rows[0].search_extra, "a-very-long-workspace-name");
        assert_eq!(rows[3].full_title(), "api                   codex · 1");
        assert_eq!(rows[3].search_extra, "");
    }

    #[test]
    fn every_row_gets_the_widest_kind_as_its_right_column() {
        let rows = rows(&goto_snapshot(), None);

        let widths: Vec<_> = rows.iter().map(|row| row.right_width).collect();
        assert_eq!(widths, [8; 5]); // `terminal`, wider than `claude`
    }

    #[test]
    fn shows_the_home_directory_as_a_tilde() {
        assert_eq!(with_tilde("/home/me", Some("/home/me")), "~");
        assert_eq!(with_tilde("/home/me/src", Some("/home/me")), "~/src");
        assert_eq!(with_tilde("/home/meow", Some("/home/me")), "/home/meow");
        assert_eq!(with_tilde("/home/me", Some("")), "/home/me");
        assert_eq!(with_tilde("/home/me", None), "/home/me");
    }

    #[test]
    fn turns_a_tilde_back_into_the_home_directory() {
        let home = Some(Path::new("/home/me"));

        assert_eq!(without_tilde("~", home), Path::new("/home/me"));
        assert_eq!(without_tilde("~/src", home), Path::new("/home/me/src"));
        assert_eq!(without_tilde("~x/src", home), Path::new("~x/src"));
        assert_eq!(without_tilde("/srv/x", home), Path::new("/srv/x"));
        assert_eq!(without_tilde("~/src", None), Path::new("~/src"));
    }

    #[test]
    fn prefers_the_foreground_cwd_in_the_footer() {
        let mut snapshot = goto_snapshot();
        snapshot.panes[0].cwd = Some("/home/me/shell".into());
        snapshot.panes[0].foreground_cwd = Some("/home/me/fg".into());

        let rows = rows(&snapshot, Some(Path::new("/home/me")));

        assert_eq!(
            rows[0].footer,
            [
                FooterLine::from_parts(["dotfiles", "1", "w1:p1"]),
                FooterLine::from_text("~/fg")
            ]
        );
    }

    #[test]
    fn names_panes_of_missing_tabs_by_id() {
        let mut snapshot = goto_snapshot();
        snapshot.tabs.retain(|tab| tab.tab_id != "w2:t2");

        let rows = rows(&snapshot, None);

        assert_eq!(rows[4].full_title(), "api       w2:p2");
    }

    #[test]
    fn locates_a_pane_by_its_named_tab() {
        let rows = rows(&goto_snapshot(), None);

        assert_eq!(
            rows[4].footer[0],
            FooterLine::from_parts(["api", "logs", "w2:p2"])
        );
    }

    #[test]
    fn locates_a_pane_by_its_unnamed_tabs_number() {
        let rows = rows(&goto_snapshot(), None);

        assert_eq!(
            rows[3].footer[0],
            FooterLine::from_parts(["api", "1", "w2:p1"])
        );
    }

    #[test]
    fn drops_the_tab_from_the_location_when_the_tab_is_missing() {
        let mut snapshot = goto_snapshot();
        snapshot.tabs.retain(|tab| tab.tab_id != "w2:t2");

        let rows = rows(&snapshot, None);

        assert_eq!(rows[4].footer[0], FooterLine::from_parts(["api", "w2:p2"]));
    }

    #[test]
    fn drops_the_tab_from_the_location_when_its_label_is_empty() {
        assert_eq!(
            location("api", Some(""), "w2:p3"),
            FooterLine::from_parts(["api", "w2:p3"])
        );
    }

    #[test]
    fn keeps_the_full_workspace_name_in_the_location() {
        let mut snapshot = goto_snapshot();
        snapshot.workspaces[0].label = "a-very-long-workspace-name".into();

        let rows = rows(&snapshot, None);

        assert_eq!(
            rows[0].footer[0],
            FooterLine::from_parts(["a-very-long-workspace-name", "1", "w1:p1"])
        );
    }

    #[test]
    fn keeps_a_tab_label_with_a_separator_in_one_part() {
        let mut snapshot = goto_snapshot();
        snapshot.tabs[2].label = "feat / x".into();
        snapshot.workspaces[1].label = "herdr-command-palette-worktree".into();
        let rows = rows(&snapshot, None);

        let footer = layout::footer(
            FooterMode::Description,
            Category::JumpTo,
            &rows[4].footer,
            None,
            30,
            LayoutMode::Full,
        );

        assert_eq!(
            rows[4].footer[0],
            FooterLine::from_parts(["herdr-command-palette-worktree", "feat / x", "w2:p2"])
        );
        assert_eq!(footer.lines().next(), Some(" herdr-co… / feat / x / w2:p2"));
    }

    #[test]
    fn measures_the_workspace_column_without_tags() {
        let mut snapshot = goto_snapshot();
        snapshot.workspaces[0].label = "dot{hcp:seg}files".into();

        let rows = rows(&snapshot, None);

        assert_eq!(rows[0].full_title(), "dotfiles  terminal · 1");
        assert_eq!(rows[3].full_title(), "api       codex · 1");
        assert_eq!(
            rows[0].footer[0],
            FooterLine::from_parts(["dotfiles", "1", "w1:p1"])
        );
    }

    #[test]
    fn cuts_the_workspace_first_in_the_footer() {
        let mut snapshot = goto_snapshot();
        snapshot.workspaces[0].label = "herdr-command-palette-worktree".into();
        let rows = rows(&snapshot, None);

        let footer = layout::footer(
            FooterMode::Description,
            Category::JumpTo,
            &rows[0].footer,
            None,
            30,
            LayoutMode::Full,
        );

        assert_eq!(
            footer,
            " herdr-command-p… / 1 / w1:p1\n /x                      [^/]"
        );
    }
}
