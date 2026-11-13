use ratatui::style::Color;
use std::fs;
use std::path::{Path, PathBuf};

/// UI color palette derived from Omarchy's active theme or a high-contrast dark fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub accent: Color,
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Color::Rgb(0x25, 0x63, 0xEB),
            background: Color::Rgb(0x0F, 0x17, 0x2A),
            foreground: Color::Rgb(0xF8, 0xFA, 0xFC),
            muted: Color::Rgb(0x64, 0x74, 0x8B),
            success: Color::Rgb(0x22, 0xC5, 0x5E),
            warning: Color::Rgb(0xD9, 0x77, 0x06),
            error: Color::Rgb(0xDC, 0x26, 0x26),
        }
    }
}

impl Theme {
    /// Load from `~/.local/state/omarchy/current/theme/colors.toml`, falling back to defaults.
    pub fn load_default() -> Self {
        let Some(state_dir) = dirs::state_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        else {
            return Self::default();
        };
        Self::from_omarchy_dir(&state_dir.join("omarchy/current/theme"))
    }

    /// Load `colors.toml` inside the given theme directory.
    pub fn from_omarchy_dir(theme_dir: &Path) -> Self {
        let path: PathBuf = theme_dir.join("colors.toml");
        match fs::read_to_string(path) {
            Ok(contents) => Self::from_toml_str(&contents),
            Err(_) => Self::default(),
        }
    }

    /// Parse Omarchy `colors.toml` keys with graceful fallback per color.
    pub fn from_toml_str(contents: &str) -> Self {
        let mut theme = Self::default();
        let Ok(value) = contents.parse::<toml::Value>() else {
            return theme;
        };
        let Some(table) = value.as_table() else {
            return theme;
        };

        if let Some(c) = get_hex(table, "accent") {
            theme.accent = c;
        }
        if let Some(c) = get_hex(table, "background") {
            theme.background = c;
        }
        if let Some(c) = get_hex(table, "foreground") {
            theme.foreground = c;
        }
        if let Some(c) = get_hex(table, "color8") {
            theme.muted = c;
        }
        if let Some(c) = get_hex(table, "color2") {
            theme.success = c;
        }
        if let Some(c) = get_hex(table, "color3") {
            theme.warning = c;
        }
        if let Some(c) = get_hex(table, "color1") {
            theme.error = c;
        }
        theme
    }
}

fn get_hex(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<Color> {
    let raw = table.get(key)?.as_str()?.trim();
    parse_hex_color(raw)
}

fn parse_hex_color(raw: &str) -> Option<Color> {
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_omarchy_colors_toml() {
        let toml_data = r##"
accent = "#FFD60A"
background = "#11111B"
foreground = "#CDD6F4"
color1 = "#F38BA8"
color2 = "#A6E3A1"
color3 = "#F9E2AF"
color8 = "#585B70"
"##;
        let theme = Theme::from_toml_str(toml_data);
        assert_eq!(theme.accent, Color::Rgb(0xFF, 0xD6, 0x0A));
        assert_eq!(theme.background, Color::Rgb(0x11, 0x11, 0x1B));
        assert_eq!(theme.foreground, Color::Rgb(0xCD, 0xD6, 0xF4));
        assert_eq!(theme.error, Color::Rgb(0xF3, 0x8B, 0xA8));
        assert_eq!(theme.success, Color::Rgb(0xA6, 0xE3, 0xA1));
        assert_eq!(theme.warning, Color::Rgb(0xF9, 0xE2, 0xAF));
        assert_eq!(theme.muted, Color::Rgb(0x58, 0x5B, 0x70));
    }

    #[test]
    fn falls_back_when_theme_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let theme = Theme::from_omarchy_dir(tmp.path());
        assert_eq!(theme, Theme::default());
    }

    #[test]
    fn ignores_invalid_hex_and_keeps_default() {
        let theme = Theme::from_toml_str("accent = \"not-a-color\"\n");
        assert_eq!(theme.accent, Theme::default().accent);
    }
}
