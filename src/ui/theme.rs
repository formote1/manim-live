#![allow(dead_code)]

use ratatui::style::Color;

/// Available themes for manim-live
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    CatppuccinMocha,
    TokyoNight,
    GruvboxDark,
    Nord,
    Dracula,
    HighContrast,
}

impl ThemeId {
    pub const ALL: [ThemeId; 6] = [
        ThemeId::CatppuccinMocha,
        ThemeId::TokyoNight,
        ThemeId::GruvboxDark,
        ThemeId::Nord,
        ThemeId::Dracula,
        ThemeId::HighContrast,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ThemeId::CatppuccinMocha => "Catppuccin Mocha",
            ThemeId::TokyoNight => "Tokyo Night",
            ThemeId::GruvboxDark => "Gruvbox Dark",
            ThemeId::Nord => "Nord Arctic",
            ThemeId::Dracula => "Dracula Dark",
            ThemeId::HighContrast => "High Contrast (A11y)",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.contains("tokyo") {
            Some(ThemeId::TokyoNight)
        } else if lower.contains("gruvbox") {
            Some(ThemeId::GruvboxDark)
        } else if lower.contains("nord") {
            Some(ThemeId::Nord)
        } else if lower.contains("dracula") {
            Some(ThemeId::Dracula)
        } else if lower.contains("contrast") {
            Some(ThemeId::HighContrast)
        } else if lower.contains("catppuccin") || lower.contains("mocha") {
            Some(ThemeId::CatppuccinMocha)
        } else {
            None
        }
    }

    pub fn is_high_contrast(&self) -> bool {
        matches!(self, ThemeId::HighContrast)
    }

    pub fn next(&self) -> Self {
        match self {
            ThemeId::CatppuccinMocha => ThemeId::TokyoNight,
            ThemeId::TokyoNight => ThemeId::GruvboxDark,
            ThemeId::GruvboxDark => ThemeId::Nord,
            ThemeId::Nord => ThemeId::Dracula,
            ThemeId::Dracula => ThemeId::HighContrast,
            ThemeId::HighContrast => ThemeId::CatppuccinMocha,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            ThemeId::CatppuccinMocha => ThemeId::HighContrast,
            ThemeId::TokyoNight => ThemeId::CatppuccinMocha,
            ThemeId::GruvboxDark => ThemeId::TokyoNight,
            ThemeId::Nord => ThemeId::GruvboxDark,
            ThemeId::Dracula => ThemeId::Nord,
            ThemeId::HighContrast => ThemeId::Dracula,
        }
    }

    pub fn palette(&self) -> ThemePalette {
        match self {
            ThemeId::CatppuccinMocha => ThemePalette {
                bg_dark: Color::Rgb(24, 24, 37),      // Mantle
                bg_crust: Color::Rgb(17, 17, 27),     // Crust
                bg_surface: Color::Rgb(49, 50, 68),   // Surface0
                border_focused: Color::Rgb(180, 190, 254), // Lavender
                border_unfocused: Color::Rgb(69, 71, 90),  // Surface1
                text_primary: Color::Rgb(205, 214, 244),   // Text
                text_muted: Color::Rgb(147, 153, 178),     // Subtext0
                text_dimmed: Color::Rgb(108, 112, 134),    // Overlay0
                accent_mauve: Color::Rgb(203, 166, 247),   // Mauve
                accent_blue: Color::Rgb(137, 180, 250),    // Blue
                accent_sapphire: Color::Rgb(116, 199, 236),// Sapphire
                success_sage: Color::Rgb(166, 227, 161),   // Green
                warn_peach: Color::Rgb(250, 179, 135),     // Peach
                error_coral: Color::Rgb(243, 139, 168),    // Red
            },
            ThemeId::TokyoNight => ThemePalette {
                bg_dark: Color::Rgb(26, 27, 38),
                bg_crust: Color::Rgb(22, 22, 30),
                bg_surface: Color::Rgb(41, 46, 66),
                border_focused: Color::Rgb(122, 162, 247),
                border_unfocused: Color::Rgb(65, 72, 104),
                text_primary: Color::Rgb(192, 202, 245),
                text_muted: Color::Rgb(154, 165, 206),
                text_dimmed: Color::Rgb(86, 95, 137),
                accent_mauve: Color::Rgb(187, 154, 247),
                accent_blue: Color::Rgb(122, 162, 247),
                accent_sapphire: Color::Rgb(125, 207, 255),
                success_sage: Color::Rgb(158, 206, 106),
                warn_peach: Color::Rgb(255, 158, 100),
                error_coral: Color::Rgb(247, 118, 142),
            },
            ThemeId::GruvboxDark => ThemePalette {
                bg_dark: Color::Rgb(40, 40, 40),
                bg_crust: Color::Rgb(29, 32, 33),
                bg_surface: Color::Rgb(60, 56, 54),
                border_focused: Color::Rgb(254, 128, 25),
                border_unfocused: Color::Rgb(80, 73, 69),
                text_primary: Color::Rgb(235, 219, 178),
                text_muted: Color::Rgb(168, 153, 132),
                text_dimmed: Color::Rgb(146, 131, 116),
                accent_mauve: Color::Rgb(211, 134, 155),
                accent_blue: Color::Rgb(131, 165, 152),
                accent_sapphire: Color::Rgb(142, 192, 124),
                success_sage: Color::Rgb(184, 187, 38),
                warn_peach: Color::Rgb(250, 189, 47),
                error_coral: Color::Rgb(251, 73, 52),
            },
            ThemeId::Nord => ThemePalette {
                bg_dark: Color::Rgb(46, 52, 64),
                bg_crust: Color::Rgb(36, 41, 51),
                bg_surface: Color::Rgb(59, 66, 82),
                border_focused: Color::Rgb(136, 192, 208),
                border_unfocused: Color::Rgb(76, 86, 106),
                text_primary: Color::Rgb(236, 239, 244),
                text_muted: Color::Rgb(216, 222, 233),
                text_dimmed: Color::Rgb(129, 161, 193),
                accent_mauve: Color::Rgb(180, 142, 173),
                accent_blue: Color::Rgb(129, 161, 193),
                accent_sapphire: Color::Rgb(143, 188, 187),
                success_sage: Color::Rgb(163, 190, 140),
                warn_peach: Color::Rgb(235, 203, 139),
                error_coral: Color::Rgb(191, 97, 106),
            },
            ThemeId::Dracula => ThemePalette {
                bg_dark: Color::Rgb(40, 42, 54),
                bg_crust: Color::Rgb(33, 34, 44),
                bg_surface: Color::Rgb(68, 71, 90),
                border_focused: Color::Rgb(189, 147, 249),
                border_unfocused: Color::Rgb(98, 114, 164),
                text_primary: Color::Rgb(248, 248, 242),
                text_muted: Color::Rgb(189, 147, 249),
                text_dimmed: Color::Rgb(98, 114, 164),
                accent_mauve: Color::Rgb(255, 121, 198),
                accent_blue: Color::Rgb(139, 233, 253),
                accent_sapphire: Color::Rgb(139, 233, 253),
                success_sage: Color::Rgb(80, 250, 123),
                warn_peach: Color::Rgb(255, 184, 108),
                error_coral: Color::Rgb(255, 85, 85),
            },
            ThemeId::HighContrast => ThemePalette {
                bg_dark: Color::Rgb(0, 0, 0),
                bg_crust: Color::Rgb(0, 0, 0),
                bg_surface: Color::Rgb(20, 20, 20),
                border_focused: Color::Rgb(255, 255, 255),
                border_unfocused: Color::Rgb(128, 128, 128),
                text_primary: Color::Rgb(255, 255, 255),
                text_muted: Color::Rgb(200, 200, 200),
                text_dimmed: Color::Rgb(150, 150, 150),
                accent_mauve: Color::Rgb(255, 255, 255),
                accent_blue: Color::Rgb(0, 255, 255),
                accent_sapphire: Color::Rgb(0, 255, 255),
                success_sage: Color::Rgb(0, 255, 0),
                warn_peach: Color::Rgb(255, 255, 0),
                error_coral: Color::Rgb(255, 0, 0),
            },
        }
    }
}

/// Palette holding concrete colors for current UI theme
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_dark: Color,
    pub bg_crust: Color,
    pub bg_surface: Color,
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub text_dimmed: Color,
    pub accent_mauve: Color,
    pub accent_blue: Color,
    pub accent_sapphire: Color,
    pub success_sage: Color,
    pub warn_peach: Color,
    pub error_coral: Color,
}

impl ThemePalette {
    pub fn is_high_contrast(&self) -> bool {
        self.border_focused == Color::White && self.bg_dark == Color::Black
    }
}


// Default constants for backward compatibility with existing static references
pub const BG_DARK: Color = Color::Rgb(24, 24, 37); // Mantle
pub const BG_CRUST: Color = Color::Rgb(17, 17, 27); // Crust
pub const BG_SURFACE: Color = Color::Rgb(49, 50, 68); // Surface0

pub const BORDER_FOCUSED: Color = Color::Rgb(180, 190, 254); // Lavender
pub const BORDER_UNFOCUSED: Color = Color::Rgb(69, 71, 90); // Surface1

pub const TEXT_PRIMARY: Color = Color::Rgb(205, 214, 244); // Text
pub const TEXT_MUTED: Color = Color::Rgb(147, 153, 178); // Subtext0
pub const TEXT_DIMMED: Color = Color::Rgb(108, 112, 134); // Overlay0

pub const ACCENT_MAUVE: Color = Color::Rgb(203, 166, 247); // Mauve
pub const ACCENT_BLUE: Color = Color::Rgb(137, 180, 250); // Blue
pub const ACCENT_SAPPHIRE: Color = Color::Rgb(116, 199, 236); // Sapphire

pub const SUCCESS_SAGE: Color = Color::Rgb(166, 227, 161); // Green
pub const WARN_PEACH: Color = Color::Rgb(250, 179, 135); // Peach
pub const ERROR_CORAL: Color = Color::Rgb(243, 139, 168); // Red
