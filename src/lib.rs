//! A keyboard-driven command palette for [Herdr](https://herdr.dev).
//!
//! The crate is organised as ports and adapters (hexagonal architecture):
//!
//! - [`domain`]: pure logic (rows, ordering, layout); no I/O.
//! - [`ports`]: traits the use cases need from the outside world.
//! - [`adapters`]: implementations of those traits (Herdr socket, SQLite,
//!   fzf, settings files).
//! - [`app`]: use cases, one per subcommand, written against the ports.
//! - [`cli`]: argument parsing; `main.rs` wires adapters into the app.
//!
//! See `docs/architecture.md` for diagrams and the reasoning behind it.

pub mod adapters;
pub mod app;
pub mod cli;
pub mod domain;
pub mod ports;

pub use app::{Error, run};
