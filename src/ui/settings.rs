use crate::runner::Quality;
use crate::ui::theme::{ThemeId, ThemePalette};
use crate::ui::FocusPane;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
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

pub const ALLOWED_FPS: [u32; 5] = [15, 24, 30, 60, 120];
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

pub const TOTAL_SETTINGS_ROWS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Close,
    Quit,
    ApplyTheme(ThemeId),
    ApplyEditor(String),
    ApplyQuality(Quality),
    ApplyFps(u32),
    FocusPane(FocusPane),
    TriggerRebuild,
    OpenScenes,
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
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
        {
            return SettingsAction::Quit;
        }

        match key.code {
            KeyCode::Esc | KeyCode::F(4) | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Char('Q') => {
                SettingsAction::Close
            }
            KeyCode::F(1) => SettingsAction::FocusPane(FocusPane::Editor),
            KeyCode::F(2) => SettingsAction::FocusPane(FocusPane::Preview),
            KeyCode::F(3) => SettingsAction::FocusPane(FocusPane::Console),
            KeyCode::F(5) => SettingsAction::TriggerRebuild,
            KeyCode::F(8) => SettingsAction::OpenScenes,

            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_row == 0 {
                    self.selected_row = TOTAL_SETTINGS_ROWS - 1;
                } else {
                    self.selected_row -= 1;
                }
                SettingsAction::None
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_row = (self.selected_row + 1) % TOTAL_SETTINGS_ROWS;
                SettingsAction::None
            }

            KeyCode::Left | KeyCode::Char('h') => {
                self.adjust_current(false)
            }

            KeyCode::Right | KeyCode::Char('l') => {
                self.adjust_current(true)
            }

            KeyCode::Enter | KeyCode::Char(' ') => self.activate_current(),

            _ => SettingsAction::None,
        }
    }

    fn activate_current(&mut self) -> SettingsAction {
        self.adjust_current(true)
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
                    .unwrap_or(0);
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

pub fn calculate_modal_area(area: Rect) -> Rect {
    let modal_w = if area.width >= 85 {
        83
    } else {
        area.width.saturating_sub(2).max(20)
    };
    let modal_h = if area.height >= 26 {
        24
    } else {
        area.height.saturating_sub(2).max(14)
    };

    let offset_x = (area.width.saturating_sub(modal_w)) / 2;
    let offset_y = (area.height.saturating_sub(modal_h)) / 2;
    Rect::new(area.x + offset_x, area.y + offset_y, modal_w, modal_h)
}

pub fn clear_settings_logo() {
    crate::ui::preview::clear_kitty_image_id(2);
}

pub fn render_settings_overlay(
    state: &SettingsState,
    area: Rect,
    buf: &mut Buffer,
    palette: &ThemePalette,
) {
    let modal_area = calculate_modal_area(area);

    // Completely wipe the modal area with bg_crust so background text/wallpaper never bleeds through
    for r in 0..modal_area.height {
        for c in 0..modal_area.width {
            if let Some(cell) = buf.cell_mut((modal_area.x + c, modal_area.y + r)) {
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(palette.bg_crust));
            }
        }
    }

    let block = ratatui::widgets::Block::default()
        .title(" ❪ Settings & Studio Customization ❫ ")
        .title_alignment(Alignment::Center)
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD));
    let inner_area = block.inner(modal_area);
    block.render(modal_area, buf);

    let show_full_ascii = inner_area.width >= 77 && inner_area.height >= 19;

    let chunks = if show_full_ascii {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),  // 6-line exact ASCII art
                Constraint::Length(1),  // Subtitle
                Constraint::Length(1),  // Divider
                Constraint::Length(11), // 4 settings (2 lines each + 1 line spacer = 11 lines)
                Constraint::Min(0),     // Spacer
                Constraint::Length(1),  // Legend footer
            ])
            .split(inner_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Compact title
                Constraint::Length(1),  // Divider
                Constraint::Min(8),     // Settings list
                Constraint::Length(1),  // Legend footer
            ])
            .split(inner_area)
    };

    // Render BIG ASCII Art Header
    if show_full_ascii {
        let ascii_lines = vec![
            Line::from(vec![
                Span::styled("███╗   ███╗ █████╗ ███╗   ██╗██╗███╗   ███╗", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("      ", Style::default()),
                Span::styled("██╗     ██╗██╗   ██╗███████╗", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("████╗ ████║██╔══██╗████╗  ██║██║████╗ ████║", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("      ", Style::default()),
                Span::styled("██║     ██║██║   ██║██╔════╝", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("██╔████╔██║███████║██╔██╗ ██║██║██╔████╔██║", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("█████╗", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
                Span::styled("██║     ██║██║   ██║█████╗  ", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("██║╚██╔╝██║██╔══██║██║╚██╗██║██║██║╚██╔╝██║", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("╚════╝", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
                Span::styled("██║     ██║╚██╗ ██╔╝██╔══╝  ", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("██║ ╚═╝ ██║██║  ██║██║ ╚████║██║██║ ╚═╝ ██║", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("      ", Style::default()),
                Span::styled("███████╗██║ ╚████╔╝ ███████╗", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝╚═╝╚═╝     ╚═╝", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("      ", Style::default()),
                Span::styled("╚══════╝╚═╝  ╚═══╝  ╚══════╝", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
            ]),
        ];
        let p = Paragraph::new(ascii_lines).alignment(Alignment::Center);
        p.render(chunks[0], buf);

        let sub_p = Paragraph::new(Line::from(Span::styled(
            "✦  STUDIO CONFIGURATION & PREFERENCES  ✦",
            Style::default().fg(palette.text_dimmed).add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center);
        sub_p.render(chunks[1], buf);

        let div_p = Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[2].width as usize),
            Style::default().fg(palette.border_unfocused),
        )));
        div_p.render(chunks[2], buf);
    } else {
        let title_p = Paragraph::new(Line::from(vec![
            Span::styled("MANIM-LIVE ", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
            Span::styled("• STUDIO SETTINGS", Style::default().fg(palette.text_dimmed)),
        ]))
        .alignment(Alignment::Center);
        title_p.render(chunks[0], buf);

        let div_p = Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[1].width as usize),
            Style::default().fg(palette.border_unfocused),
        )));
        div_p.render(chunks[1], buf);
    }

    // Direct, flat, spacious settings list (left-aligned)
    let list_area = if show_full_ascii {
        chunks[3]
    } else {
        chunks[2]
    };

    let row_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Row 0
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Row 1
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Row 2
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Row 3
        ])
        .split(list_area);

    let row_data: [(usize, &str, String); TOTAL_SETTINGS_ROWS] = [
        (0, "🎨  Theme Palette", state.theme.name().to_string()),
        (1, "✏️   Editor Command", state.editor.display_name().to_string()),
        (2, "📺  Render Quality", quality_label(state.quality).to_string()),
        (3, "⚡  Render FPS", format!("{} FPS", state.fps)),
    ];

    for (row_idx, label, val) in row_data {
        let chunk_idx = row_idx * 2;
        if chunk_idx >= row_layout.len() {
            break;
        }
        let is_selected = state.selected_row == row_idx;
        let r_area = row_layout[chunk_idx];

        let (prefix, label_style) = if is_selected {
            (
                Span::styled(" ❯ ", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD),
            )
        } else {
            (
                Span::styled("   ", Style::default()),
                Style::default().fg(palette.text_dimmed),
            )
        };

        let title_line = Line::from(vec![
            prefix,
            Span::styled(label, label_style),
        ]);

        let val_line = if is_selected {
            Line::from(vec![
                Span::styled("      ◄ [ ", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" {} ", val),
                    Style::default()
                        .fg(palette.bg_crust)
                        .bg(palette.accent_mauve)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ] ►", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
                Span::styled("   (use ← / → to adjust)", Style::default().fg(palette.text_dimmed)),
            ])
        } else {
            Line::from(vec![
                Span::styled("        [ ", Style::default().fg(palette.text_dimmed)),
                Span::styled(val, Style::default().fg(palette.text_muted)),
                Span::styled(" ]", Style::default().fg(palette.text_dimmed)),
            ])
        };

        let row_p = Paragraph::new(vec![title_line, val_line]).alignment(Alignment::Left);
        row_p.render(r_area, buf);
    }

    // Static, non-selectable hotkey reference footer
    let footer_area = chunks[chunks.len() - 1];
    let footer_line = Line::from(vec![
        Span::styled(" ⌨  Quick Keys: ", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
        Span::styled("↑/↓", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Select  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("←/→", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Change  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("Esc / ?", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Close  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("F1", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Code  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("F2", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Preview  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("F3", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Console  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("Ctrl+Q", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit", Style::default().fg(palette.text_dimmed)),
    ]);
    let p_footer = Paragraph::new(footer_line).alignment(Alignment::Center);
    p_footer.render(footer_area, buf);
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

        // Wrap around down from 3 to 0
        settings.handle_key(key_down);
        assert_eq!(settings.selected_row, 0);

        // Wrap around up from 0 to 3
        let key_up = KeyEvent::new(KeyCode::Up, KeyModifiers::empty());
        settings.handle_key(key_up);
        assert_eq!(settings.selected_row, 3);

        // Direct Ctrl+Q triggers Quit
        let key_ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(settings.handle_key(key_ctrl_q), SettingsAction::Quit);

        // Direct F-key press tests
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::empty())),
            SettingsAction::FocusPane(FocusPane::Editor)
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty())),
            SettingsAction::FocusPane(FocusPane::Preview)
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty())),
            SettingsAction::FocusPane(FocusPane::Console)
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::empty())),
            SettingsAction::TriggerRebuild
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::empty())),
            SettingsAction::OpenScenes
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())),
            SettingsAction::Close
        );
        assert_eq!(
            settings.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::empty())),
            SettingsAction::Close
        );
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
