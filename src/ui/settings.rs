use crate::runner::Quality;
use crate::ui::theme::{ThemeId, ThemePalette};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Widget},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorChoice {
    Micro,
    Vim,
    Nvim,
    Nano,
}

impl EditorChoice {
    #[allow(dead_code)]
    pub const ALL: [EditorChoice; 4] = [
        EditorChoice::Micro,
        EditorChoice::Vim,
        EditorChoice::Nvim,
        EditorChoice::Nano,
    ];

    pub fn as_cmd(&self) -> &'static str {
        match self {
            EditorChoice::Micro => "micro",
            EditorChoice::Vim => "vim",
            EditorChoice::Nvim => "nvim",
            EditorChoice::Nano => "nano",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            EditorChoice::Micro => "micro (terminal editor)",
            EditorChoice::Vim => "vim (vi improved)",
            EditorChoice::Nvim => "nvim (neovim)",
            EditorChoice::Nano => "nano (simple editor)",
        }
    }

    pub fn from_cmd(cmd: &str) -> Self {
        let first = cmd.split_whitespace().next().unwrap_or("micro");
        if first.contains("nvim") {
            EditorChoice::Nvim
        } else if first.contains("vim") {
            EditorChoice::Vim
        } else if first.contains("nano") {
            EditorChoice::Nano
        } else {
            EditorChoice::Micro
        }
    }

    pub fn next(&self) -> Self {
        match self {
            EditorChoice::Micro => EditorChoice::Vim,
            EditorChoice::Vim => EditorChoice::Nvim,
            EditorChoice::Nvim => EditorChoice::Nano,
            EditorChoice::Nano => EditorChoice::Micro,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            EditorChoice::Micro => EditorChoice::Nano,
            EditorChoice::Vim => EditorChoice::Micro,
            EditorChoice::Nvim => EditorChoice::Vim,
            EditorChoice::Nano => EditorChoice::Nvim,
        }
    }
}

pub const ALLOWED_FPS: [u32; 4] = [15, 24, 30, 60];
pub const ALLOWED_QUALITIES: [Quality; 4] = [
    Quality::Low,
    Quality::Medium,
    Quality::High,
    Quality::FourK,
];

pub fn quality_label(q: Quality) -> &'static str {
    match q {
        Quality::Low => "Low (480p)",
        Quality::Medium => "Medium (720p)",
        Quality::High => "High (1080p)",
        Quality::FourK => "4K Ultra (2160p)",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Close,
    ApplyTheme(ThemeId),
    ApplyEditor(String),
    ApplyQuality(Quality),
    ApplyFps(u32),
}

pub struct SettingsState {
    pub theme: ThemeId,
    pub editor: EditorChoice,
    pub quality: Quality,
    pub fps: u32,
    pub selected_row: usize,
}

impl SettingsState {
    pub fn new(current_editor: &str, current_quality: Quality, current_fps: u32) -> Self {
        Self {
            theme: ThemeId::CatppuccinMocha,
            editor: EditorChoice::from_cmd(current_editor),
            quality: current_quality,
            fps: current_fps.max(1),
            selected_row: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SettingsAction {
        match key.code {
            KeyCode::Esc | KeyCode::F(4) | KeyCode::Char('?') => SettingsAction::Close,
            KeyCode::Char('q') | KeyCode::Char('Q') => SettingsAction::Close,

            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_row == 0 {
                    self.selected_row = 3;
                } else {
                    self.selected_row -= 1;
                }
                SettingsAction::None
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_row = (self.selected_row + 1) % 4;
                SettingsAction::None
            }

            KeyCode::Left | KeyCode::Char('h') => self.adjust_current(false),

            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => {
                self.adjust_current(true)
            }

            _ => SettingsAction::None,
        }
    }

    fn adjust_current(&mut self, next: bool) -> SettingsAction {
        match self.selected_row {
            0 => {
                self.theme = if next {
                    self.theme.next()
                } else {
                    self.theme.prev()
                };
                SettingsAction::ApplyTheme(self.theme)
            }
            1 => {
                self.editor = if next {
                    self.editor.next()
                } else {
                    self.editor.prev()
                };
                SettingsAction::ApplyEditor(self.editor.as_cmd().to_string())
            }
            2 => {
                let idx = ALLOWED_QUALITIES
                    .iter()
                    .position(|&q| q == self.quality)
                    .unwrap_or(1);
                let new_idx = if next {
                    (idx + 1) % ALLOWED_QUALITIES.len()
                } else if idx == 0 {
                    ALLOWED_QUALITIES.len() - 1
                } else {
                    idx - 1
                };
                self.quality = ALLOWED_QUALITIES[new_idx];
                SettingsAction::ApplyQuality(self.quality)
            }
            3 => {
                let idx = ALLOWED_FPS
                    .iter()
                    .position(|&f| f == self.fps)
                    .unwrap_or(2);
                let new_idx = if next {
                    (idx + 1) % ALLOWED_FPS.len()
                } else if idx == 0 {
                    ALLOWED_FPS.len() - 1
                } else {
                    idx - 1
                };
                self.fps = ALLOWED_FPS[new_idx];
                SettingsAction::ApplyFps(self.fps)
            }
            _ => SettingsAction::None,
        }
    }
}

pub fn render_settings_overlay(
    state: &SettingsState,
    area: Rect,
    buf: &mut Buffer,
    palette: &ThemePalette,
) {
    // Center modal inside window
    let modal_w = 70.min(area.width.saturating_sub(2));
    let modal_h = 22.min(area.height.saturating_sub(2));

    let offset_x = (area.width.saturating_sub(modal_w)) / 2;
    let offset_y = (area.height.saturating_sub(modal_h)) / 2;
    let modal_area = Rect::new(area.x + offset_x, area.y + offset_y, modal_w, modal_h);

    // Blank out underlying cells
    Clear.render(modal_area, buf);

    let block = Block::default()
        .title(" ❪ Settings & Studio Customization ❫ ")
        .title_alignment(Alignment::Center)
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(palette.border_focused)
                .add_modifier(Modifier::BOLD),
        );

    let inner_area = block.inner(modal_area);
    block.render(modal_area, buf);

    let show_ascii = inner_area.height >= 18;

    // Layout inside modal: Header, Settings rows, Footer
    let chunks = if show_ascii {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // ASCII Art MANIM-LIVE
                Constraint::Length(1), // Spacer
                Constraint::Length(2), // Subtitle / navigation tips
                Constraint::Length(8), // 4 setting rows (each 2 rows)
                Constraint::Min(0),    // Spacer
                Constraint::Length(2), // Bottom hints
            ])
            .split(inner_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(0),
                Constraint::Length(0),
                Constraint::Length(2),
                Constraint::Length(8),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(inner_area)
    };

    // ASCII Art Header
    if show_ascii {
        let ascii_lines = vec![
            Line::from(vec![
                Span::styled("█▀▄▀█ ▄▀▀█ █▄  █ █ █▀▄▀█", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("          ", Style::default().fg(palette.warn_peach)),
                Span::styled("█    █ █   █ █▀▀▀", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("█ █ █ █▀▀█ █ ▀▄█ █ █ █ █", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("   ▄▄▄▄   ", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
                Span::styled("█    █ █   █ █▀▀ ", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("█ █ █ █  █ █   █ █ █ █ █", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("          ", Style::default().fg(palette.warn_peach)),
                Span::styled("█▄▄▄ █  ▀▄▀  █▄▄▄", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("▀   ▀ ▀  ▀ ▀   ▀ ▀ ▀   ▀", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("          ", Style::default().fg(palette.warn_peach)),
                Span::styled("▀▀▀▀ ▀   ▀   ▀▀▀▀", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
        ];
        let ascii_p = Paragraph::new(ascii_lines).alignment(Alignment::Center);
        ascii_p.render(chunks[0], buf);
    }

    // Subtitle
    let subtitle = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("↑/↓", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
            Span::styled(" Select  •  ", Style::default().fg(palette.text_dimmed)),
            Span::styled("←/→/Enter", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            Span::styled(" Change  •  ", Style::default().fg(palette.text_dimmed)),
            Span::styled("Esc / F4 / ?", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
            Span::styled(" Close", Style::default().fg(palette.text_dimmed)),
        ]),
        Line::from(Span::styled(
            "────────────────────────────────────────────────────────",
            Style::default().fg(palette.border_unfocused),
        )),
    ])
    .alignment(Alignment::Center);
    subtitle.render(chunks[2], buf);

    // 4 Settings Rows
    let row_constraints = vec![
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
    ];
    let rows_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(chunks[3]);

    let row_data: [(usize, &str, String); 4] = [
        (0, "🎨 Theme Palette", state.theme.name().to_string()),
        (1, "✏️  Editor Command", state.editor.display_name().to_string()),
        (2, "📺 Render Quality", quality_label(state.quality).to_string()),
        (3, "⚡ Render FPS", format!("{} FPS", state.fps)),
    ];

    for (row_idx, label, val) in row_data {
        let is_selected = state.selected_row == row_idx;
        let r_area = rows_chunks[row_idx];

        let prefix_indicator = if is_selected { " ❯ " } else { "   " };
        let label_style = if is_selected {
            Style::default()
                .fg(palette.accent_mauve)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text_primary)
        };

        let val_spans = if is_selected {
            vec![
                Span::styled(" ◄ [ ", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
                Span::styled(
                    val,
                    Style::default()
                        .fg(palette.bg_crust)
                        .bg(palette.accent_blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ] ► ", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
            ]
        } else {
            vec![
                Span::styled("   [ ", Style::default().fg(palette.text_dimmed)),
                Span::styled(val, Style::default().fg(palette.text_muted)),
                Span::styled(" ]   ", Style::default().fg(palette.text_dimmed)),
            ]
        };

        let mut line_spans = vec![
            Span::styled(
                prefix_indicator,
                Style::default()
                    .fg(palette.border_focused)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:<22}", label), label_style),
        ];
        line_spans.extend(val_spans);

        let row_p = Paragraph::new(Line::from(line_spans));
        row_p.render(r_area, buf);
    }

    // Bottom Footer Hints
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(
            "────────────────────────────────────────────────────────",
            Style::default().fg(palette.border_unfocused),
        )),
        Line::from(vec![
            Span::styled("✦ Settings take effect immediately in real-time.", Style::default().fg(palette.success_sage).add_modifier(Modifier::BOLD)),
        ]),
    ])
    .alignment(Alignment::Center);
    footer.render(chunks[5], buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_settings_navigation_and_cycling() {
        let mut settings = SettingsState::new("micro", Quality::Medium, 30);
        assert_eq!(settings.selected_row, 0);
        assert_eq!(settings.theme, ThemeId::CatppuccinMocha);

        // Cycle theme to next (Right)
        let key_right = KeyEvent::new(KeyCode::Right, KeyModifiers::empty());
        let action = settings.handle_key(key_right);
        assert_eq!(action, SettingsAction::ApplyTheme(ThemeId::TokyoNight));
        assert_eq!(settings.theme, ThemeId::TokyoNight);

        // Cycle theme to prev (Left)
        let key_left = KeyEvent::new(KeyCode::Left, KeyModifiers::empty());
        let action = settings.handle_key(key_left);
        assert_eq!(action, SettingsAction::ApplyTheme(ThemeId::CatppuccinMocha));

        // Move down to Editor
        let key_down = KeyEvent::new(KeyCode::Down, KeyModifiers::empty());
        settings.handle_key(key_down);
        assert_eq!(settings.selected_row, 1);
        assert_eq!(settings.editor, EditorChoice::Micro);

        // Cycle editor to vim
        let action = settings.handle_key(key_right);
        assert_eq!(action, SettingsAction::ApplyEditor("vim".to_string()));
        assert_eq!(settings.editor, EditorChoice::Vim);

        // Move down to Quality
        settings.handle_key(key_down);
        assert_eq!(settings.selected_row, 2);

        // Cycle quality
        let action = settings.handle_key(key_right);
        assert_eq!(action, SettingsAction::ApplyQuality(Quality::High));

        // Move down to FPS
        settings.handle_key(key_down);
        assert_eq!(settings.selected_row, 3);

        // Cycle fps
        let action = settings.handle_key(key_right);
        assert_eq!(action, SettingsAction::ApplyFps(60));

        // Test wrap down to 0
        settings.handle_key(key_down);
        assert_eq!(settings.selected_row, 0);

        // Test Escape closes settings
        let key_esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert_eq!(settings.handle_key(key_esc), SettingsAction::Close);

        // Test F4 closes settings
        let key_f4 = KeyEvent::new(KeyCode::F(4), KeyModifiers::empty());
        assert_eq!(settings.handle_key(key_f4), SettingsAction::Close);

        // Test ? closes settings
        let key_qmark = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty());
        assert_eq!(settings.handle_key(key_qmark), SettingsAction::Close);
    }

    #[test]
    fn test_quality_clamping_logic() {
        assert_eq!(Quality::High.clamp_to_cap(Quality::Medium), Quality::Medium);
        assert_eq!(Quality::FourK.clamp_to_cap(Quality::Medium), Quality::Medium);
        assert_eq!(Quality::Low.clamp_to_cap(Quality::Medium), Quality::Low);
        assert_eq!(Quality::High.clamp_to_cap(Quality::FourK), Quality::High);

        assert_eq!(Quality::Low.max_height(), 480);
        assert_eq!(Quality::Medium.max_height(), 720);
        assert_eq!(Quality::High.max_height(), 1080);
        assert_eq!(Quality::FourK.max_height(), 2160);
    }
}
