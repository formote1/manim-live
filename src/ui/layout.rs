use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Minimum fraction of total width for the editor pane (prevents squashing).
pub const H_SPLIT_MIN: f64 = 0.20;
/// Maximum fraction of total width for the editor pane.
pub const H_SPLIT_MAX: f64 = 0.80;
/// Minimum fraction of right-column height for the preview pane.
pub const V_SPLIT_MIN: f64 = 0.15;
/// Maximum fraction of right-column height for the preview pane.
pub const V_SPLIT_MAX: f64 = 0.85;

pub struct AppLayout {
    pub editor_area: Rect,
    pub preview_area: Rect,
    pub console_area: Rect,
    pub status_bar_area: Rect,
    /// Column index of the vertical separator (border between editor and right column).
    #[allow(dead_code)]
    pub h_sep_col: u16,
    /// Row index of the horizontal separator (border between preview and console).
    #[allow(dead_code)]
    pub v_sep_row: u16,
}


impl AppLayout {
    /// Compute layout from split ratios.
    /// `h_split`: fraction [0,1] of total width allocated to the editor pane (clamped to [H_SPLIT_MIN, H_SPLIT_MAX]).
    /// `v_split`: fraction [0,1] of right-column height allocated to the preview pane (clamped to [V_SPLIT_MIN, V_SPLIT_MAX]).
    pub fn compute_with_splits(area: Rect, h_split: f64, v_split: f64) -> Self {
        // Clamp to safe limits
        let h_split = h_split.clamp(H_SPLIT_MIN, H_SPLIT_MAX);
        let v_split = v_split.clamp(V_SPLIT_MIN, V_SPLIT_MAX);

        // Vertical layout: main work area + status bar
        let root_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(area);

        let main_area = root_chunks[0];
        let status_bar_area = root_chunks[1];

        // Editor width in columns:
        // Clamped to split ratio and guaranteed at least 15 cols for editor and 20 cols for right side.
        let total_w = main_area.width.max(2);
        let min_editor_cols = 15.min(total_w.saturating_sub(1));
        let max_editor_cols = total_w.saturating_sub(20).max(min_editor_cols);
        let editor_cols = ((total_w as f64 * h_split).round() as u16)
            .clamp(min_editor_cols, max_editor_cols);
        let right_cols = total_w - editor_cols;

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(editor_cols),
                Constraint::Length(right_cols),
            ])
            .split(main_area);

        let editor_area = main_chunks[0];
        let right_column = main_chunks[1];

        // Preview height in rows:
        // Clamped to split ratio and guaranteed at least 4 rows for preview and 4 rows for console.
        let total_h = right_column.height.max(2);
        let min_preview_rows = 4.min(total_h.saturating_sub(1));
        let max_preview_rows = total_h.saturating_sub(4).max(min_preview_rows);
        let preview_rows = ((total_h as f64 * v_split).round() as u16)
            .clamp(min_preview_rows, max_preview_rows);
        let console_rows = total_h - preview_rows;


        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(preview_rows),
                Constraint::Length(console_rows),
            ])
            .split(right_column);

        let preview_area = right_chunks[0];
        let console_area = right_chunks[1];

        // The separator column is the right edge of the editor pane
        let h_sep_col = editor_area.right().saturating_sub(1);
        // The separator row is the bottom edge of the preview pane
        let v_sep_row = preview_area.bottom().saturating_sub(1);

        Self {
            editor_area,
            preview_area,
            console_area,
            status_bar_area,
            h_sep_col,
            v_sep_row,
        }
    }

    /// Default compute (55% / 50% splits).
    #[allow(dead_code)]
    pub fn compute(area: Rect) -> Self {
        Self::compute_with_splits(area, 0.55, 0.50)
    }

    /// Fullscreen preview layout: allocates entire work canvas to live preview.
    pub fn compute_fullscreen_preview(area: Rect) -> Self {
        let root_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(area);

        Self {
            editor_area: Rect::default(),
            preview_area: root_chunks[0],
            console_area: Rect::default(),
            status_bar_area: root_chunks[1],
            h_sep_col: 0,
            v_sep_row: 0,
        }
    }
}

