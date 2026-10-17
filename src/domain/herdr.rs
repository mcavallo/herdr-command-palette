//! What Herdr sends and receives, as typed data: the session snapshot, the
//! plugin action list, the invocation context, socket requests and the
//! shape of their results.
//!
//! Only the fields the palette uses are modelled; serde ignores the rest (no
//! `deny_unknown_fields`, unlike the plugin's settings), so
//! newer Herdr versions that add fields keep working. Field names and
//! optionality follow Herdr's API schema (`herdr api schema --json`,
//! protocol 22, Herdr 0.9.3).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

// Manifest: entrypoints declared in `herdr-plugin.toml` (`[[panes]]`).

/// The manifest's pane that runs the palette popup (`[[panes]] palette`).
pub(crate) const PALETTE_ENTRYPOINT: &str = "palette";

/// The manifest's pane that runs `sh -c "$HCP_EXEC"` for custom
/// `popup`/`pane` commands (`[[panes]] exec`).
pub(crate) const EXEC_ENTRYPOINT: &str = "exec";

/// The manifest's pane that asks before a repeat action runs a row the
/// settings' `confirm_ids` lists (`[[panes]] confirm`).
pub(crate) const CONFIRM_ENTRYPOINT: &str = "confirm";

/// Title of the palette's Herdr notifications.
const NOTIFICATION_TITLE: &str = "command-palette";

/// One socket call: a method name and its JSON params.
///
/// The adapter wraps it in Herdr's envelope (`{"id", "method", "params"}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Socket method, e.g. `pane.focus`.
    pub method: String,
    /// Method params, a JSON object (`{}` when there are none).
    pub params: Value,
}

impl Request {
    /// A request with the given method and params.
    #[must_use]
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }

    /// A request without params (`{}`).
    #[must_use]
    pub fn without_params(method: impl Into<String>) -> Self {
        Self::new(method, Value::Object(Map::new()))
    }

    /// `pane.focus` on `pane_id`.
    #[must_use]
    pub fn focus_pane(pane_id: &str) -> Self {
        Self::new("pane.focus", json!({"pane_id": pane_id}))
    }

    /// `plugin.pane.open` with `params` (`plugin_id`, `entrypoint`, ...).
    #[must_use]
    pub fn plugin_pane_open(params: Map<String, Value>) -> Self {
        Self::new("plugin.pane.open", Value::Object(params))
    }

    /// `session.snapshot`; its result is a [`SnapshotResult`].
    #[must_use]
    pub fn snapshot() -> Self {
        Self::without_params("session.snapshot")
    }

    /// `plugin.action.list`; its result is a [`PluginActionsResult`].
    #[must_use]
    pub fn plugin_actions() -> Self {
        Self::without_params("plugin.action.list")
    }

    /// `notification.show`: a toast titled `NOTIFICATION_TITLE`.
    #[must_use]
    pub fn notification(body: &str) -> Self {
        Self::new(
            "notification.show",
            json!({"title": NOTIFICATION_TITLE, "body": body}),
        )
    }
}

/// The `result` of `session.snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SnapshotResult {
    /// The session.
    pub snapshot: Snapshot,
}

/// The `result` of `plugin.action.list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PluginActionsResult {
    /// Every action of every enabled plugin.
    pub actions: Vec<PluginAction>,
}

/// `method {params}`, as dry runs print it.
impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.method, self.params)
    }
}

/// The live session, from `session.snapshot` (its `result.snapshot`).
///
/// Lists are in Herdr's order (workspace, tab, pane), which is the order
/// the palette shows panes in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Snapshot {
    /// Workspaces in Herdr's order.
    pub workspaces: Vec<Workspace>,
    /// Tabs of every workspace, in order.
    pub tabs: Vec<Tab>,
    /// Panes of every tab, in order.
    pub panes: Vec<Pane>,
    /// Panes that run a detected agent.
    #[serde(default)]
    pub agents: Vec<Agent>,
    /// Geometry of each tab.
    #[serde(default)]
    pub layouts: Vec<Layout>,
    /// Focused workspace, if any.
    pub focused_workspace_id: Option<String>,
    /// Focused tab, if any.
    pub focused_tab_id: Option<String>,
    /// Focused pane, if any.
    pub focused_pane_id: Option<String>,
}

/// Herdr's close rules (`workspace_close_indices`,
/// `close_pane_would_close_workspace` and `request_tab_close`, Herdr
/// `src/app/actions.rs` and `src/client/shell/overlay_input.rs`, v0.9.3).
impl Snapshot {
    /// The workspace closing tab `tab_id` closes: its workspace, when it
    /// is that workspace's last tab.
    #[must_use]
    pub fn workspace_closed_by_tab(&self, tab_id: &str) -> Option<&Workspace> {
        let tab = self.tabs.iter().find(|tab| tab.tab_id == tab_id)?;
        let is_last = !self
            .tabs
            .iter()
            .any(|other| other.workspace_id == tab.workspace_id && other.tab_id != tab_id);
        if !is_last {
            return None;
        }
        self.workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == tab.workspace_id)
    }

    /// The workspaces closing `workspace` closes: every workspace of its
    /// repository when it is the main checkout, else just itself.
    #[must_use]
    pub fn close_group<'a>(&'a self, workspace: &'a Workspace) -> Vec<&'a Workspace> {
        let key = workspace
            .worktree
            .as_ref()
            .filter(|worktree| !worktree.is_linked_worktree)
            .map(|worktree| worktree.repo_key.as_str());
        match key {
            Some(key) => self
                .workspaces
                .iter()
                .filter(|member| {
                    member
                        .worktree
                        .as_ref()
                        .is_some_and(|worktree| worktree.repo_key == key)
                })
                .collect(),
            None => vec![workspace],
        }
    }

    /// The workspace whose worktree group closing tab `tab_id` closes:
    /// the tab is its workspace's last, and that workspace's close takes
    /// other workspaces with it.
    #[must_use]
    pub fn group_closed_by_tab(&self, tab_id: &str) -> Option<&Workspace> {
        let workspace = self.workspace_closed_by_tab(tab_id)?;
        (self.close_group(workspace).len() > 1).then_some(workspace)
    }

    /// The workspace whose worktree group closing pane `pane_id` closes:
    /// the pane is the only one of its workspace's only tab
    /// (`close_pane_would_close_workspace`), and that workspace's close
    /// takes other workspaces with it.
    #[must_use]
    pub fn group_closed_by_pane(&self, pane_id: &str) -> Option<&Workspace> {
        let pane = self.panes.iter().find(|pane| pane.pane_id == pane_id)?;
        let is_only = !self
            .panes
            .iter()
            .any(|other| other.tab_id == pane.tab_id && other.pane_id != pane_id);
        if !is_only {
            return None;
        }
        self.group_closed_by_tab(&pane.tab_id)
    }
}

/// A workspace (`WorkspaceInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Workspace {
    /// Id, e.g. `w1`.
    pub workspace_id: String,
    /// Display name.
    pub label: String,
    /// The git checkout the workspace was opened on, if any; `None` too
    /// in snapshots that predate the field.
    #[serde(default)]
    pub worktree: Option<WorkspaceWorktree>,
}

/// The git checkout of a workspace (`WorkspaceWorktreeInfo`): what Herdr
/// groups worktree workspaces by.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceWorktree {
    /// The repository, shared by its main checkout and its linked
    /// worktrees: the group key.
    pub repo_key: String,
    /// Whether this is a linked worktree (`git worktree add`), not the
    /// main checkout.
    pub is_linked_worktree: bool,
}

/// A tab (`TabInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Tab {
    /// Id, e.g. `w1:t1`.
    pub tab_id: String,
    /// Workspace it belongs to.
    pub workspace_id: String,
    /// Display name; Herdr uses the tab number (`"1"`) when unnamed.
    pub label: String,
}

/// A pane (`PaneInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pane {
    /// Id, e.g. `w1:p2`.
    pub pane_id: String,
    /// Workspace it belongs to.
    pub workspace_id: String,
    /// Tab it belongs to.
    pub tab_id: String,
    /// Name the user gave the pane, if any.
    pub label: Option<String>,
    /// Shell working directory, if known.
    pub cwd: Option<String>,
    /// Working directory of the foreground process, if known; preferred
    /// over [`cwd`](Self::cwd).
    pub foreground_cwd: Option<String>,
}

impl Pane {
    /// The best known working directory: foreground process, else shell.
    #[must_use]
    pub fn working_dir(&self) -> Option<&str> {
        self.foreground_cwd.as_deref().or(self.cwd.as_deref())
    }
}

/// A pane running a detected agent (`AgentInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Agent {
    /// Pane the agent runs in.
    pub pane_id: String,
    /// Agent kind, e.g. `claude`.
    pub agent: Option<String>,
    /// Agent kind as displayed, when [`agent`](Self::agent) is unset.
    pub display_agent: Option<String>,
    /// Session name the agent reported, if any.
    pub name: Option<String>,
    /// Title the agent reported, if any.
    pub title: Option<String>,
    /// Terminal title without status glyphs, if any.
    pub terminal_title_stripped: Option<String>,
}

impl Agent {
    /// The agent kind: [`agent`](Self::agent), else
    /// [`display_agent`](Self::display_agent).
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.agent.as_deref().or(self.display_agent.as_deref())
    }
}

/// Geometry of one tab (`PaneLayoutSnapshot`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Layout {
    /// The tab this layout belongs to.
    pub tab_id: String,
    /// Screen area the tab's panes share.
    pub area: Area,
}

/// A screen rectangle in cells (`PaneLayoutRect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Area {
    /// Left column.
    pub x: u16,
    /// Top row.
    pub y: u16,
    /// Width in columns.
    pub width: u16,
    /// Height in rows.
    pub height: u16,
}

/// One plugin action (`PluginActionInfo`), from `plugin.action.list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PluginAction {
    /// Plugin id, e.g. `wtherdr`.
    pub plugin_id: String,
    /// Action id within the plugin, e.g. `merge`.
    pub action_id: String,
    /// Title from the plugin manifest.
    pub title: String,
    /// Description from the plugin manifest, if any.
    pub description: Option<String>,
}

impl PluginAction {
    /// `<plugin_id>.<action_id>`: the id `plugin.action.invoke` and
    /// `[[keys.command]]` use.
    #[must_use]
    pub fn qualified_id(&self) -> String {
        format!("{}.{}", self.plugin_id, self.action_id)
    }
}

/// Where the user was when Herdr started the plugin
/// (`HERDR_PLUGIN_CONTEXT_JSON`, `PluginInvocationContext`).
///
/// Every field is optional; targets fall back to the snapshot's focused ids.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct InvocationContext {
    /// Workspace the plugin was invoked in.
    pub workspace_id: Option<String>,
    /// Tab the plugin was invoked in.
    pub tab_id: Option<String>,
    /// Pane that was focused.
    pub focused_pane_id: Option<String>,
    /// Working directory of the focused pane.
    pub focused_pane_cwd: Option<String>,
}

impl InvocationContext {
    /// Parses the context JSON. Missing, empty or invalid JSON gives an
    /// empty context: invalid context must not stop the palette.
    #[must_use]
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }
}

/// A `pane.moved` event (`HERDR_PLUGIN_EVENT_JSON` of the `pane.moved`
/// hook): the pane's id before and after the move.
///
/// Verified in Herdr 0.9.3: `{"event": "pane_moved", "data": {"type":
/// "pane_moved", "previous_pane_id": "w1:p2", "pane": {"pane_id": "w2:p2",
/// ...}, ...}}`. Moving within a workspace keeps the id; moving to another
/// workspace gives a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMoved {
    /// The id the pane had before the move.
    pub previous_pane_id: String,
    /// The id it has now.
    pub pane_id: String,
}

impl PaneMoved {
    /// Parses the hook's event JSON; `None` when it isn't a `pane_moved`
    /// event with both ids.
    #[must_use]
    pub fn from_event_json(json: &str) -> Option<Self> {
        // Herdr's envelope; serde ignores the many other fields.
        #[derive(Debug, Deserialize)]
        struct Envelope {
            data: Data,
        }
        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "type")]
            kind: String,
            previous_pane_id: String,
            pane: MovedPane,
        }
        #[derive(Debug, Deserialize)]
        struct MovedPane {
            pane_id: String,
        }

        let envelope: Envelope = serde_json::from_str(json).ok()?;
        (envelope.data.kind == "pane_moved").then_some(Self {
            previous_pane_id: envelope.data.previous_pane_id,
            pane_id: envelope.data.pane.pane_id,
        })
    }

    /// Whether the pane got a new id (it moved to another workspace).
    #[must_use]
    pub fn has_new_id(&self) -> bool {
        self.previous_pane_id != self.pane_id
    }
}

/// A `pane.created` event (`HERDR_PLUGIN_EVENT_JSON` of the
/// `pane.created` hook): the new pane's id.
///
/// Verified in Herdr 0.9.3: `{"event": "pane_created", "data": {"type":
/// "pane_created", "pane": {"pane_id": "w2:p2", ...}}}`. Fires on a new
/// workspace, tab or split, including a pane id reused after a restart;
/// never on session restore or a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneCreated {
    /// The new pane's id.
    pub pane_id: String,
}

impl PaneCreated {
    /// Parses the hook's event JSON; `None` when it isn't a `pane_created`
    /// event with a pane id.
    #[must_use]
    pub fn from_event_json(json: &str) -> Option<Self> {
        // Herdr's envelope; serde ignores the many other fields.
        #[derive(Debug, Deserialize)]
        struct Envelope {
            data: Data,
        }
        #[derive(Debug, Deserialize)]
        struct Data {
            #[serde(rename = "type")]
            kind: String,
            pane: CreatedPane,
        }
        #[derive(Debug, Deserialize)]
        struct CreatedPane {
            pane_id: String,
        }

        let envelope: Envelope = serde_json::from_str(json).ok()?;
        (envelope.data.kind == "pane_created").then_some(Self {
            pane_id: envelope.data.pane.pane_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real Herdr 0.9.3 snapshot.
    const SNAPSHOT: &str = r#"{
      "agents": [{"agent": "claude", "agent_status": "working", "pane_id": "w4:p1",
                  "terminal_title_stripped": "fix bug", "tab_id": "w4:t1"}],
      "focused_pane_id": "w4:p1", "focused_tab_id": "w4:t1", "focused_workspace_id": "w4",
      "layouts": [{"area": {"height": 55, "width": 229, "x": 0, "y": 0},
                   "focused_pane_id": "w4:p1", "panes": [], "splits": [],
                   "tab_id": "w4:t1", "workspace_id": "w4", "zoomed": false}],
      "panes": [{"cwd": "/home/u", "foreground_cwd": "/home/u/src", "pane_id": "w4:p1",
                 "tab_id": "w4:t1", "workspace_id": "w4", "terminal_id": "t1"}],
      "protocol": 22,
      "tabs": [{"label": "1", "number": 1, "tab_id": "w4:t1", "workspace_id": "w4"}],
      "version": "0.9.3",
      "workspaces": [{"label": "dotfiles", "number": 1, "workspace_id": "w4",
                      "worktree": {"checkout_path": "/home/u/src", "is_linked_worktree": false,
                                   "repo_key": "/home/u/src/.git", "repo_name": "src",
                                   "repo_root": "/home/u/src"}}]
    }"#;

    #[test]
    fn snapshot_parses_a_real_shape() {
        let snap: Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
        assert_eq!(snap.focused_pane_id.as_deref(), Some("w4:p1"));
        assert_eq!(snap.workspaces[0].label, "dotfiles");
        assert_eq!(
            snap.workspaces[0].worktree,
            Some(WorkspaceWorktree {
                repo_key: "/home/u/src/.git".into(),
                is_linked_worktree: false
            })
        );
        assert_eq!(snap.tabs[0].label, "1");
        assert_eq!(snap.panes[0].working_dir(), Some("/home/u/src"));
        assert_eq!(snap.panes[0].label, None);
        assert_eq!(snap.agents[0].kind(), Some("claude"));
        assert_eq!(
            snap.layouts[0].area,
            Area {
                x: 0,
                y: 0,
                width: 229,
                height: 55
            }
        );
    }

    #[test]
    fn snapshot_parts_other_than_the_lists_are_optional() {
        let snap: Snapshot =
            serde_json::from_str(r#"{"workspaces": [], "tabs": [], "panes": []}"#).unwrap();

        assert_eq!(snap, Snapshot::default());
    }

    #[test]
    fn workspaces_without_a_worktree_parse() {
        let workspace: Workspace =
            serde_json::from_value(json!({"workspace_id": "w1", "label": "a", "worktree": null}))
                .unwrap();
        let old: Workspace =
            serde_json::from_value(json!({"workspace_id": "w1", "label": "a"})).unwrap();

        assert_eq!(workspace.worktree, None);
        assert_eq!(old, workspace);
    }

    #[test]
    fn pane_working_dir_falls_back_to_its_cwd() {
        let pane: Pane = serde_json::from_str(
            r#"{"pane_id": "p", "workspace_id": "w", "tab_id": "t", "cwd": "/x"}"#,
        )
        .unwrap();

        assert_eq!(pane.working_dir(), Some("/x"));
    }

    #[test]
    fn agent_kind_falls_back_to_the_display_agent() {
        let agent: Agent =
            serde_json::from_str(r#"{"pane_id": "p", "display_agent": "codex"}"#).unwrap();

        assert_eq!(agent.kind(), Some("codex"));
    }

    #[test]
    fn plugin_actions_have_a_qualified_id() {
        let action: PluginAction = serde_json::from_value(json!({
            "plugin_id": "wtherdr", "action_id": "merge", "title": "wt merge",
            "command": ["wtherdr", "popup", "merge"]
        }))
        .unwrap();
        assert_eq!(action.qualified_id(), "wtherdr.merge");
        assert_eq!(action.description, None);
    }

    #[test]
    fn invocation_context_reads_the_ids_and_ignores_unknown_fields() {
        let ctx = InvocationContext::from_json(
            r#"{"workspace_id": "w1", "tab_id": "w1:t2", "focused_pane_id": "w1:p3",
                "focused_pane_cwd": "/tmp", "invocation_source": "keybinding"}"#,
        );

        assert_eq!(ctx.focused_pane_id.as_deref(), Some("w1:p3"));
        assert_eq!(ctx.tab_id.as_deref(), Some("w1:t2"));
    }

    /// Trimmed from a real Herdr 0.9.3 `pane.moved` hook.
    const PANE_MOVED: &str = r#"{"event": "pane_moved", "data": {"type": "pane_moved",
        "previous_pane_id": "w1:p2", "previous_workspace_id": "w1",
        "pane": {"pane_id": "w2:p2", "workspace_id": "w2", "tab_id": "w2:t2"},
        "created_tab": {"tab_id": "w2:t2"}}}"#;

    #[test]
    fn pane_moved_parses_a_real_event() {
        let moved = PaneMoved::from_event_json(PANE_MOVED).unwrap();

        assert_eq!(
            moved,
            PaneMoved {
                previous_pane_id: "w1:p2".into(),
                pane_id: "w2:p2".into()
            }
        );
        assert!(moved.has_new_id());
    }

    #[test]
    fn a_move_within_the_workspace_keeps_the_id() {
        let json = PANE_MOVED.replace("w1:p2", "w2:p2");

        let moved = PaneMoved::from_event_json(&json).unwrap();

        assert!(!moved.has_new_id());
    }

    /// Trimmed from a real Herdr 0.9.3 `pane.created` hook.
    const PANE_CREATED: &str = r#"{"event": "pane_created", "data": {"type": "pane_created",
        "pane": {"pane_id": "w2:p2", "terminal_id": "term_65d79ca54992b6",
        "workspace_id": "w2", "tab_id": "w2:t1", "focused": true, "revision": 0}}}"#;

    #[test]
    fn pane_created_parses_a_real_event() {
        let created = PaneCreated::from_event_json(PANE_CREATED);

        assert_eq!(
            created,
            Some(PaneCreated {
                pane_id: "w2:p2".into()
            })
        );
    }

    #[test]
    fn other_or_invalid_events_are_not_creations() {
        let focused = r#"{"event": "pane_focused", "data": {"type": "pane_focused",
            "pane": {"pane_id": "w1:p1"}}}"#;

        assert_eq!(PaneCreated::from_event_json(focused), None);
        assert_eq!(PaneCreated::from_event_json("{bad"), None);
        assert_eq!(PaneCreated::from_event_json(PANE_MOVED), None);
    }

    #[test]
    fn other_or_invalid_events_are_not_moves() {
        let focused = r#"{"event": "pane_focused", "data": {"type": "pane_focused",
            "previous_pane_id": "w1:p1", "pane": {"pane_id": "w1:p1"}}}"#;

        assert_eq!(PaneMoved::from_event_json(focused), None);
        assert_eq!(PaneMoved::from_event_json("{bad"), None);
        assert_eq!(PaneMoved::from_event_json(r#"{"data": {}}"#), None);
    }

    #[test]
    fn an_empty_or_invalid_invocation_context_is_the_default() {
        assert_eq!(
            InvocationContext::from_json(""),
            InvocationContext::default()
        );
        assert_eq!(
            InvocationContext::from_json("{bad"),
            InvocationContext::default()
        );
    }

    #[test]
    fn requests_print_like_a_dry_run() {
        let request = Request::new("pane.focus", json!({"pane_id": "w1:p2"}));

        assert_eq!(request.to_string(), r#"pane.focus {"pane_id":"w1:p2"}"#);
    }

    #[test]
    fn requests_without_params_send_an_empty_object() {
        let bare = Request::without_params("server.reload_config");

        assert_eq!(bare.params, json!({}));
    }

    #[test]
    fn named_requests_carry_herdr_methods_and_params() {
        assert_eq!(Request::snapshot().to_string(), "session.snapshot {}");
        assert_eq!(
            Request::plugin_actions().to_string(),
            "plugin.action.list {}"
        );
        assert_eq!(
            Request::notification("gone"),
            Request::new(
                "notification.show",
                json!({"title": "command-palette", "body": "gone"})
            )
        );
    }

    #[test]
    fn results_wrap_the_snapshot_and_the_actions() {
        let snapshot: SnapshotResult =
            serde_json::from_value(json!({"type": "session_snapshot", "snapshot": {
                "workspaces": [], "tabs": [], "panes": []
            }}))
            .unwrap();
        let actions: PluginActionsResult = serde_json::from_value(json!({"actions": []})).unwrap();

        assert_eq!(snapshot.snapshot, Snapshot::default());
        assert_eq!(actions.actions, []);
    }
}
