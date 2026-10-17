//! Terminal colors as Herdr's theme writes them, and how each consumer
//! renders one: fzf's `--color` ([`Color::to_fzf`]), an ANSI background
//! ([`Color::to_ansi_bg`], the error screen) and foreground
//! ([`Color::to_ansi_fg`], our own text). Which theme color goes where is
//! [`theme`](crate::domain::theme)'s and the picker's job, not this
//! module's.

/// A terminal color, as Herdr's theme has it (a subset of ratatui's
/// `Color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// The terminal's default color (`reset`).
    Reset,
    /// An ANSI color: one of the 16 base colors (0 to 15, what Herdr's theme
    /// names) or of the 256-color palette (the settings' numbers).
    Ansi(u8),
    /// A 24-bit color.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Parses a `[theme.custom]` value the way Herdr does: reset aliases,
    /// `#rrggbb`, `#rgb`, `rgb(r, g, b)` and the 16 color names
    /// (case-insensitive). Unknown values give cyan, Herdr's fallback.
    ///
    /// Mirrors `parse_color` in Herdr `src/config/theme.rs`, v0.9.3.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::color::Color;
    ///
    /// assert_eq!(Color::parse("#1E1E2E"), Color::Rgb(0x1e, 0x1e, 0x2e));
    /// assert_eq!(Color::parse("#fff"), Color::Rgb(255, 255, 255));
    /// assert_eq!(Color::parse("rgb(1, 2, 3)"), Color::Rgb(1, 2, 3));
    /// assert_eq!(Color::parse("Transparent"), Color::Reset);
    /// assert_eq!(Color::parse("gray"), Color::Ansi(7));
    /// ```
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let text = text.trim().to_lowercase();
        if matches!(text.as_str(), "reset" | "default" | "none" | "transparent") {
            return Self::Reset;
        }
        if let Some(color) = text.strip_prefix('#').and_then(parse_hex) {
            return color;
        }
        if let Some(color) = text
            .strip_prefix("rgb(")
            .and_then(|inner| inner.strip_suffix(')'))
            .and_then(parse_rgb)
        {
            return color;
        }
        // ANSI indexes of ratatui's named colors (Gray = 7, DarkGray = 8).
        let index = match text.as_str() {
            "black" => 0,
            "red" => 1,
            "green" => 2,
            "yellow" => 3,
            "blue" => 4,
            "magenta" | "purple" => 5,
            "gray" | "grey" => 7,
            "darkgray" | "darkgrey" => 8,
            "lightred" => 9,
            "lightgreen" => 10,
            "lightyellow" => 11,
            "lightblue" => 12,
            "lightmagenta" => 13,
            "lightcyan" => 14,
            "white" => 15,
            // "cyan" and anything unknown.
            _ => 6,
        };
        Self::Ansi(index)
    }

    /// fzf's `--color` syntax: `-1` (default), `0` to `15`, `#rrggbb`.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::color::Color;
    ///
    /// assert_eq!(Color::Rgb(0x1e, 0x1e, 0x2e).to_fzf(), "#1e1e2e");
    /// assert_eq!(Color::Reset.to_fzf(), "-1");
    /// ```
    #[must_use]
    pub fn to_fzf(self) -> String {
        match self {
            Self::Reset => "-1".to_owned(),
            Self::Ansi(index) => index.to_string(),
            Self::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }

    /// The SGR escape that sets this as the terminal background (`48;5;n`,
    /// `48;2;r;g;b`); empty for [`Color::Reset`], which keeps the
    /// terminal's own.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::color::Color;
    ///
    /// assert_eq!(Color::Ansi(4).to_ansi_bg(), "\x1b[48;5;4m");
    /// assert_eq!(Color::Reset.to_ansi_bg(), "");
    /// ```
    #[must_use]
    pub fn to_ansi_bg(self) -> String {
        match self {
            Self::Reset => String::new(),
            Self::Ansi(index) => format!("\x1b[48;5;{index}m"),
            Self::Rgb(r, g, b) => format!("\x1b[48;2;{r};{g};{b}m"),
        }
    }

    /// The SGR escape that sets this as the text color (`38;5;n`,
    /// `38;2;r;g;b`, or `39` for the terminal's own).
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::color::Color;
    ///
    /// assert_eq!(Color::Rgb(1, 2, 3).to_ansi_fg(), "\x1b[38;2;1;2;3m");
    /// assert_eq!(Color::Reset.to_ansi_fg(), "\x1b[39m");
    /// ```
    #[must_use]
    pub fn to_ansi_fg(self) -> String {
        match self {
            Self::Reset => "\x1b[39m".to_owned(),
            Self::Ansi(index) => format!("\x1b[38;5;{index}m"),
            Self::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        }
    }
}

/// `rrggbb` or `rgb` (hex digits, no `#`).
pub(crate) fn parse_hex(hex: &str) -> Option<Color> {
    // Checking the digits first means the parses below can't fail and
    // `len()` (bytes) equals the number of characters.
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    // `to_be_bytes` splits the u32 into 4 bytes, most significant first.
    let [_, r, g, b] = value.to_be_bytes();
    match hex.len() {
        6 => Some(Color::Rgb(r, g, b)),
        // `#abc` is `#aabbcc`: each digit times 17 (0x11).
        3 => {
            let digit = |shift: u32| u8::try_from((value >> shift) & 0xf).ok().map(|d| d * 17);
            Some(Color::Rgb(digit(8)?, digit(4)?, digit(0)?))
        }
        _ => None,
    }
}

/// `r, g, b` (decimal, 0 to 255).
fn parse_rgb(inner: &str) -> Option<Color> {
    let parts: Vec<u8> = inner
        .split(',')
        .map(|part| part.trim().parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [r, g, b] => Some(Color::Rgb(*r, *g, *b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors_like_herdr() {
        assert_eq!(Color::parse("  #ABCDEF "), Color::Rgb(0xab, 0xcd, 0xef));
        assert_eq!(Color::parse("#abc"), Color::Rgb(0xaa, 0xbb, 0xcc));
        assert_eq!(Color::parse("rgb( 10 ,20,30)"), Color::Rgb(10, 20, 30));
        assert_eq!(Color::parse("None"), Color::Reset);
        assert_eq!(Color::parse("DarkGrey"), Color::Ansi(8));
        assert_eq!(Color::parse("purple"), Color::Ansi(5));
        assert_eq!(Color::parse("white"), Color::Ansi(15));
    }

    #[test]
    fn renders_foreground_escapes() {
        assert_eq!(Color::Ansi(244).to_ansi_fg(), "\x1b[38;5;244m");
        assert_eq!(Color::Rgb(1, 2, 3).to_ansi_fg(), "\x1b[38;2;1;2;3m");
        assert_eq!(Color::Reset.to_ansi_fg(), "\x1b[39m");
    }

    #[test]
    fn unparseable_colors_fall_back_to_cyan() {
        for text in [
            "#12345",
            "#ggg",
            "#日本語",
            "rgb(1,2)",
            "rgb(1,2,300)",
            "chartreuse",
        ] {
            assert_eq!(Color::parse(text), Color::Ansi(6), "{text}");
        }
    }

    #[test]
    fn named_colors_map_to_ansi_indexes() {
        let names = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "grey",
            "darkgray",
            "lightred",
            "lightgreen",
            "lightyellow",
            "lightblue",
            "lightmagenta",
            "lightcyan",
            "white",
        ];
        for (index, name) in (0..).zip(names) {
            assert_eq!(Color::parse(name), Color::Ansi(index), "{name}");
        }
    }
}
