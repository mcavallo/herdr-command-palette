//! The "recent" orders of the settings' `[commands] order` and
//! `[jump_to] order`: rows the history knows
//! move to the front, newest first; the rest keep their fixed (Herdr)
//! order.
//!
//! History comes in as keys, newest first, as the store returns them;
//! keys of rows that no longer exist are simply never matched. Reading the
//! store and the settings is the caller's job.

use std::collections::HashMap;
use std::hash::Hash;

use crate::domain::row::{Category, Kind, Row, RowId};

/// Puts the Commands rows whose [`RowId`] `key` is in `recent` (newest
/// first) before the others, which keep their order. Jump to rows don't
/// move.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::recent;
/// use herdr_command_palette::domain::row::{Kind, Row, RowId};
///
/// let mut rows = vec![
///     Row::new(Kind::Builtin, "zoom", "Zoom"),
///     Row::new(Kind::Builtin, "split", "Split"),
/// ];
/// let recent = [RowId::new(Kind::Builtin, "split")];
///
/// recent::order_commands(&mut rows, &recent, |row| Some(row.id.clone()));
///
/// assert_eq!(rows[0].payload, "split");
/// ```
pub fn order_commands(rows: &mut [Row], recent: &[RowId], key: impl Fn(&Row) -> Option<RowId>) {
    let positions = positions(recent);
    rows.sort_by_cached_key(|row| {
        let rank = match row.category() {
            Category::Commands => key(row)
                .and_then(|key| positions.get(&key).copied())
                .map_or(Rank::Rest, Rank::Recent),
            Category::JumpTo => Rank::Rest,
        };
        (row.category(), rank)
    });
}

/// Puts the Jump to rows of panes in `focused` (newest first) before the
/// others, which keep Herdr's order, and the `current` pane last: jumping
/// to it does nothing. Commands rows don't move.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::recent;
/// use herdr_command_palette::domain::row::{Kind, Row};
///
/// let mut rows = vec![
///     Row::new(Kind::Pane, "w1:p1", "one"),
///     Row::new(Kind::Pane, "w1:p2", "two"),
///     Row::new(Kind::Pane, "w1:p3", "three"),
/// ];
/// let focused = ["w1:p1".to_owned(), "w1:p3".to_owned()];
///
/// recent::order_panes(&mut rows, &focused, Some("w1:p1"));
///
/// let order: Vec<_> = rows.iter().map(|row| row.payload.as_str()).collect();
/// assert_eq!(order, ["w1:p3", "w1:p2", "w1:p1"]);
/// ```
pub fn order_panes(rows: &mut [Row], focused: &[String], current: Option<&str>) {
    let positions = positions(focused);
    rows.sort_by_cached_key(|row| {
        let rank = if row.kind != Kind::Pane {
            Rank::Rest
        } else if Some(row.payload.as_str()) == current {
            Rank::Last
        } else {
            positions
                .get(&row.payload)
                .copied()
                .map_or(Rank::Rest, Rank::Recent)
        };
        (row.category(), rank)
    });
}

/// Where a row goes within its category. The derived `Ord` follows the
/// variant order, so `Recent` rows come first, by position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// In the history, at this position (0 = newest).
    Recent(usize),
    /// Not in the history: fixed order (sorting is stable).
    Rest,
    /// The current pane.
    Last,
}

/// Each key's position in `keys`; the first one wins for duplicates.
fn positions<K: Clone + Eq + Hash>(keys: &[K]) -> HashMap<K, usize> {
    let mut positions = HashMap::new();
    for (index, key) in keys.iter().enumerate() {
        positions.entry(key.clone()).or_insert(index);
    }
    positions
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proptest::sample;

    use super::*;

    fn payloads(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.payload.as_str()).collect()
    }

    fn commands_and_panes() -> Vec<Row> {
        vec![
            Row::new(Kind::Plugin, "demo.hello", "Hello"),
            Row::new(Kind::Builtin, "zoom", "Zoom"),
            Row::new(Kind::Builtin, "split", "Split"),
            Row::new(Kind::Custom, "0", "Lazygit"),
            Row::new(Kind::Pane, "w1:p1", "one"),
            Row::new(Kind::Pane, "w1:p2", "two"),
            Row::new(Kind::Pane, "w2:p1", "three"),
        ]
    }

    #[test]
    fn recent_commands_come_first_newest_first() {
        let mut rows = commands_and_panes();
        let recent = [
            RowId::new(Kind::Custom, "0"),
            RowId::new(Kind::Builtin, "zoom"),
        ];

        order_commands(&mut rows, &recent, |row| Some(row.id.clone()));

        assert_eq!(
            payloads(&rows),
            [
                "0",
                "zoom",
                "demo.hello",
                "split",
                "w1:p1",
                "w1:p2",
                "w2:p1"
            ]
        );
    }

    #[test]
    fn commands_match_history_by_the_given_key() {
        let mut rows = commands_and_panes();
        let recent = [RowId::new(Kind::Custom, "hash-of-lazygit")];

        order_commands(&mut rows, &recent, |row| {
            (row.kind == Kind::Custom).then(|| RowId::new(Kind::Custom, "hash-of-lazygit"))
        });

        assert_eq!(payloads(&rows)[0], "0");
    }

    #[test]
    fn unknown_command_keys_change_nothing() {
        let mut rows = commands_and_panes();
        let recent = [
            RowId::new(Kind::Builtin, "gone"),
            RowId::new(Kind::Pane, "w1:p2"),
        ];

        order_commands(&mut rows, &recent, |row| Some(row.id.clone()));

        assert_eq!(payloads(&rows), payloads(&commands_and_panes()));
    }

    #[test]
    fn recently_focused_panes_come_first_and_the_current_one_last() {
        let mut rows = commands_and_panes();
        let focused = ["w1:p1".to_owned(), "w2:p1".to_owned(), "w9:p9".to_owned()];

        order_panes(&mut rows, &focused, Some("w1:p1"));

        assert_eq!(
            payloads(&rows),
            [
                "demo.hello",
                "zoom",
                "split",
                "0",
                "w2:p1",
                "w1:p2",
                "w1:p1"
            ]
        );
    }

    #[test]
    fn panes_without_history_keep_herdr_order() {
        let mut rows = commands_and_panes();

        order_panes(&mut rows, &[], None);

        assert_eq!(payloads(&rows), payloads(&commands_and_panes()));
    }

    #[test]
    fn the_first_of_duplicate_keys_wins() {
        let positions = positions(&["a", "b", "a"]);

        assert_eq!(positions["a"], 0);
        assert_eq!(positions["b"], 1);
    }

    // Property tests: random catalogs (every kind, shuffled, so categories
    // may interleave on input) and random history keys (duplicates, keys
    // of other categories, keys of rows that don't exist).

    const NAMES: [&str; 3] = ["a", "b", "c"];
    const PANES: [&str; 4] = ["w1:p1", "w1:p2", "w2:p1", "w2:p2"];

    /// One row per kind and name: unique ids, as a real catalog has.
    fn candidates() -> Vec<Row> {
        let commands = [Kind::Plugin, Kind::Builtin, Kind::Palette, Kind::Custom]
            .into_iter()
            .flat_map(|kind| NAMES.map(|name| Row::new(kind, name, name)));
        let panes = PANES.map(|pane| Row::new(Kind::Pane, pane, pane));
        commands.chain(panes).collect()
    }

    fn catalog() -> impl Strategy<Value = Vec<Row>> {
        let all = candidates();
        let len = all.len();
        sample::subsequence(all, 0..=len).prop_shuffle()
    }

    fn command_history() -> impl Strategy<Value = Vec<RowId>> {
        let mut keys: Vec<RowId> = candidates().into_iter().map(|row| row.id).collect();
        keys.push(RowId::new(Kind::Builtin, "gone"));
        proptest::collection::vec(sample::select(keys), 0..12)
    }

    fn pane_ids() -> Vec<String> {
        PANES
            .into_iter()
            .chain(["w9:p9"])
            .map(str::to_owned)
            .collect()
    }

    fn focus_history() -> impl Strategy<Value = Vec<String>> {
        proptest::collection::vec(sample::select(pane_ids()), 0..8)
    }

    fn current_pane() -> impl Strategy<Value = Option<String>> {
        proptest::option::of(sample::select(pane_ids()))
    }

    fn ids<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Vec<String> {
        rows.into_iter().map(|row| row.id.to_string()).collect()
    }

    fn sorted_ids(rows: &[Row]) -> Vec<String> {
        let mut ids = ids(rows);
        ids.sort();
        ids
    }

    /// The ids of `rows` that `keep` accepts, in order.
    fn ids_where(rows: &[Row], keep: impl Fn(&Row) -> bool) -> Vec<String> {
        ids(rows.iter().filter(|row| keep(row)))
    }

    fn categories_are_grouped(rows: &[Row]) -> bool {
        rows.windows(2)
            .all(|pair| pair[0].category() <= pair[1].category())
    }

    proptest! {
        #[test]
        fn order_commands_is_a_grouped_permutation(
            rows in catalog(), recent in command_history(),
        ) {
            let mut ordered = rows.clone();

            order_commands(&mut ordered, &recent, |row| Some(row.id.clone()));

            prop_assert_eq!(sorted_ids(&ordered), sorted_ids(&rows));
            prop_assert!(categories_are_grouped(&ordered), "{:?}", ids(&ordered));
        }

        #[test]
        fn order_commands_moves_history_rows_only(
            rows in catalog(), recent in command_history(),
        ) {
            let mut ordered = rows.clone();

            order_commands(&mut ordered, &recent, |row| Some(row.id.clone()));

            let stays = |row: &Row| {
                row.category() == Category::JumpTo || !recent.contains(&row.id)
            };
            for category in [Category::Commands, Category::JumpTo] {
                let in_rest = |row: &Row| row.category() == category && stays(row);
                prop_assert_eq!(ids_where(&ordered, in_rest), ids_where(&rows, in_rest));
            }
            // History rows: newest first (a duplicate counts where it is
            // newest), before the rest of Commands.
            let mut expected: Vec<String> = Vec::new();
            for key in &recent {
                let is_command = rows
                    .iter()
                    .any(|row| row.category() == Category::Commands && &row.id == key);
                if is_command && !expected.contains(&key.to_string()) {
                    expected.push(key.to_string());
                }
            }
            prop_assert_eq!(ids(ordered.iter().take_while(|row| !stays(row))), expected);
        }

        #[test]
        fn order_panes_is_a_grouped_permutation(
            rows in catalog(), focused in focus_history(), current in current_pane(),
        ) {
            let mut ordered = rows.clone();

            order_panes(&mut ordered, &focused, current.as_deref());

            prop_assert_eq!(sorted_ids(&ordered), sorted_ids(&rows));
            prop_assert!(categories_are_grouped(&ordered), "{:?}", ids(&ordered));
        }

        #[test]
        fn order_panes_keeps_the_rest_in_order_and_the_current_pane_last(
            rows in catalog(), focused in focus_history(), current in current_pane(),
        ) {
            let mut ordered = rows.clone();

            order_panes(&mut ordered, &focused, current.as_deref());

            let is_current = |row: &Row| Some(row.payload.as_str()) == current.as_deref();
            let commands = |row: &Row| row.category() == Category::Commands;
            prop_assert_eq!(ids_where(&ordered, commands), ids_where(&rows, commands));
            let rest = |row: &Row| {
                row.kind == Kind::Pane && !is_current(row) && !focused.contains(&row.payload)
            };
            prop_assert_eq!(ids_where(&ordered, rest), ids_where(&rows, rest));
            if rows.iter().any(is_current) {
                prop_assert!(ordered.last().is_some_and(is_current), "{:?}", ids(&ordered));
            }
        }
    }
}
