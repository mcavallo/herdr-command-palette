//! Pane names identical to Herdr's "Go to" list, so a pane is
//! called the same in the palette and in Herdr.
//!
//! Mirrors `navigator_rows` in Herdr `src/client/shell/aggregate_navigation.rs`,
//! v0.9.3. One difference Herdr's socket forces on us: its `TabInfo` has no
//! `custom_label`, so a tab the user renamed to a number counts as unnamed.

use std::collections::HashMap;

use crate::domain::herdr::{Agent, Snapshot};

/// What a pane running no detected agent is called: its name in Jump to
/// labels and its right column (Herdr's "Go to" says `terminal` too).
pub(crate) const NO_AGENT: &str = "terminal";

/// Pane id -> the name Herdr's Go to shows for it.
///
/// In order of preference:
///
/// - Alone in its tab: pane label or agent name, else the tab name, else
///   the agent title; else `<agent kind or terminal> · <tab label>` when the
///   workspace has several tabs, or the workspace label when it has one.
/// - Sharing its tab: pane label or agent name, else agent title, else
///   agent kind, else `terminal`; then `· <position in tab>`, prefixed with
///   `<tab name> · ` if the tab is named and the name differs.
///
/// A tab is named when its label isn't empty or just its number. Panes
/// whose tab or workspace isn't in the snapshot get no name.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::herdr::Snapshot;
/// use herdr_command_palette::domain::jump_to::pane_labels;
///
/// let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
///     "workspaces": [{"workspace_id": "w1", "label": "dotfiles"}],
///     "tabs": [{"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"}],
///     "panes": [
///         {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"},
///         {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1", "label": "logs"},
///     ],
/// }))
/// .unwrap();
///
/// let labels = pane_labels(&snapshot);
/// assert_eq!(labels["w1:p1"], "terminal · 1");
/// assert_eq!(labels["w1:p2"], "logs · 2");
/// ```
#[must_use]
pub fn pane_labels(snapshot: &Snapshot) -> HashMap<String, String> {
    let agents: HashMap<&str, &Agent> = snapshot
        .agents
        .iter()
        .map(|agent| (agent.pane_id.as_str(), agent))
        .collect();
    let mut labels = HashMap::new();
    for workspace in &snapshot.workspaces {
        let tabs: Vec<_> = snapshot
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace.workspace_id)
            .collect();
        let multiple_tabs = tabs.len() > 1;
        for tab in tabs {
            let panes: Vec<_> = snapshot
                .panes
                .iter()
                .filter(|pane| pane.tab_id == tab.tab_id)
                .collect();
            let tab_name = tab_name(&tab.label);
            for (index, pane) in panes.iter().enumerate() {
                let agent = agents.get(pane.pane_id.as_str()).copied();
                let kind = agent.and_then(Agent::kind);
                let name = pane
                    .label
                    .as_deref()
                    .or_else(|| agent.and_then(|agent| agent.name.as_deref()));
                let title = agent.and_then(|agent| {
                    agent
                        .title
                        .as_deref()
                        .or(agent.terminal_title_stripped.as_deref())
                });
                let label = if panes.len() == 1 {
                    match name.or(tab_name).or(title) {
                        Some(label) => label.to_owned(),
                        None if multiple_tabs => {
                            format!("{} · {}", kind.unwrap_or(NO_AGENT), tab.label)
                        }
                        None => workspace.label.clone(),
                    }
                } else {
                    let pane_name = name.or(title).or(kind).unwrap_or(NO_AGENT);
                    let position = index + 1;
                    match tab_name {
                        Some(tab_name) if tab_name != pane_name => {
                            format!("{tab_name} · {pane_name} · {position}")
                        }
                        _ => format!("{pane_name} · {position}"),
                    }
                };
                labels.insert(pane.pane_id.clone(), label);
            }
        }
    }
    labels
}

/// The tab's name, `None` for an unnamed tab: Herdr labels those with
/// their number. Empty labels also count as unnamed.
fn tab_name(label: &str) -> Option<&str> {
    let is_number = label.chars().all(|c| c.is_ascii_digit());
    (!label.is_empty() && !is_number).then_some(label)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::domain::fixtures::goto_snapshot;

    use super::*;

    fn labels(json: serde_json::Value) -> HashMap<String, String> {
        pane_labels(&serde_json::from_value(json).unwrap())
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect()
    }

    #[test]
    fn names_panes_like_herdr_go_to() {
        let labels = pane_labels(&goto_snapshot());

        assert_eq!(
            labels,
            map(&[
                ("w1:p1", "terminal · 1"),
                ("w1:p2", "potatoes · 2"),
                ("w1:p3", "fix · 3"),
                // Single pane, unnamed tab, several tabs.
                ("w2:p1", "codex · 1"),
                // Single pane: the named tab wins.
                ("w2:p2", "logs"),
            ])
        );
    }

    #[test]
    fn a_tab_is_unnamed_when_its_label_is_digits_or_empty() {
        assert_eq!(tab_name("3"), None);
        assert_eq!(tab_name("99999999999999999999999"), None);
        assert_eq!(tab_name(""), None);
        assert_eq!(tab_name("+1"), Some("+1"));
        assert_eq!(tab_name("logs"), Some("logs"));
    }

    #[test]
    fn names_a_lone_unnamed_pane_after_its_workspace() {
        let labels = labels(json!({
            "workspaces": [{"workspace_id": "w1", "label": "dotfiles"}],
            "tabs": [{"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"}],
            "panes": [{"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"}],
        }));

        assert_eq!(labels, map(&[("w1:p1", "dotfiles")]));
    }

    #[test]
    fn prefers_name_over_tab_and_title_for_a_lone_pane() {
        let labels = labels(json!({
            "workspaces": [{"workspace_id": "w1", "label": "dotfiles"}],
            "tabs": [
                {"tab_id": "w1:t1", "workspace_id": "w1", "label": "build"},
                {"tab_id": "w1:t2", "workspace_id": "w1", "label": "2"},
            ],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t2"},
            ],
            "agents": [
                {"pane_id": "w1:p1", "name": "session", "title": "ignored"},
                {"pane_id": "w1:p2", "display_agent": "pi", "title": "refactor"},
            ],
        }));

        assert_eq!(labels, map(&[("w1:p1", "session"), ("w1:p2", "refactor")]));
    }

    #[test]
    fn prefixes_shared_tab_panes_with_a_differing_tab_name() {
        let labels = labels(json!({
            "workspaces": [{"workspace_id": "w1", "label": "dotfiles"}],
            "tabs": [{"tab_id": "w1:t1", "workspace_id": "w1", "label": "build"}],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1", "label": "build"},
            ],
        }));

        assert_eq!(
            labels,
            map(&[("w1:p1", "build · terminal · 1"), ("w1:p2", "build · 2")])
        );
    }
}
