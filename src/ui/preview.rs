use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use crossterm::cursor::MoveTo;
use crossterm::execute;
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};
use crate::runner::FrameArtifacts;
use crate::ui::theme::ThemePalette;
use std::{
    io::{stdout, Write},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    Kitty,
    Halfblocks,
}

pub struct PreviewState {
    pub protocol: ImageProtocol,
    pub frames: Vec<PathBuf>,
    pub current_frame_idx: usize,
    pub is_playing: bool,
    pub fps: u32,
    pub playback_speed: f64,
    pub last_tick: Instant,
    pub frame_guard: Option<Arc<tempfile::TempDir>>,
    // Timeline metrics
    pub is_scrubbing: bool,
    pub force_redraw: bool,
    cached_frame_path: Option<PathBuf>,
    cached_image: Option<DynamicImage>,
    last_rendered_area: Option<Rect>,
    last_rendered_kitty_frame: Option<PathBuf>,
    last_rendered_kitty_area: Option<Rect>,
}

impl PreviewState {
    pub fn new(protocol_override: Option<&str>, fps: u32) -> Self {
        let protocol = match protocol_override {
            Some("kitty") => ImageProtocol::Kitty,
            Some("halfblocks") => ImageProtocol::Halfblocks,
            _ => detect_terminal_protocol(),
        };

        Self {
            protocol,
            frames: Vec::new(),
            current_frame_idx: 0,
            is_playing: true,
            fps: fps.clamp(1, 60),
            playback_speed: 1.0,
            last_tick: Instant::now(),
            frame_guard: None,
            is_scrubbing: false,
            force_redraw: false,

            cached_frame_path: None,
            cached_image: None,
            last_rendered_area: None,
            last_rendered_kitty_frame: None,
            last_rendered_kitty_area: None,
        }
    }

    pub fn set_artifacts(&mut self, artifacts: FrameArtifacts) {
        self.frame_guard = artifacts.temp_dir_guard;
        self.set_frames(artifacts.frames);
        self.is_playing = true;
        self.is_scrubbing = false;
        self.current_frame_idx = 0;
        self.last_tick = Instant::now();
        self.force_redraw = true;
    }

    pub fn set_frames(&mut self, new_frames: Vec<PathBuf>) {
        self.frames = new_frames;
        self.current_frame_idx = 0;
        self.is_playing = true;
        self.is_scrubbing = false;
        self.last_tick = Instant::now();
        self.cached_frame_path = None;
        self.cached_image = None;
        self.last_rendered_kitty_frame = None;
        self.force_redraw = true;
    }

    pub fn tick(&mut self) -> bool {
        if !self.is_playing || self.frames.len() <= 1 {
            return false;
        }

        let effective_fps = (self.fps as f64 * self.playback_speed).max(1.0);
        let frame_duration = Duration::from_secs_f64(1.0 / effective_fps);
        if self.last_tick.elapsed() >= frame_duration {
            self.last_tick = Instant::now();
            self.current_frame_idx = (self.current_frame_idx + 1) % self.frames.len();
            self.force_redraw = true;
            true
        } else {
            false
        }
    }

    pub fn toggle_play(&mut self) {
        self.is_playing = !self.is_playing;
        self.is_scrubbing = false;
    }

    pub fn step_next(&mut self) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            self.current_frame_idx = (self.current_frame_idx + 1) % self.frames.len();
            self.force_redraw = true;
        }
    }

    pub fn step_prev(&mut self) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            if self.current_frame_idx == 0 {
                self.current_frame_idx = self.frames.len() - 1;
            } else {
                self.current_frame_idx -= 1;
            }
            self.force_redraw = true;
        }
    }

    pub fn seek_by(&mut self, delta: i32) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            let len = self.frames.len() as i32;
            let new_idx = (self.current_frame_idx as i32 + delta).clamp(0, len - 1);
            self.current_frame_idx = new_idx as usize;
            self.force_redraw = true;
        }
    }

    pub fn seek_to_start(&mut self) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            self.current_frame_idx = 0;
            self.force_redraw = true;
        }
    }

    pub fn seek_to_end(&mut self) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            self.current_frame_idx = self.frames.len().saturating_sub(1);
            self.force_redraw = true;
        }
    }

    pub fn cycle_speed(&mut self, faster: bool) {
        let speeds = [0.5, 1.0, 2.0];
        let mut cur_idx = speeds
            .iter()
            .position(|&s| (s - self.playback_speed).abs() < 0.01)
            .unwrap_or(1);
        if faster {
            cur_idx = (cur_idx + 1) % speeds.len();
        } else if cur_idx == 0 {
            cur_idx = speeds.len() - 1;
        } else {
            cur_idx -= 1;
        }
        self.playback_speed = speeds[cur_idx];
    }

    fn current_frame_path(&self) -> Option<&PathBuf> {
        self.frames.get(self.current_frame_idx)
    }

    fn load_current_image(&mut self) -> Option<&DynamicImage> {
        let path = self.current_frame_path()?.clone();
        if self.cached_frame_path.as_ref() == Some(&path) && self.cached_image.is_some() {
            return self.cached_image.as_ref();
        }

        if let Ok(img) = image::open(&path) {
            self.cached_frame_path = Some(path);
            self.cached_image = Some(img);
            self.cached_image.as_ref()
        } else {
            None
        }
    }

    /// Renders the preview widget into the provided Ratatui buffer.
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        is_focused: bool,
        is_building: bool,
        border_type: BorderType,
        palette: &ThemePalette,
    ) {
        let border_color = if is_focused {
            palette.accent_mauve
        } else {
            palette.border_unfocused
        };

        let title = if self.frames.is_empty() {
            if is_building {
                if is_focused {
                    " █ Live Preview: Compiling... [ACTIVE] ".to_string()
                } else {
                    " ❪ Live Preview: Compiling... ❫ ".to_string()
                }
            } else if is_focused {
                " █ Live Preview [ACTIVE] ".to_string()
            } else {
                " ❪ Live Preview [F2 to focus] ❫ ".to_string()
            }
        } else if self.frames.len() == 1 {
            if is_building {
                if is_focused {
                    " █ Preview: Snapshot (1 frame) [ACTIVE] (Compiling...) ".to_string()
                } else {
                    " ❪ Preview: Snapshot (1 frame) [Compiling...] ❫ ".to_string()
                }
            } else if is_focused {
                " █ Preview: Snapshot (1 frame) [ACTIVE] ".to_string()
            } else {
                " ❪ Preview: Snapshot (1 frame) ❫ ".to_string()
            }
        } else {
            let status = if is_building {
                "PLAYING (Compiling update...)"
            } else if self.is_playing {
                "PLAYING"
            } else {
                "PAUSED"
            };
            if is_focused {
                format!(
                    " █ Preview: Frame {}/{} [{}] [ACTIVE] ",
                    self.current_frame_idx + 1,
                    self.frames.len(),
                    status
                )
            } else {
                format!(
                    " ❪ Preview: Frame {}/{} [{}] ❫ ",
                    self.current_frame_idx + 1,
                    self.frames.len(),
                    status
                )
            }
        };

        let block = Block::default()
            .title(title)
            .title_alignment(Alignment::Left)
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color));

        let inner_area = block.inner(area);
        block.render(area, buf);

        if self.frames.is_empty() {
            clear_kitty_images();
            let msg = if is_building {
                vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "⚙ Compiling Scene...",
                        Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Rendering animation & extracting video frames",
                        Style::default().fg(palette.accent_sapphire),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Live playback will begin automatically",
                        Style::default().fg(palette.text_muted),
                    )),
                ]
            } else {
                vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "No preview active",
                        Style::default().fg(palette.text_muted).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Type 'render' in console or press 'r' / F5 to compile",
                        Style::default().fg(palette.accent_sapphire),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Saving python file in editor (Ctrl+S) renders snapshot (-s)",
                        Style::default().fg(palette.text_muted),
                    )),
                ]
            };
            let p = Paragraph::new(msg).alignment(Alignment::Center);
            p.render(inner_area, buf);
            return;
        }

        if inner_area.width < 2 || inner_area.height < 2 {
            return;
        }

        // Layout: Reserve 2 rows at the bottom of inner_area for Timeline HUD when frames exist
        let (graphic_area, hud_area) = if self.frames.len() > 1 && inner_area.height >= 4 {
            let hud = Rect::new(inner_area.x, inner_area.bottom().saturating_sub(2), inner_area.width, 2);
            let graphic = Rect::new(inner_area.x, inner_area.y, inner_area.width, inner_area.height.saturating_sub(2));
            (graphic, Some(hud))
        } else {
            (inner_area, None)
        };

        match self.protocol {
            ImageProtocol::Halfblocks => {
                self.render_halfblocks(graphic_area, buf, palette);
            }
            ImageProtocol::Kitty => {
                self.render_kitty(graphic_area, buf, palette);
            }
        }

        // Render interactive timeline scrubber HUD
        if let Some(hud) = hud_area {
            let total_frames = self.frames.len();
            let current_idx = self.current_frame_idx;
            let fps = self.fps.max(1) as f64;
            let cur_secs = current_idx as f64 / fps;
            let total_secs = total_frames as f64 / fps;

            let track_width = (hud.width as usize).saturating_sub(44).max(10);
            let progress_ratio = if total_frames > 1 {
                current_idx as f64 / (total_frames - 1) as f64
            } else {
                0.0
            };
            let head_pos = ((track_width as f64) * progress_ratio).round() as usize;

            let mut track_bar = String::with_capacity(track_width);
            for i in 0..track_width {
                if i == head_pos {
                    track_bar.push('●');
                } else if i < head_pos {
                    track_bar.push('═');
                } else {
                    track_bar.push('─');
                }
            }

            let speed_badge = if (self.playback_speed - 1.0).abs() > 0.01 {
                format!(" [{:.1}x]", self.playback_speed)
            } else {
                String::new()
            };

            let hud_line = Line::from(vec![
                Span::styled(
                    format!("{:02}:{:05.2} ", (cur_secs / 60.0).floor() as u32, cur_secs % 60.0),
                    Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD),
                ),
                Span::styled("[ ", Style::default().fg(palette.text_dimmed)),
                Span::styled(track_bar, Style::default().fg(palette.accent_mauve)),
                Span::styled(" ] ", Style::default().fg(palette.text_dimmed)),
                Span::styled(
                    format!("{:02}:{:05.2} ", (total_secs / 60.0).floor() as u32, total_secs % 60.0),
                    Style::default().fg(palette.text_muted),
                ),
                Span::styled(
                    format!("(F:{}/{})", current_idx + 1, total_frames),
                    Style::default().fg(palette.text_dimmed),
                ),
                Span::styled(speed_badge, Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
            ]);

            // Top row of HUD is subtle divider line
            let sep_line = Line::from(Span::styled("─".repeat(hud.width as usize), Style::default().fg(palette.border_unfocused)));
            let p_sep = Paragraph::new(sep_line);
            p_sep.render(Rect::new(hud.x, hud.y, hud.width, 1), buf);

            let p_hud = Paragraph::new(hud_line).alignment(Alignment::Center);
            p_hud.render(Rect::new(hud.x, hud.y + 1, hud.width, 1), buf);
        }

        self.last_rendered_area = Some(inner_area);
    }

    /// Computes fitting dimensions preserving the exact aspect ratio (like VLC player).
    /// Uses detected or calibrated cell aspect ratio (height/width ≈ 2.25) so videos are never vertically stretched.
    /// Returns (fit_x, fit_y, fit_cols, fit_rows).
    fn compute_fit_dimensions(&mut self, area: Rect) -> (u16, u16, u16, u16) {
        // Default strictly to 16:9 if no frame is loaded yet or if dimensions are 0
        let (orig_w, orig_h) = if let Some(img) = self.load_current_image() {
            let (w, h) = img.dimensions();
            if w > 0 && h > 0 {
                (w, h)
            } else {
                (16, 9)
            }
        } else {
            (16, 9)
        };

        let aspect_img = (orig_w as f64) / (orig_h as f64);
        let max_cols = area.width as f64;
        let max_rows = area.height as f64;

        // Calibrated monospace cell aspect ratio (height / width ≈ 2.25)
        let cell_aspect = detect_cell_aspect_ratio();
        let effective_aspect = aspect_img * cell_aspect;

        let is_width_constrained = (max_cols / effective_aspect) <= max_rows;

        let (fit_cols, fit_rows, offset_x, offset_y) = if is_width_constrained {
            let fit_cols = area.width;
            let est_rows = (fit_cols as f64 / effective_aspect).round() as u16;
            let fit_rows = est_rows.clamp(1, area.height);
            let offset_x = 0;
            let offset_y = (area.height.saturating_sub(fit_rows)) / 2;
            (fit_cols, fit_rows, offset_x, offset_y)
        } else {
            let fit_rows = area.height;
            let est_cols = (fit_rows as f64 * effective_aspect).round() as u16;
            let fit_cols = est_cols.clamp(1, area.width);
            let offset_x = (area.width.saturating_sub(fit_cols)) / 2;
            let offset_y = 0;
            (fit_cols, fit_rows, offset_x, offset_y)
        };

        (
            area.x + offset_x,
            area.y + offset_y,
            fit_cols,
            fit_rows,
        )
    }

    fn render_halfblocks(&mut self, area: Rect, buf: &mut Buffer, palette: &ThemePalette) {
        // Clear background cells to solid black/theme bg creating VLC-style letterbox/pillarbox bars
        for r in 0..area.height {
            for c in 0..area.width {
                if let Some(cell) = buf.cell_mut((area.x + c, area.y + r)) {
                    cell.set_symbol(" ");
                    cell.set_style(Style::default().bg(palette.bg_dark));
                }
            }
        }

        let (fit_x, fit_y, fit_cols, fit_rows) = self.compute_fit_dimensions(area);
        let img = match self.load_current_image() {
            Some(i) => i.clone(),
            None => return,
        };

        let pixel_w = fit_cols as u32;
        let pixel_h = (fit_rows as u32) * 2;

        let resized = img.resize_exact(pixel_w, pixel_h, FilterType::Triangle);
        let rgba = resized.to_rgba8();

        for r in 0..fit_rows {
            let y_upper = (r as u32) * 2;
            let y_lower = y_upper + 1;

            for c in 0..fit_cols {
                let screen_x = fit_x + c;
                let screen_y = fit_y + r;

                if screen_x >= area.right() || screen_y >= area.bottom() {
                    continue;
                }

                let px_upper = rgba.get_pixel(c as u32, y_upper);
                let fg = Color::Rgb(px_upper[0], px_upper[1], px_upper[2]);

                let bg = if y_lower < pixel_h {
                    let px_lower = rgba.get_pixel(c as u32, y_lower);
                    Color::Rgb(px_lower[0], px_lower[1], px_lower[2])
                } else {
                    palette.bg_dark
                };

                if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
                    cell.set_symbol("▀");
                    cell.set_style(Style::default().fg(fg).bg(bg));
                }
            }
        }
    }

    fn render_kitty(&mut self, area: Rect, buf: &mut Buffer, palette: &ThemePalette) {
        // Clear background cells so previous buffer content does not shine through
        // and provides solid black letterbox/pillarbox margins (VLC style)
        for r in 0..area.height {
            for c in 0..area.width {
                if let Some(cell) = buf.cell_mut((area.x + c, area.y + r)) {
                    cell.set_symbol(" ");
                    cell.set_style(Style::default().bg(palette.bg_dark));
                }
            }
        }

        let (fit_x, fit_y, fit_cols, fit_rows) = self.compute_fit_dimensions(area);
        let fit_rect = Rect::new(fit_x, fit_y, fit_cols, fit_rows);

        let current_path = self.current_frame_path().cloned();
        let area_changed = self.last_rendered_kitty_area != Some(fit_rect);
        let frame_changed = self.last_rendered_kitty_frame != current_path;

        if self.force_redraw || area_changed || frame_changed {
            if let Some(ref path) = current_path {
                if area_changed {
                    clear_kitty_images();
                }

                let _ = emit_kitty_image_path(
                    path,
                    fit_x,
                    fit_y,
                    fit_cols,
                    fit_rows,
                );
                self.last_rendered_kitty_frame = current_path;
                self.last_rendered_kitty_area = Some(fit_rect);
                self.force_redraw = false;
            }
        }

        // Ratatui cell skipping & buffer protection:
        // Mark all cells covered by the kitty image as skipped so ratatui diffing doesn't repaint spaces over GPU image
        for r in 0..fit_rows {
            for c in 0..fit_cols {
                let screen_x = fit_x + c;
                let screen_y = fit_y + r;
                if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
                    cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
                }
            }
        }
    }
}

pub fn clear_kitty_images() {
    let mut out = stdout();
    let _ = write!(out, "\x1b_Ga=d,d=a,q=2\x1b\\");
    let _ = out.flush();
}

fn emit_kitty_image_path(
    path: &std::path::Path,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
) -> std::io::Result<()> {
    let mut out = stdout();
    execute!(out, MoveTo(x, y))?;

    // Canonicalize path so Kitty receives absolute path on filesystem
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = abs_path.to_string_lossy();
    let b64_path = BASE64.encode(path_str.as_bytes());

    // a=T: transmit and display
    // f=100: PNG format
    // t=f: transmit file path (zero copy)
    // i=1: image id
    // q=2: quiet mode
    // C=1: do not move cursor
    // z=-1: render behind text for clean modal overlays
    write!(
        out,
        "\x1b_Ga=T,f=100,t=f,i=1,q=2,C=1,z=-1,c={},r={};{}\x1b\\",
        cols, rows, b64_path
    )?;
    out.flush()
}

/// Returns the physical height/width ratio of a character cell in the terminal.
/// Defaults to 2.25 (calibrated for standard monospace fonts with line padding).
pub fn detect_cell_aspect_ratio() -> f64 {
    if let Ok(ws) = crossterm::terminal::window_size() {
        if ws.width > 0 && ws.height > 0 && ws.columns > 0 && ws.rows > 0 {
            let cell_w = ws.width as f64 / ws.columns as f64;
            let cell_h = ws.height as f64 / ws.rows as f64;
            if cell_w > 0.0 {
                let ratio = cell_h / cell_w;
                if (1.6..=3.0).contains(&ratio) {
                    return ratio;
                }
            }
        }
    }
    // Standard calibrated terminal cell aspect ratio (height / width ≈ 2.25)
    2.25
}

fn detect_terminal_protocol() -> ImageProtocol {
    if let Ok(v) = std::env::var("KITTY_WINDOW_ID") {
        if !v.is_empty() {
            return ImageProtocol::Kitty;
        }
    }

    if std::env::var("GHOSTTY_RESOURCES_DIR").is_ok()
        || std::env::var("GHOSTTY_BIN_DIR").is_ok()
        || std::env::var("KITTY_PID").is_ok()
        || std::env::var("WEZTERM_PANE").is_ok()
    {
        return ImageProtocol::Kitty;
    }

    if let Ok(term) = std::env::var("TERM") {
        let lower = term.to_lowercase();
        if lower.contains("kitty") || lower.contains("ghostty") || lower.contains("wezterm") {
            return ImageProtocol::Kitty;
        }
    }

    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        let lower = prog.to_lowercase();
        if lower.contains("kitty") || lower.contains("ghostty") || lower.contains("wezterm") {
            return ImageProtocol::Kitty;
        }
    }

    // Default to universal Unicode half-blocks
    ImageProtocol::Halfblocks
}
