//! What the palette's named commands are: the actions behind built-in and
//! palette-command rows, as closed enums.
//!
//! Shared by [`catalog`](crate::domain::catalog) (which lists them, in the
//! [`NamedCommand`](crate::domain::catalog::NamedCommand) tables) and
//! [`requests`](crate::domain::requests) (which maps each one to an
//! [`Effect`](crate::domain::requests::Effect), parsing names through the
//! catalog's tables). Both depend on this module and it on neither, so
//! the dependency between them is one-way: `requests` -> `catalog`.

/// A direction for focus, swap and resize (Herdr's `PaneDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Towards the left edge.
    Left,
    /// Towards the bottom edge.
    Down,
    /// Towards the top edge.
    Up,
    /// Towards the right edge.
    Right,
}

impl Direction {
    /// The name Herdr's API uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Down => "down",
            Self::Up => "up",
            Self::Right => "right",
        }
    }
}

/// Which way a cycling action moves through a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// One back, wrapping from the first to the last item.
    Previous,
    /// One forward, wrapping from the last to the first item.
    Next,
}

/// A Herdr key action the palette can run (`[keys]` names in `config.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// `split_vertical`: split the focused pane, new pane on the right.
    SplitVertical,
    /// `split_horizontal`: split the focused pane, new pane below.
    SplitHorizontal,
    /// `close_pane`.
    ClosePane,
    /// `zoom`: toggle zoom of the focused pane.
    Zoom,
    /// `focus_pane_<direction>`.
    FocusPane(Direction),
    /// `swap_pane_<direction>`.
    SwapPane(Direction),
    /// `resize_pane_<direction>`.
    ResizePane(Direction),
    /// `cycle_pane_next` / `cycle_pane_previous`: panes of the current tab.
    CyclePane(Step),
    /// `clear_pane`.
    ClearPane,
    /// `edit_scrollback`.
    EditScrollback,
    /// `new_tab`.
    NewTab,
    /// `previous_tab` / `next_tab`: tabs of the current workspace.
    FocusTab(Step),
    /// `move_tab_previous` / `move_tab_next`.
    MoveTab(Step),
    /// `close_tab`.
    CloseTab,
    /// `new_workspace`.
    NewWorkspace,
    /// `close_workspace`.
    CloseWorkspace,
    /// `previous_workspace` / `next_workspace`.
    FocusWorkspace(Step),
    /// `previous_agent` / `next_agent`: panes running an agent.
    FocusAgent(Step),
    /// `reload_config`.
    ReloadConfig,
}

/// The palette's own commands, for things no Herdr key action does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    /// `move_pane_new_tab`: focused pane into a new tab, same workspace.
    MovePaneNewTab,
    /// `move_pane_new_workspace`: focused pane into a new workspace.
    MovePaneNewWorkspace,
    /// `edit_herdr_config`: the editor on `config.toml`, then check and reload.
    EditHerdrConfig,
    /// `edit_settings`: the editor on the plugin's `settings.toml` (written
    /// with every key commented on first use), then check it.
    EditSettings,
}
