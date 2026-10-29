pub mod console;
pub mod layout;
pub mod preview;
pub mod settings;
pub mod theme;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};
use theme::ThemePalette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Editor,
    Preview,
    Console,
}

pub fn format_breadcrumb(path: &std::path::Path, scene: Option<&str>) -> String {
    let path_str = path.to_string_lossy();
    let display_path = if let Ok(home) = std::env::var("HOME") {
        if let Some(stripped) = path_str.strip_prefix(&home) {
            format!("~{}", stripped)
        } else {
            path_str.to_string()
        }
    } else {
        path_str.to_string()
    };

    if let Some(sc) = scene {
        format!("{} › {}", display_path, sc)
    } else {
        display_path
    }
}

pub fn render_editor_container(
    area: Rect,
    buf: &mut Buffer,
    breadcrumb: &str,
    is_focused: bool,
    border_type: BorderType,
    palette: &ThemePalette,
) -> Rect {
    let border_color = if is_focused {
        palette.accent_mauve
    } else {
        palette.border_unfocused
    };

    let title = if is_focused {
        format!(" █ Code: {} [ACTIVE] ", breadcrumb)
    } else {
        format!(" ❪ Code: {} [F1 to Edit] ❫ ", breadcrumb)
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

pub fn render_status_bar(
    area: Rect,
    buf: &mut Buffer,
    focus: FocusPane,
    quality: &str,
    fps: u32,
    active_error: Option<&console::DiagnosticError>,
    palette: &ThemePalette,
) {
    let focus_badge = match focus {
        FocusPane::Editor => Span::styled(
            " █ F1 CODE ",
            Style::default()
                .fg(palette.bg_crust)
                .bg(palette.accent_mauve)
                .add_modifier(Modifier::BOLD),
        ),
        FocusPane::Preview => Span::styled(
            " █ F2 PREVIEW ",
            Style::default()
                .fg(palette.bg_crust)
                .bg(palette.accent_blue)
                .add_modifier(Modifier::BOLD),
        ),
        FocusPane::Console => Span::styled(
            " █ F3 CONSOLE ",
            Style::default()
                .fg(palette.bg_crust)
                .bg(palette.accent_sapphire)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let mut line_spans = vec![focus_badge, Span::raw("  ")];

    // Show jump to line error shortcut if compiler error is active
    if let Some(err) = active_error {
        line_spans.push(Span::styled(
            format!(" [F7: Jump to Line {}] ", err.line_number),
            Style::default()
                .fg(palette.bg_crust)
                .bg(palette.error_coral)
                .add_modifier(Modifier::BOLD),
        ));
        line_spans.push(Span::raw("  "));
    }

    line_spans.push(Span::styled(
        format!("({} • {}fps)", quality, fps),
        Style::default().fg(palette.text_dimmed),
    ));

    let line = Line::from(line_spans);
    let p = Paragraph::new(line)
        .alignment(Alignment::Center)
        .style(Style::default().bg(palette.bg_crust));
    p.render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use theme::ThemeId;

    #[test]
    fn test_status_bar_only_shows_current_badge() {
        let palette = ThemeId::CatppuccinMocha.palette();
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);

        render_status_bar(
            area,
            &mut buf,
            FocusPane::Editor,
            "Low (480p)",
            30,
            None,
            &palette,
        );

        let content = (0..area.width)
            .map(|x| buf.cell((x, 0)).unwrap().symbol())
            .collect::<String>();

        // Current badge is shown
        assert!(content.contains("F1 CODE"));
        // Status info is shown
        assert!(content.contains("Low (480p) • 30fps"));
        // Old available options pills are NOT shown
        assert!(!content.contains("[F2 Preview]"));
        assert!(!content.contains("[F3 Console]"));
        assert!(!content.contains("[F4 Settings]"));
        assert!(!content.contains("[F5 Build]"));
        assert!(!content.contains("[F8 Scenes]"));
        assert!(!content.contains("[Ctrl+Q Quit]"));
    }
}
