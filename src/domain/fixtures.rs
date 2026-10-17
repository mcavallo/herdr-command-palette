//! Test data shared by the domain tests (and the use case fakes): Herdr's
//! answers, snapshots, plugin actions, request effects and tricky text.
//!
//! Test data used by one module only stays in that module's `tests`.

/// Herdr's answers as JSON, shared with the integration tests.
#[path = "../../tests/support/herdr_json.rs"]
pub(crate) mod herdr_json;

use proptest::prelude::*;
use serde_json::{Value, json};

use crate::domain::herdr::{
    InvocationContext, PluginAction, PluginActionsResult, Request, Snapshot,
};
use crate::domain::requests::Effect;

/// A session with two workspaces, a three-pane tab containing a labelled
/// pane and an agent, and a named tab.
pub(crate) fn goto_snapshot() -> Snapshot {
    serde_json::from_value(json!({
        "workspaces": [
            {"workspace_id": "w1", "label": "dotfiles"},
            {"workspace_id": "w2", "label": "api"},
        ],
        "tabs": [
            {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
            {"tab_id": "w2:t1", "workspace_id": "w2", "label": "1"},
            {"tab_id": "w2:t2", "workspace_id": "w2", "label": "logs"},
        ],
        "panes": [
            {"pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1", "cwd": "/x"},
            {"pane_id": "w1:p2", "tab_id": "w1:t1", "workspace_id": "w1", "label": "potatoes"},
            {"pane_id": "w1:p3", "tab_id": "w1:t1", "workspace_id": "w1"},
            {"pane_id": "w2:p1", "tab_id": "w2:t1", "workspace_id": "w2"},
            {"pane_id": "w2:p2", "tab_id": "w2:t2", "workspace_id": "w2"},
        ],
        "agents": [
            {"pane_id": "w1:p3", "agent": "claude", "terminal_title_stripped": "fix"},
            {"pane_id": "w2:p1", "agent": "codex"},
        ],
    }))
    .unwrap()
}

/// `repo` (w1, main checkout, 2 panes), its linked worktree `repo-fix`
/// (w2, 1 pane), and `notes` (w3, no checkout, 1 pane); one tab each.
pub(crate) fn worktree_snapshot() -> Snapshot {
    let worktree = |linked: bool| {
        json!({"repo_key": "/src/repo/.git", "repo_name": "repo", "repo_root": "/src/repo",
               "checkout_path": "/src/repo", "is_linked_worktree": linked})
    };
    serde_json::from_value(json!({
        "workspaces": [
            {"workspace_id": "w1", "label": "repo", "worktree": worktree(false)},
            {"workspace_id": "w2", "label": "repo-fix", "worktree": worktree(true)},
            {"workspace_id": "w3", "label": "notes"},
        ],
        "tabs": [
            {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
            {"tab_id": "w2:t1", "workspace_id": "w2", "label": "1"},
            {"tab_id": "w3:t1", "workspace_id": "w3", "label": "1"},
        ],
        "panes": [
            {"pane_id": "w1:p1", "tab_id": "w1:t1", "workspace_id": "w1"},
            {"pane_id": "w1:p2", "tab_id": "w1:t1", "workspace_id": "w1"},
            {"pane_id": "w2:p1", "tab_id": "w2:t1", "workspace_id": "w2"},
            {"pane_id": "w3:p1", "tab_id": "w3:t1", "workspace_id": "w3"},
        ],
    }))
    .unwrap()
}

/// [`worktree_snapshot`] with `repo` down to one pane.
pub(crate) fn group_with_one_pane() -> Snapshot {
    let mut snapshot = worktree_snapshot();
    snapshot.panes.retain(|pane| pane.pane_id != "w1:p2");
    snapshot
}

/// `snapshot` with a second tab, `<workspace_id>:t2`, in `workspace_id`.
pub(crate) fn with_second_tab(mut snapshot: Snapshot, workspace_id: &str) -> Snapshot {
    snapshot.tabs.push(
        serde_json::from_value(json!({
            "tab_id": format!("{workspace_id}:t2"), "workspace_id": workspace_id, "label": "2",
        }))
        .unwrap(),
    );
    snapshot
}

/// Invoked from tab `tab_id`.
pub(crate) fn in_tab(tab_id: &str) -> InvocationContext {
    InvocationContext {
        tab_id: Some(tab_id.into()),
        ..InvocationContext::default()
    }
}

/// Invoked with `pane_id` focused.
pub(crate) fn on_pane(pane_id: &str) -> InvocationContext {
    InvocationContext {
        focused_pane_id: Some(pane_id.into()),
        ..InvocationContext::default()
    }
}

/// A plugin action without a description.
pub(crate) fn plugin_action(plugin_id: &str, action_id: &str, title: &str) -> PluginAction {
    PluginAction {
        plugin_id: plugin_id.into(),
        action_id: action_id.into(),
        title: title.into(),
        description: None,
    }
}

/// [`herdr_json::plugin_actions`], decoded.
pub(crate) fn plugin_actions() -> Vec<PluginAction> {
    let result: PluginActionsResult = serde_json::from_value(herdr_json::plugin_actions()).unwrap();
    result.actions
}

/// The effect of sending `method` with `params` to Herdr.
pub(crate) fn herdr_effect(method: &str, params: Value) -> Effect {
    Effect::Herdr(Request::new(method, params))
}

/// Tricky text for snapshots and property tests: wide, combining,
/// joined, variation-selected, private-use and zero-width characters,
/// plus plain ASCII and the empty string.
pub(crate) const CORPUS: [&str; 10] = [
    "Split pane right",
    "日本語テキスト",
    "Cafe\u{301} cre\u{300}me",
    "👨‍👩‍👧‍👦 family",
    "🇦🇷🇯🇵 flags",
    "❤\u{fe0f} love",
    "\u{e0a0} wt create",
    "a…b · c",
    "x\u{200b}y",
    "",
];

/// Strings mixing [`CORPUS`] entries with arbitrary characters.
pub(crate) fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        proptest::sample::select(CORPUS.to_vec()).prop_map(str::to_owned),
        any::<char>().prop_map(String::from),
    ];
    proptest::collection::vec(piece, 0..8).prop_map(|pieces| pieces.concat())
}
