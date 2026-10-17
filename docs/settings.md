# Settings

Every setting the palette reads, with its default. For what the palette
does, see the [README](../README.md).

All settings are optional. They live in `settings.toml` in the plugin's
configuration directory. Changes apply the next time the palette opens.
To find the directory, run:

```sh
herdr plugin config-dir mmc.command-palette # e.g. ~/.config/herdr/plugins/config/mmc.command-palette
```

## Create the settings file

The plugin uses default settings when the file is absent. Run **Edit
Command Palette settings** to create it and open it in your editor. The
generated file explains each setting and shows its default value in a
comment. Existing files are preserved.

When the editor exits, the plugin validates the file and reports any
errors. Unknown keys and invalid values are errors: when you open the
palette, it shows them on an error screen instead; press `e` to edit
the file. To validate it manually, run:

```sh
herdr-command-palette settings check
```

Add `--file <PATH>` to check another file.

## Example

```toml
editor = "code -w"
open_on = "jump_to"

[commands]
hide_kinds = ["custom"]
confirm_ids = ["builtin:close_workspace", "builtin:close_tab"]

[popup]
max_width = 100
max_height = 30

[colors]
matches = "#f9e2af"
branch = 141

[history]
max_age_days = 30
```

## Top level

| Key       | Default      | What it does                                                                                                                                                   |
| --------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `editor`  | `""`         | Editor for **Edit Command Palette settings** and **Edit Herdr config**. An empty value uses `$EDITOR`, then `vi`. Arguments are supported, such as `"code -w"` |
| `open_on` | `"commands"` | Initial category: `"commands"` or `"jump_to"`                                                                                                                  |

## `[commands]`

| Key               | Default                       | What it does                                                                                                  |
| ----------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `order`           | `"recent"`                    | `"recent"` (last run first, then the fixed order) or `"fixed"` (plugin, built-in, palette, custom commands)   |
| `hide_kinds`      | `[]`                          | Hide command categories: `"plugin"`, `"builtin"`, `"palette"`, `"custom"`                                     |
| `hide_ids`        | `[]`                          | Hide individual commands ([ids](#command-ids))                                                                |
| `confirm_ids`     | `["builtin:close_workspace"]` | Require confirmation for these commands ([ids](#command-ids), [dialog](#confirmation-dialog))                 |
| `skip_recent_ids` | `["builtin:reload_config"]`   | Keep these commands in their fixed positions ([ids](#command-ids)). They remain available to `repeat-command` |

## `[jump_to]`

| Key     | Default    | What it does                                                                                         |
| ------- | ---------- | ---------------------------------------------------------------------------------------------------- |
| `order` | `"recent"` | `"recent"` (last visited first, current pane last) or `"herdr"` (Herdr's workspace, tab, pane order) |

## `[popup]`

| Key                         | Default     | What it does         |
| --------------------------- | ----------- | -------------------- |
| `min_width` / `max_width`   | `76` / `76` | Popup width, columns |
| `min_height` / `max_height` | `16` / `22` | Popup height, rows   |

The popup's size is computed when the palette opens. It uses the
maximum, shrunk on small screens to keep a margin of 2 columns and 1
row, but never below the minimum unless the screen itself is smaller.
Values below the defaults are raised to the defaults. Each minimum must
be at most its maximum.

## `[colors]`

The palette uses your Herdr theme colors. Set these values to override
individual colors.

| Key          | Default | What it does                                                     | Theme color used when empty                 |
| ------------ | ------- | ---------------------------------------------------------------- | ------------------------------------------- |
| `background` | `""`    | Popup and error screen background                                | `panel_bg`                                  |
| `muted`      | `""`    | Footer, shortcuts and inactive categories                        | `overlay0` for the footer, dim for the rest |
| `matches`    | `""`    | Characters matching your search (underlined, bold when selected) | The row's own color                         |
| `selected`   | `""`    | Selected line's text (it stays bold)                             | `text`                                      |
| `dividers`   | `""`    | Lines around the search and footer                               | Dim                                         |
| `tags`       | `""`    | Footer tag (`plugin`, `palette`, `custom`)                       | `subtext0`                                  |
| `branch`     | `""`    | Git branch in Jump to's footer (needs git 2.22 or newer)         | `mauve`                                     |

Colors accept `"#rrggbb"`, an unquoted ANSI color number from `0` to
`255`, or `"default"` for the terminal's color. An empty string uses the
theme's color.

If you set `background`, keep it equal to your Herdr theme's `panel_bg`
color. Otherwise, the column Herdr reserves along the popup's right edge
shows as a visible stripe.

## `[history]`

| Key            | Default | What it does                                                                |
| -------------- | ------- | --------------------------------------------------------------------------- |
| `enabled`      | `true`  | Record command usage and pane visits for recent ordering and repeat actions |
| `max_age_days` | `90`    | History retention in days. Set `0` to retain entries indefinitely           |

With `enabled = false`, nothing is recorded, the `"recent"` orders fall
back to `"fixed"` and `"herdr"`, and the repeat actions report that
history is off. Old entries are removed after the palette opens, at most
once an hour.

## Confirmation dialog

Use `confirm_ids` in `[commands]` to choose which commands require
confirmation ([command ids](#command-ids)). In the dialog, Enter runs
the command and Esc cancels it.

When Herdr's `[ui] confirm_close` is enabled (the default), closing a
workspace's last tab or a worktree group's last pane always requires
confirmation, matching Herdr's key bindings.

## Command ids

`hide_ids`, `confirm_ids` and `skip_recent_ids` name commands as
`<kind>:<name>`. To list every command's id:

```sh
herdr-command-palette list --ids
```

| Kind      | Name                                                                                                            | Example                   |
| --------- | --------------------------------------------------------------------------------------------------------------- | ------------------------- |
| `plugin`  | plugin action id                                                                                                | `plugin:wtherdr.merge`    |
| `builtin` | Herdr action name                                                                                               | `builtin:close_workspace` |
| `palette` | palette command name                                                                                            | `palette:edit_settings`   |
| `custom`  | The `[[keys.command]]` entry's `description`, else its `command`, else its `type` (empty values count as unset) | `custom:Popup: lazygit`   |

Unknown `builtin` and `palette` names are errors. Custom commands with
the same description share an id. A custom command with an empty or
missing `command` is listed but fails when run.
