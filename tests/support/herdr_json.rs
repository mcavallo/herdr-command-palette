//! Herdr's answers as JSON, shared by the use case fakes
//! (`src/app/fakes.rs`), the domain fixtures and the integration tests.
//!
//! Include from an integration test with
//! `#[path = "support/herdr_json.rs"] mod herdr_json;`
//! (`src/domain/fixtures.rs` uses `#[path = "../../tests/support/herdr_json.rs"]`).

// Reason: each test crate that includes this file uses only part of it.
#![allow(dead_code)]

use serde_json::{Value, json};

/// `session.snapshot` result: workspace `w1` ("dotfiles"), tab `w1:t1`
/// (120 columns by 40 rows) with panes `w1:p1` (focused, running claude) and `w1:p2`.
pub fn snapshot() -> Value {
    json!({"snapshot": {
        "focused_workspace_id": "w1",
        "focused_tab_id": "w1:t1",
        "focused_pane_id": "w1:p1",
        "workspaces": [{"workspace_id": "w1", "label": "dotfiles"}],
        "tabs": [{"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"}],
        "panes": [
            {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1", "cwd": "/home/me/src"},
            {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1", "cwd": "/tmp"},
        ],
        "agents": [{"pane_id": "w1:p1", "agent": "claude"}],
        "layouts": [{"tab_id": "w1:t1", "area": {"x": 0, "y": 0, "width": 120, "height": 40}}],
    }})
}

/// `session.snapshot` result with a worktree group: `repo` (`w1`, main
/// checkout, one tab `w1:t1`, focused) and its linked worktree
/// `repo-fix` (`w2`), one pane each. Closing `w1:t1` closes both.
pub fn worktree_group_snapshot() -> Value {
    let worktree =
        |linked: bool| json!({"repo_key": "/src/repo/.git", "is_linked_worktree": linked});
    json!({"snapshot": {
        "focused_workspace_id": "w1",
        "focused_tab_id": "w1:t1",
        "focused_pane_id": "w1:p1",
        "workspaces": [
            {"workspace_id": "w1", "label": "repo", "worktree": worktree(false)},
            {"workspace_id": "w2", "label": "repo-fix", "worktree": worktree(true)},
        ],
        "tabs": [
            {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
            {"tab_id": "w2:t1", "workspace_id": "w2", "label": "1"},
        ],
        "panes": [
            {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"},
            {"pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1"},
        ],
        "layouts": [{"tab_id": "w1:t1", "area": {"x": 0, "y": 0, "width": 120, "height": 40}}],
    }})
}

/// `plugin.action.list` result with two demo actions and this plugin's
/// own actions. The palette excludes its own actions from the list.
pub fn plugin_actions() -> Value {
    json!({"actions": [
        {"plugin_id": "demo", "action_id": "hello", "title": "Say hello"},
        {"plugin_id": "demo", "action_id": "bye", "title": "Say bye"},
        {"plugin_id": "mmc.command-palette", "action_id": "open", "title": "Open command palette"},
        {"plugin_id": "mmc.command-palette", "action_id": "repeat-command", "title": "Repeat last command"},
        {"plugin_id": "mmc.command-palette", "action_id": "repeat-jump", "title": "Repeat last jump"},
    ]})
}
