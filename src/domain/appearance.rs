//! The terminal's light/dark appearance, the way Herdr decides it for
//! `[theme] auto_switch`.
//!
//! Herdr's client asks the host terminal with `CSI ?996n` (color scheme
//! report) and `OSC 11` (background color). An explicit report wins; else
//! the background's luminance decides; unknown means dark. Inside a Herdr
//! pane both queries are answered by Herdr from the host's values, so the
//! palette asks the same questions and gets the answers Herdr's chrome
//! uses.
//!
//! This module only builds the query and reads the replies; talking to the
//! terminal is the [`Terminal`](crate::ports::Terminal) adapter's job.

use std::fmt;
use std::str::FromStr;

/// Light or dark, as Herdr's `HostAppearance` (Herdr
/// `src/terminal_theme.rs`, v0.9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    /// A dark background (Herdr's `dark_name` slot).
    Dark,
    /// A light background (Herdr's `light_name` slot).
    Light,
}

impl Appearance {
    /// The appearance of a background color: light when its luminance
    /// `299r + 587g + 114b` is at least 128 000 (Herdr
    /// `RgbColor::inferred_appearance`, `src/terminal_theme.rs`, v0.9.3).
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::appearance::Appearance;
    ///
    /// assert_eq!(Appearance::from_background(0xfd, 0xf6, 0xe3), Appearance::Light);
    /// assert_eq!(Appearance::from_background(0x1e, 0x1e, 0x2e), Appearance::Dark);
    /// ```
    #[must_use]
    pub fn from_background(r: u8, g: u8, b: u8) -> Self {
        let luminance = u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114;
        if luminance >= 128_000 {
            Self::Light
        } else {
            Self::Dark
        }
    }

    /// `dark` or `light`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

impl fmt::Display for Appearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A value that isn't `dark` or `light`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown appearance `{0}` (expected `dark` or `light`)")]
pub struct UnknownAppearance(pub String);

impl FromStr for Appearance {
    type Err = UnknownAppearance;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            _ => Err(UnknownAppearance(text.to_owned())),
        }
    }
}

/// What the palette writes to the terminal: the color scheme query
/// (`CSI ?996n`), the background query (`OSC 11`) and, last, Primary
/// Device Attributes (`CSI c`). Every terminal answers the last one, and
/// answers in order, so its reply means no other reply is coming
/// ([`is_complete`]).
pub const QUERY: &[u8] = b"\x1b[?996n\x1b]11;?\x1b\\\x1b[c";

/// Whether `replies` ends the exchange: it holds the Device Attributes
/// reply (`ESC [ ? <digits and ;> c`).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::appearance::is_complete;
///
/// assert!(is_complete(b"\x1b[?997;1n\x1b[?62;22c"));
/// assert!(!is_complete(b"\x1b[?997;1n"));
/// ```
#[must_use]
pub fn is_complete(replies: &[u8]) -> bool {
    let text = String::from_utf8_lossy(replies);
    text.split("\x1b[?").skip(1).any(|reply| {
        reply
            .split_once('c')
            .is_some_and(|(params, _)| params.chars().all(|c| c.is_ascii_digit() || c == ';'))
    })
}

/// The appearance the terminal's `replies` to [`QUERY`] report, as
/// Herdr's client decides it: a color scheme report (`CSI ?997;1n` dark,
/// `CSI ?997;2n` light) wins; else the `OSC 11` background's luminance
/// ([`Appearance::from_background`]); else `None`.
///
/// Mirrors `HostColorSchemeChanged` / `HostDefaultColor` handling in
/// Herdr `src/client/shell/input.rs` and `src/raw_input.rs`, v0.9.3.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::appearance::{Appearance, from_replies};
///
/// let replies = b"\x1b[?997;2n\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\x1b[?62;22c";
/// assert_eq!(from_replies(replies), Some(Appearance::Light));
/// assert_eq!(from_replies(b"\x1b]11;#fdf6e3\x07"), Some(Appearance::Light));
/// assert_eq!(from_replies(b"\x1b[?62;22c"), None);
/// ```
#[must_use]
pub fn from_replies(replies: &[u8]) -> Option<Appearance> {
    let text = String::from_utf8_lossy(replies);
    if text.contains("\x1b[?997;1n") {
        return Some(Appearance::Dark);
    }
    if text.contains("\x1b[?997;2n") {
        return Some(Appearance::Light);
    }
    let (_, rest) = text.split_once("\x1b]11;")?;
    // OSC replies end with ST (`ESC \`) or BEL.
    let end = rest.find(['\x1b', '\x07'])?;
    let (r, g, b) = parse_rgb(rest.get(..end)?)?;
    Some(Appearance::from_background(r, g, b))
}

/// An X11 color spec as terminals report it: `rgb:r/g/b` (1 to 4 hex digits
/// each) or `#rgb` with 1 to 4 digits per component.
///
/// Mirrors `parse_rgb_color` in Herdr `src/terminal_theme.rs`, v0.9.3.
fn parse_rgb(value: &str) -> Option<(u8, u8, u8)> {
    if let Some(rgb) = value.strip_prefix("rgb:") {
        let parts: Vec<&str> = rgb.split('/').collect();
        return match parts.as_slice() {
            [r, g, b] => Some((component(r)?, component(g)?, component(b)?)),
            _ => None,
        };
    }
    let hex = value.strip_prefix('#')?;
    // Checked first, so the byte slicing below is on ASCII digits.
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digits = hex.len() / 3;
    if !(1..=4).contains(&digits) || hex.len() != digits * 3 {
        return None;
    }
    Some((
        component(hex.get(..digits)?)?,
        component(hex.get(digits..digits * 2)?)?,
        component(hex.get(digits * 2..)?)?,
    ))
}

/// One component, 1 to 4 hex digits, scaled to 0 to 255 with rounding (`f` and
/// `ffff` are both 255, `8000` is 128).
fn component(digits: &str) -> Option<u8> {
    if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    // `len` is 1 to 4, so `max` is 0xf..=0xffff.
    let max = (1_u32 << (digits.len() * 4)) - 1;
    u8::try_from((value * 255 + max / 2) / max).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luminance_threshold_matches_herdr() {
        // 299*128 + 587*128 + 114*128 = 128 000: exactly the threshold.
        assert_eq!(
            Appearance::from_background(128, 128, 128),
            Appearance::Light
        );
        assert_eq!(Appearance::from_background(127, 128, 128), Appearance::Dark);
        assert_eq!(Appearance::from_background(0, 0, 0), Appearance::Dark);
        assert_eq!(
            Appearance::from_background(255, 255, 255),
            Appearance::Light
        );
    }

    #[test]
    fn a_color_scheme_report_wins_over_the_background() {
        let dark = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b[?997;1n";
        let light = b"\x1b]11;rgb:0000/0000/0000\x1b\\\x1b[?997;2n";

        assert_eq!(from_replies(dark), Some(Appearance::Dark));
        assert_eq!(from_replies(light), Some(Appearance::Light));
    }

    #[test]
    fn other_color_scheme_values_are_ignored() {
        assert_eq!(from_replies(b"\x1b[?997;0n\x1b[?997;3n"), None);
        assert_eq!(
            from_replies(b"\x1b[?997;3n\x1b]11;rgb:ff/ff/ff\x07"),
            Some(Appearance::Light)
        );
    }

    #[test]
    fn reads_the_background_in_every_spec_form() {
        let appearance = |reply: &str| from_replies(reply.as_bytes());

        assert_eq!(
            appearance("\x1b]11;rgb:fdfd/f6f6/e3e3\x1b\\"),
            Some(Appearance::Light)
        );
        assert_eq!(appearance("\x1b]11;rgb:1/1/2\x07"), Some(Appearance::Dark));
        assert_eq!(appearance("\x1b]11;#fff\x07"), Some(Appearance::Light));
        assert_eq!(appearance("\x1b]11;#000000000\x07"), Some(Appearance::Dark));
        assert_eq!(
            appearance("\x1b]11;#ffffffffffff\x07"),
            Some(Appearance::Light)
        );
    }

    #[test]
    fn bad_background_specs_give_no_appearance() {
        for reply in [
            "\x1b]11;rgb:ff/ff\x07",
            "\x1b]11;rgb:ff/ff/ff/ff\x07",
            "\x1b]11;rgb:ff/ff/fffff\x07",
            "\x1b]11;rgb:ff/ff/\x07",
            "\x1b]11;rgb:gg/ff/ff\x07",
            "\x1b]11;#ffff\x07",
            "\x1b]11;#fffffffffffffff\x07",
            "\x1b]11;#ggg\x07",
            "\x1b]11;#日本語\x07",
            "\x1b]11;red\x07",
            // Not terminated: cut off by the timeout.
            "\x1b]11;rgb:ff/ff/ff",
        ] {
            assert_eq!(from_replies(reply.as_bytes()), None, "{reply:?}");
        }
    }

    #[test]
    fn scales_short_components_like_herdr() {
        assert_eq!(component("f"), Some(255));
        assert_eq!(component("80"), Some(128));
        assert_eq!(component("800"), Some(128));
        assert_eq!(component("8000"), Some(128));
    }

    #[test]
    fn the_device_attributes_reply_completes_the_exchange() {
        assert!(is_complete(b"\x1b[?1;2;4c"));
        assert!(is_complete(b"junk\x1b]11;rgb:0/0/0\x07\x1b[?62;22c"));
        assert!(!is_complete(b"\x1b[?997;1n"));
        assert!(!is_complete(b"\x1b[?62;22"));
        assert!(!is_complete(b""));
    }

    #[test]
    fn appearance_round_trips_through_text() {
        for appearance in [Appearance::Dark, Appearance::Light] {
            assert_eq!(appearance.to_string().parse(), Ok(appearance));
        }
        assert_eq!(
            "dusk".parse::<Appearance>(),
            Err(UnknownAppearance("dusk".to_owned()))
        );
    }

    #[test]
    fn the_query_ends_with_device_attributes() {
        assert!(QUERY.starts_with(b"\x1b[?996n"));
        assert!(QUERY.ends_with(b"\x1b[c"));
    }
}
