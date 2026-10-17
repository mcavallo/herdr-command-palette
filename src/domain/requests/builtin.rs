//! Built-in Herdr key actions and the socket request each one sends.
//!
//! Mirrors Herdr's client, which turns a key action into a socket method in
//! `ClientShell::endpoint_method_for_action` (and a few special cases in
//! `record_binding`), Herdr `src/client/shell/actions.rs`, v0.9.3.
//! Computed actions (next/previous tab, workspace, agent, cycle pane, move
//! tab) reproduce its wrap-around and insert indices.

use std::str::FromStr;

use serde_json::json;

use crate::domain::catalog::NamedCommand;
use crate::domain::catalog::builtins::BUILTINS;
use crate::domain::commands::{Builtin, Direction, Step};
use crate::domain::herdr::{Request, Snapshot};

use super::{Effect, RequestError, Targets};

/// Parses a `[keys]` name (`zoom`, `focus_pane_left`, ...), as listed in
/// [`BUILTINS`], the single table of built-ins.
impl FromStr for Builtin {
    type Err = RequestError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        NamedCommand::find(&BUILTINS, name)
            .map(|builtin| builtin.action)
            .ok_or_else(|| RequestError::UnknownBuiltin(name.to_owned()))
    }
}

impl Builtin {
    /// What running this built-in does, given the session and the targets.
    ///
    /// [`Effect::Nothing`] when there is nothing to act on: a missing
    /// workspace, tab or pane target, an empty list to cycle, a single tab
    /// to move. Herdr's client silently does nothing in those cases too.
    #[must_use]
    pub fn to_effect(self, snapshot: &Snapshot, targets: Targets<'_>) -> Effect {
        match self.request(snapshot, targets) {
            Some(request) => Effect::Herdr(request),
            None => Effect::Nothing,
        }
    }

    /// The request this built-in sends; `None` when there is nothing to do.
    /// `?` on an `Option` returns `None` early, like on a `Result`.
    fn request(self, snapshot: &Snapshot, targets: Targets<'_>) -> Option<Request> {
        let request = match self {
            Self::SplitVertical | Self::SplitHorizontal => {
                let direction = if self == Self::SplitVertical {
                    Direction::Right
                } else {
                    Direction::Down
                };
                Request::new(
                    "pane.split",
                    json!({
                        "workspace_id": targets.workspace_id?,
                        "target_pane_id": targets.pane_id?,
                        "direction": direction.as_str(),
                        "focus": true,
                    }),
                )
            }
            Self::ClosePane => focused_pane_request("pane.close", targets)?,
            Self::Zoom => Request::new(
                "pane.zoom",
                json!({"pane_id": targets.pane_id?, "mode": "toggle"}),
            ),
            Self::FocusPane(direction) => directional("pane.focus_direction", direction, targets)?,
            Self::SwapPane(direction) => directional("pane.swap", direction, targets)?,
            Self::ResizePane(direction) => directional("pane.resize", direction, targets)?,
            Self::CyclePane(step) => cycle_pane(snapshot, targets, step)?,
            Self::ClearPane => focused_pane_request("pane.clear", targets)?,
            Self::EditScrollback => focused_pane_request("pane.edit_scrollback", targets)?,
            Self::NewTab => Request::new(
                "tab.create",
                json!({"workspace_id": targets.workspace_id?, "focus": true}),
            ),
            Self::FocusTab(step) => focus_tab(snapshot, targets, step)?,
            Self::MoveTab(step) => move_tab(snapshot, targets, step)?,
            Self::CloseTab => Request::new("tab.close", json!({"tab_id": targets.tab_id?})),
            Self::NewWorkspace => Request::new("workspace.create", json!({"focus": true})),
            // Like Herdr's keybind: also close linked worktree workspaces
            // (`close_group: true`, `record_binding` in actions.rs v0.9.3).
            Self::CloseWorkspace => Request::new(
                "workspace.close",
                json!({"workspace_id": targets.workspace_id?, "close_group": true}),
            ),
            Self::FocusWorkspace(step) => focus_workspace(snapshot, targets, step)?,
            Self::FocusAgent(step) => focus_agent(snapshot, targets, step)?,
            Self::ReloadConfig => Request::without_params("server.reload_config"),
        };
        Some(request)
    }
}

/// `{method} {"pane_id"}` on the target pane.
fn focused_pane_request(method: &str, targets: Targets<'_>) -> Option<Request> {
    Some(Request::new(method, json!({"pane_id": targets.pane_id?})))
}

/// `{method} {"pane_id", "direction"}` on the target pane.
fn directional(method: &str, direction: Direction, targets: Targets<'_>) -> Option<Request> {
    Some(Request::new(
        method,
        json!({"pane_id": targets.pane_id?, "direction": direction.as_str()}),
    ))
}

/// Focuses the pane one `step` away from the target pane in its tab.
fn cycle_pane(snapshot: &Snapshot, targets: Targets<'_>, step: Step) -> Option<Request> {
    let tab = targets.tab_id?;
    let panes: Vec<&str> = snapshot
        .panes
        .iter()
        .filter(|pane| pane.tab_id == tab)
        .map(|pane| pane.pane_id.as_str())
        .collect();
    let pane = cycle(&panes, targets.pane_id, step)?;
    Some(Request::focus_pane(pane))
}

/// Focuses the tab one `step` away from the target tab in its workspace.
fn focus_tab(snapshot: &Snapshot, targets: Targets<'_>, step: Step) -> Option<Request> {
    let tabs = workspace_tabs(snapshot, targets.workspace_id?);
    let tab = cycle(&tabs, targets.tab_id, step)?;
    Some(Request::new("tab.focus", json!({"tab_id": tab})))
}

/// Moves the target tab one `step` within its workspace.
fn move_tab(snapshot: &Snapshot, targets: Targets<'_>, step: Step) -> Option<Request> {
    let tabs = workspace_tabs(snapshot, targets.workspace_id?);
    let tab = targets.tab_id?;
    let source = tabs.iter().position(|id| *id == tab)?;
    let index = move_tab_index(source, tabs.len(), step)?;
    Some(Request::new(
        "tab.move",
        json!({"tab_id": tab, "insert_index": index}),
    ))
}

/// Focuses the workspace one `step` away from the target workspace.
fn focus_workspace(snapshot: &Snapshot, targets: Targets<'_>, step: Step) -> Option<Request> {
    let workspaces: Vec<&str> = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.as_str())
        .collect();
    let workspace = cycle(&workspaces, targets.workspace_id, step)?;
    Some(Request::new(
        "workspace.focus",
        json!({"workspace_id": workspace}),
    ))
}

/// Focuses the agent pane one `step` away from the target pane.
fn focus_agent(snapshot: &Snapshot, targets: Targets<'_>, step: Step) -> Option<Request> {
    let agents: Vec<&str> = snapshot
        .agents
        .iter()
        .map(|agent| agent.pane_id.as_str())
        .collect();
    let pane = cycle(&agents, targets.pane_id, step)?;
    Some(Request::focus_pane(pane))
}

/// Tab ids of `workspace`, in Herdr's order.
fn workspace_tabs<'a>(snapshot: &'a Snapshot, workspace: &str) -> Vec<&'a str> {
    snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace)
        .map(|tab| tab.tab_id.as_str())
        .collect()
}

/// The item one `step` away from `current`, wrapping around.
///
/// When `current` isn't in the list, `Next` gives the first item and
/// `Previous` the last (Herdr does this for agents). `None` for an
/// empty list.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::commands::Step;
/// use herdr_command_palette::domain::requests::builtin::cycle;
///
/// let tabs = ["t1", "t2", "t3"];
/// assert_eq!(cycle(&tabs, Some("t3"), Step::Next), Some("t1"));
/// assert_eq!(cycle(&tabs, Some("t1"), Step::Previous), Some("t3"));
/// assert_eq!(cycle(&tabs, None, Step::Previous), Some("t3"));
/// ```
#[must_use]
pub fn cycle<'a>(items: &[&'a str], current: Option<&str>, step: Step) -> Option<&'a str> {
    let len = items.len();
    if len == 0 {
        return None;
    }
    let position = current.and_then(|current| items.iter().position(|item| *item == current));
    let next = match (position, step) {
        // `+ len - 1` instead of `- 1` keeps the unsigned index from underflowing.
        (Some(index), Step::Previous) => (index + len - 1) % len,
        (Some(index), Step::Next) => (index + 1) % len,
        (None, Step::Previous) => len - 1,
        (None, Step::Next) => 0,
    };
    items.get(next).copied()
}

/// `insert_index` for `tab.move` of the tab at `source` among `count` tabs.
///
/// Herdr counts the insert position before the tab is taken out, so one to
/// the right is `source + 2`. The last tab moving right goes first (0); the
/// first tab moving left goes last (`count`). `None` with fewer than two
/// tabs: there is nothing to move.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::commands::Step;
/// use herdr_command_palette::domain::requests::builtin::move_tab_index;
///
/// assert_eq!(move_tab_index(1, 3, Step::Next), Some(3));
/// assert_eq!(move_tab_index(2, 3, Step::Next), Some(0));
/// assert_eq!(move_tab_index(0, 3, Step::Previous), Some(3));
/// assert_eq!(move_tab_index(0, 1, Step::Next), None);
/// ```
#[must_use]
pub fn move_tab_index(source: usize, count: usize, step: Step) -> Option<usize> {
    if count <= 1 {
        return None;
    }
    let index = match step {
        Step::Next if source + 1 >= count => 0,
        Step::Next => source + 2,
        Step::Previous if source == 0 => count,
        Step::Previous => source - 1,
    };
    Some(index)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::domain::fixtures::herdr_effect;
    use crate::domain::herdr::InvocationContext;
    use crate::domain::requests::builtin_effect;

    use super::*;

    /// A test session with focus on w1, tab w1:t2 (2nd of 3), pane w1:p2.
    fn snapshot() -> Snapshot {
        serde_json::from_value(json!({
            "focused_workspace_id": "w1",
            "focused_tab_id": "w1:t2",
            "focused_pane_id": "w1:p2",
            "workspaces": [{"workspace_id": "w1", "label": "a"},
                           {"workspace_id": "w2", "label": "b"}],
            "tabs": [
                {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
                {"tab_id": "w1:t2", "workspace_id": "w1", "label": "2"},
                {"tab_id": "w1:t3", "workspace_id": "w1", "label": "3"},
                {"tab_id": "w2:t1", "workspace_id": "w2", "label": "1"},
            ],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t2", "cwd": "/a"},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t2", "cwd": "/b"},
            ],
            "agents": [{"pane_id": "w1:p1"}, {"pane_id": "w2:p1"}],
        }))
        .unwrap()
    }

    fn run(name: &str) -> Effect {
        builtin_effect(name, &snapshot(), &InvocationContext::default()).unwrap()
    }

    #[test]
    fn splits_the_focused_pane_and_focuses_the_new_one() {
        assert_eq!(
            run("split_horizontal"),
            herdr_effect(
                "pane.split",
                json!({"workspace_id": "w1", "target_pane_id": "w1:p2",
                       "direction": "down", "focus": true})
            )
        );
        assert_eq!(
            run("split_vertical"),
            herdr_effect(
                "pane.split",
                json!({"workspace_id": "w1", "target_pane_id": "w1:p2",
                       "direction": "right", "focus": true})
            )
        );
    }

    #[test]
    fn pane_actions_target_the_focused_pane() {
        let pane = json!({"pane_id": "w1:p2"});
        assert_eq!(run("close_pane"), herdr_effect("pane.close", pane.clone()));
        assert_eq!(run("clear_pane"), herdr_effect("pane.clear", pane.clone()));
        assert_eq!(
            run("edit_scrollback"),
            herdr_effect("pane.edit_scrollback", pane)
        );
        assert_eq!(
            run("zoom"),
            herdr_effect("pane.zoom", json!({"pane_id": "w1:p2", "mode": "toggle"}))
        );
    }

    #[test]
    fn directional_actions_pass_the_direction() {
        assert_eq!(
            run("swap_pane_up"),
            herdr_effect("pane.swap", json!({"pane_id": "w1:p2", "direction": "up"}))
        );
        assert_eq!(
            run("focus_pane_left"),
            herdr_effect(
                "pane.focus_direction",
                json!({"pane_id": "w1:p2", "direction": "left"})
            )
        );
        assert_eq!(
            run("resize_pane_right"),
            herdr_effect(
                "pane.resize",
                json!({"pane_id": "w1:p2", "direction": "right"})
            )
        );
        assert_eq!(
            run("resize_pane_down"),
            herdr_effect(
                "pane.resize",
                json!({"pane_id": "w1:p2", "direction": "down"})
            )
        );
    }

    #[test]
    fn tab_cycle_wraps_within_the_workspace() {
        assert_eq!(
            run("next_tab"),
            herdr_effect("tab.focus", json!({"tab_id": "w1:t3"}))
        );
        assert_eq!(
            run("previous_tab"),
            herdr_effect("tab.focus", json!({"tab_id": "w1:t1"}))
        );
    }

    #[test]
    fn move_tab_uses_herdrs_insert_index() {
        assert_eq!(
            run("move_tab_next"),
            herdr_effect("tab.move", json!({"tab_id": "w1:t2", "insert_index": 3}))
        );
        assert_eq!(
            run("move_tab_previous"),
            herdr_effect("tab.move", json!({"tab_id": "w1:t2", "insert_index": 0}))
        );
    }

    #[test]
    fn move_tab_does_nothing_with_a_single_tab() {
        let context = InvocationContext {
            workspace_id: Some("w2".into()),
            tab_id: Some("w2:t1".into()),
            ..InvocationContext::default()
        };

        let effect = builtin_effect("move_tab_next", &snapshot(), &context).unwrap();

        assert_eq!(effect, Effect::Nothing);
    }

    #[test]
    fn move_tab_does_nothing_when_the_tab_is_not_in_the_workspace() {
        let context = InvocationContext {
            tab_id: Some("w9:t9".into()),
            ..InvocationContext::default()
        };

        let effect = builtin_effect("move_tab_previous", &snapshot(), &context).unwrap();

        assert_eq!(effect, Effect::Nothing);
    }

    #[test]
    fn move_tab_index_wraps_at_both_ends() {
        assert_eq!(move_tab_index(2, 3, Step::Next), Some(0));
        assert_eq!(move_tab_index(0, 3, Step::Previous), Some(3));
        assert_eq!(move_tab_index(2, 3, Step::Previous), Some(1));
        assert_eq!(move_tab_index(0, 0, Step::Previous), None);
    }

    #[test]
    fn cycle_pane_moves_within_the_tab() {
        assert_eq!(
            run("cycle_pane_next"),
            herdr_effect("pane.focus", json!({"pane_id": "w1:p1"}))
        );
        assert_eq!(
            run("cycle_pane_previous"),
            herdr_effect("pane.focus", json!({"pane_id": "w1:p1"}))
        );
    }

    #[test]
    fn cycle_pane_does_nothing_in_a_tab_without_panes() {
        let context = InvocationContext {
            tab_id: Some("w1:t1".into()),
            ..InvocationContext::default()
        };

        let effect = builtin_effect("cycle_pane_next", &snapshot(), &context).unwrap();

        assert_eq!(effect, Effect::Nothing);
    }

    #[test]
    fn workspace_cycle_wraps() {
        assert_eq!(
            run("next_workspace"),
            herdr_effect("workspace.focus", json!({"workspace_id": "w2"}))
        );
        assert_eq!(
            run("previous_workspace"),
            herdr_effect("workspace.focus", json!({"workspace_id": "w2"}))
        );
    }

    #[test]
    fn agent_cycle_starts_at_first_when_focus_is_not_an_agent() {
        assert_eq!(
            run("next_agent"),
            herdr_effect("pane.focus", json!({"pane_id": "w1:p1"}))
        );
        assert_eq!(
            run("previous_agent"),
            herdr_effect("pane.focus", json!({"pane_id": "w2:p1"}))
        );
    }

    #[test]
    fn agent_cycle_does_nothing_without_agents() {
        let mut snapshot = snapshot();
        snapshot.agents.clear();

        let effect =
            builtin_effect("next_agent", &snapshot, &InvocationContext::default()).unwrap();

        assert_eq!(effect, Effect::Nothing);
    }

    #[test]
    fn tab_and_workspace_cycles_do_nothing_when_empty() {
        let empty = Snapshot {
            focused_workspace_id: Some("w1".into()),
            ..Snapshot::default()
        };
        let context = InvocationContext::default();

        for name in ["next_tab", "next_workspace"] {
            assert_eq!(
                builtin_effect(name, &empty, &context).unwrap(),
                Effect::Nothing
            );
        }
    }

    #[test]
    fn tab_actions_target_the_focused_tab_and_workspace() {
        assert_eq!(
            run("new_tab"),
            herdr_effect("tab.create", json!({"workspace_id": "w1", "focus": true}))
        );
        assert_eq!(
            run("close_tab"),
            herdr_effect("tab.close", json!({"tab_id": "w1:t2"}))
        );
    }

    #[test]
    fn new_workspace_is_focused() {
        assert_eq!(
            run("new_workspace"),
            herdr_effect("workspace.create", json!({"focus": true}))
        );
    }

    #[test]
    fn close_workspace_closes_the_whole_group() {
        assert_eq!(
            run("close_workspace"),
            herdr_effect(
                "workspace.close",
                json!({"workspace_id": "w1", "close_group": true})
            )
        );
    }

    #[test]
    fn reload_config_sends_no_params() {
        assert_eq!(
            run("reload_config"),
            herdr_effect("server.reload_config", json!({}))
        );
    }

    #[test]
    fn context_overrides_snapshot_focus() {
        let context = InvocationContext::from_json(r#"{"focused_pane_id":"w1:p1"}"#);

        let effect = builtin_effect("zoom", &snapshot(), &context).unwrap();

        assert_eq!(
            effect,
            herdr_effect("pane.zoom", json!({"pane_id": "w1:p1", "mode": "toggle"}))
        );
    }

    #[test]
    fn does_nothing_when_a_target_is_missing() {
        let empty = Snapshot::default();
        let context = InvocationContext::default();
        let effect = |name| builtin_effect(name, &empty, &context).unwrap();

        // Herdr's client silently skips these too; the palette mirrors it.
        for name in [
            "zoom",
            "close_tab",
            "new_tab",
            "close_workspace",
            "move_tab_next",
        ] {
            assert_eq!(effect(name), Effect::Nothing, "{name}");
        }
    }

    #[test]
    fn unknown_names_are_errors() {
        let error =
            builtin_effect("detach", &snapshot(), &InvocationContext::default()).unwrap_err();

        assert_eq!(error, RequestError::UnknownBuiltin("detach".into()));
    }

    #[test]
    fn every_listed_builtin_maps_to_a_herdr_request() {
        for builtin in BUILTINS {
            assert!(
                matches!(run(builtin.name), Effect::Herdr(_)),
                "{}",
                builtin.name
            );
        }
    }

    #[test]
    fn every_listed_builtin_name_parses_back_to_its_action() {
        for builtin in BUILTINS {
            assert_eq!(builtin.name.parse::<Builtin>(), Ok(builtin.action));
        }
    }
}
