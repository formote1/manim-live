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

    #[allow(dead_code)]
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
    OpenFile,
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
            KeyCode::F(7) => SettingsAction::OpenFile,
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
    let modal_w = if area.width >= 86 {
        84
    } else {
        area.width.saturating_sub(2).max(20)
    };
    let modal_h = if area.height >= 27 {
        25
    } else {
        area.height.saturating_sub(2).max(15)
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

    let show_full_ascii = inner_area.width >= 77 && inner_area.height >= 22;

    let chunks = if show_full_ascii {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),  // 0: 6-line exact ASCII art
                Constraint::Length(1),  // 1: Subtitle
                Constraint::Length(1),  // 2: Divider
                Constraint::Length(11), // 3: 4 settings (2 lines each + 1 line spacer = 11 lines)
                Constraint::Min(0),     // 4: Flexible Spacer
                Constraint::Length(1),  // 5: Divider above footer
                Constraint::Length(2),  // 6: 2-line Legend footer
            ])
            .split(inner_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // 0: Compact title
                Constraint::Length(1),  // 1: Divider
                Constraint::Min(8),     // 2: Settings list
                Constraint::Length(1),  // 3: Divider above footer
                Constraint::Length(2),  // 4: 2-line Legend footer
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
            Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD),
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
            Span::styled("• STUDIO SETTINGS", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
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

    const GOLD: ratatui::style::Color = ratatui::style::Color::Rgb(255, 215, 0);
    const WHITE: ratatui::style::Color = ratatui::style::Color::Rgb(255, 255, 255);

    for row_idx in 0..TOTAL_SETTINGS_ROWS {
        let chunk_idx = row_idx * 2;
        if chunk_idx >= row_layout.len() {
            break;
        }
        let is_selected = state.selected_row == row_idx;
        let r_area = row_layout[chunk_idx];

        let (label, row_accent, val_spans) = match row_idx {
            0 => {
                let spans = vec![
                    Span::styled(format!(" {} ", state.theme.name()), Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
                ];
                ("🎨  Theme Palette", palette.accent_mauve, spans)
            }
            1 => {
                let (cmd, desc) = match state.editor {
                    EditorChoice::Micro => ("micro", "(terminal editor)"),
                    EditorChoice::Vim => ("vim", "(vi improved)"),
                    EditorChoice::Nvim => ("nvim", "(neovim)"),
                    EditorChoice::Nano => ("nano", "(simple editor)"),
                };
                let spans = vec![
                    Span::styled(format!(" {} ", cmd), Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{} ", desc), Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
                ];
                ("✏️   Editor Command", palette.warn_peach, spans)
            }
            2 => {
                let (q_name, q_res) = match state.quality {
                    Quality::Low => ("Low", "480p"),
                    Quality::Medium => ("Medium", "720p"),
                    Quality::High => ("High", "1080p"),
                    Quality::FourK => ("4K Ultra", "2160p"),
                };
                let spans = vec![
                    Span::styled(format!(" {} ", q_name), Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
                    Span::styled("• ", Style::default().fg(palette.border_unfocused)),
                    Span::styled(format!("{} ", q_res), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
                ];
                ("📺  Render Quality", palette.accent_sapphire, spans)
            }
            3 => {
                let spans = vec![
                    Span::styled(format!(" {} ", state.fps), Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
                    Span::styled("FPS ", Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
                ];
                ("⚡  Render FPS", palette.success_sage, spans)
            }
            _ => continue,
        };

        let title_line = if is_selected {
            Line::from(vec![
                Span::styled(" ❯ ", Style::default().fg(row_accent).add_modifier(Modifier::BOLD)),
                Span::styled(label, Style::default().fg(row_accent).add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(vec![
                Span::styled("   ", Style::default()),
                Span::styled(label, Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
            ])
        };

        let val_line = if is_selected {
            let mut spans = vec![
                Span::styled("      ◄ [", Style::default().fg(row_accent).add_modifier(Modifier::BOLD)),
            ];
            spans.extend(val_spans);
            spans.push(Span::styled("] ►", Style::default().fg(row_accent).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled("   (use ← / → to adjust)", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)));
            Line::from(spans)
        } else {
            let mut spans = vec![
                Span::styled("        [", Style::default().fg(row_accent).add_modifier(Modifier::BOLD)),
            ];
            spans.extend(val_spans);
            spans.push(Span::styled("]", Style::default().fg(row_accent).add_modifier(Modifier::BOLD)));
            Line::from(spans)
        };

        let row_p = Paragraph::new(vec![title_line, val_line]).alignment(Alignment::Left);
        row_p.render(r_area, buf);
    }

    // Static, non-selectable hotkey reference footer (split into 2 centered, un-truncated lines)
    let (footer_div_area, footer_area) = if show_full_ascii {
        (chunks[5], chunks[6])
    } else {
        (chunks[3], chunks[4])
    };

    let div_footer = Paragraph::new(Line::from(Span::styled(
        "─".repeat(footer_div_area.width as usize),
        Style::default().fg(palette.border_unfocused),
    )));
    div_footer.render(footer_div_area, buf);

    let sep_bullet = Span::styled("  •  ", Style::default().fg(palette.border_unfocused));

    let footer_line_1 = Line::from(vec![
        Span::styled("[↑/↓]", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Select", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet.clone(),
        Span::styled("[←/→]", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Adjust", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet.clone(),
        Span::styled("[Esc / ?]", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet.clone(),
        Span::styled("[Ctrl+Q]", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
    ]);

    let footer_line_2 = Line::from(vec![
        Span::styled("[F1]", Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)),
        Span::styled(" Code Editor", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet.clone(),
        Span::styled("[F2]", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
        Span::styled(" Live Preview", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet.clone(),
        Span::styled("[F7]", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Open File", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
        sep_bullet,
        Span::styled("[F5]", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
        Span::styled(" Rebuild", Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
    ]);

    let p_footer = Paragraph::new(vec![footer_line_1, footer_line_2]).alignment(Alignment::Center);
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
            settings.handle_key(KeyEvent::new(KeyCode::F(7), KeyModifiers::empty())),
            SettingsAction::OpenFile
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

    #[test]
    fn test_calculate_modal_area_dimensions() {
        let area = Rect::new(0, 0, 100, 30);
        let modal = calculate_modal_area(area);
        assert_eq!(modal.width, 84);
        assert_eq!(modal.height, 25);
        assert_eq!(modal.x, (100 - 84) / 2);
        assert_eq!(modal.y, (30 - 25) / 2);

        // Constrained screen
        let small_area = Rect::new(0, 0, 50, 18);
        let small_modal = calculate_modal_area(small_area);
        assert_eq!(small_modal.width, 48);
        assert_eq!(small_modal.height, 16);
    }

    #[test]
    fn test_render_settings_overlay_no_panic() {
        let state = SettingsState::new("micro", Quality::Medium, 30);
        let palette = ThemeId::GruvboxDark.palette();
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        render_settings_overlay(&state, area, &mut buf, &palette);

        // Verify overlay buffer has characters rendered
        assert!(buf.content.iter().any(|cell| cell.symbol() != " "));

        // Verify vibrant gold numbers are rendered for high contrast
        let gold = ratatui::style::Color::Rgb(255, 215, 0);
        let has_gold_number = buf.content.iter().any(|cell| cell.fg == gold && (cell.symbol() == "7" || cell.symbol() == "3"));
        assert!(has_gold_number, "Gold numbers must be rendered in settings overlay");
    }
}
