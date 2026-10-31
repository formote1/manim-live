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
    pub is_fullscreen: bool,
    pub last_hud_area: Option<Rect>,
    pub last_scrub_track: Option<(u16, u16)>,
    pub last_fullscreen_btn_area: Option<Rect>,
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
            is_fullscreen: false,
            last_hud_area: None,
            last_scrub_track: None,
            last_fullscreen_btn_area: None,

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

    pub fn seek_ratio(&mut self, ratio: f64) {
        if !self.frames.is_empty() {
            self.is_playing = false;
            self.is_scrubbing = true;
            let total = self.frames.len();
            let target = ((total.saturating_sub(1)) as f64 * ratio.clamp(0.0, 1.0)).round() as usize;
            self.current_frame_idx = target.min(total - 1);
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
                        "Saving python file in editor (Ctrl+S) automatically renders scene",
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

        // Explicitly clear all cells in inner_area so no previous compilation text or lingering messages remain
        for r in 0..inner_area.height {
            for c in 0..inner_area.width {
                if let Some(cell) = buf.cell_mut((inner_area.x + c, inner_area.y + r)) {
                    cell.set_symbol(" ");
                    cell.set_style(Style::default().bg(palette.bg_dark));
                }
            }
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

        let fs_btn_text = if self.is_fullscreen {
            " [ 🗗 Restore ] "
        } else {
            " [ ⛶ Fullscreen ] "
        };
        let fs_btn_w = fs_btn_text.chars().count() as u16;

        // Render interactive timeline scrubber HUD
        if let Some(hud) = hud_area {
            self.last_hud_area = Some(hud);
            let total_frames = self.frames.len();
            let current_idx = self.current_frame_idx;
            let fps = self.fps.max(1) as f64;
            let cur_secs = current_idx as f64 / fps;
            let total_secs = total_frames as f64 / fps;

            let cur_time_str = format!("{:02}:{:05.2} ", (cur_secs / 60.0).floor() as u32, cur_secs % 60.0);
            let total_time_str = format!("{:02}:{:05.2} ", (total_secs / 60.0).floor() as u32, total_secs % 60.0);
            let frame_counter = format!("(F:{}/{})", current_idx + 1, total_frames);

            let speed_badge = if (self.playback_speed - 1.0).abs() > 0.01 {
                format!(" [{:.1}x]", self.playback_speed)
            } else {
                String::new()
            };

            // Calculate track coordinates taking into account labels and fullscreen button
            let track_start_x = hud.x + cur_time_str.len() as u16 + 2;
            let right_reserved = total_time_str.len() + frame_counter.len() + speed_badge.len() + fs_btn_w as usize + 6;
            let track_width = (hud.width as usize).saturating_sub(cur_time_str.len() + 2 + right_reserved).max(8);
            self.last_scrub_track = Some((track_start_x, track_width as u16));

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

            const GOLD: ratatui::style::Color = ratatui::style::Color::Rgb(255, 215, 0);
            let hud_spans = vec![
                Span::styled(cur_time_str, Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
                Span::styled("[ ", Style::default().fg(palette.border_unfocused)),
                Span::styled(track_bar, Style::default().fg(palette.accent_mauve)),
                Span::styled(" ] ", Style::default().fg(palette.border_unfocused)),
                Span::styled(total_time_str, Style::default().fg(palette.text_primary).add_modifier(Modifier::BOLD)),
                Span::styled(frame_counter, Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
                Span::styled(speed_badge, Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
            ];

            // Top row of HUD is subtle divider line
            let sep_line = Line::from(Span::styled("─".repeat(hud.width as usize), Style::default().fg(palette.border_unfocused)));
            let p_sep = Paragraph::new(sep_line);
            p_sep.render(Rect::new(hud.x, hud.y, hud.width, 1), buf);

            let hud_left_w = hud.width.saturating_sub(fs_btn_w + 1);
            let p_hud = Paragraph::new(Line::from(hud_spans)).alignment(Alignment::Left);
            p_hud.render(Rect::new(hud.x, hud.y + 1, hud_left_w, 1), buf);

            // Fullscreen icon button at bottom right
            if hud.width >= fs_btn_w + 10 {
                let btn_rect = Rect::new(hud.right().saturating_sub(fs_btn_w), hud.y + 1, fs_btn_w, 1);
                self.last_fullscreen_btn_area = Some(btn_rect);
                let p_fs = Paragraph::new(Span::styled(
                    fs_btn_text,
                    Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD),
                ));
                p_fs.render(btn_rect, buf);
            } else {
                self.last_fullscreen_btn_area = None;
            }
        } else {
            self.last_hud_area = None;
            self.last_scrub_track = None;
            if inner_area.width >= fs_btn_w + 2 && inner_area.height >= 2 {
                let btn_rect = Rect::new(inner_area.right().saturating_sub(fs_btn_w + 1), inner_area.bottom().saturating_sub(1), fs_btn_w, 1);
                self.last_fullscreen_btn_area = Some(btn_rect);
                let p_fs = Paragraph::new(Span::styled(
                    fs_btn_text,
                    Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD),
                ));
                p_fs.render(btn_rect, buf);
            } else {
                self.last_fullscreen_btn_area = None;
            }
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

        // Diffing note: leaving cells with normal diff allows Ratatui to wipe
        // any previous compilation text messages from the screen upon frame arrival.
    }
}

pub fn clear_kitty_images() {
    let mut out = stdout();
    let _ = write!(out, "\x1b_Ga=d,d=a,q=2\x1b\\");
    let _ = out.flush();
}

pub fn clear_kitty_image_id(id: u32) {
    let mut out = stdout();
    let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", id);
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
    // z=1: render in front of text cells so previous text overlay is never visible
    write!(
        out,
        "\x1b_Ga=T,f=100,t=f,i=1,q=2,C=1,z=1,c={},r={};{}\x1b\\",
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

#[allow(dead_code)]
pub fn is_kitty_supported() -> bool {
    detect_terminal_protocol() == ImageProtocol::Kitty
}

pub fn detect_terminal_protocol() -> ImageProtocol {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::ThemeId;

    #[test]
    fn test_preview_buffer_cleared_on_frame_arrival() {
        let palette = ThemeId::CatppuccinMocha.palette();
        let area = Rect::new(0, 0, 60, 20);
        let mut buf = Buffer::empty(area);
        let mut state = PreviewState::new(Some("kitty"), 30);

        // Step 1: Compiling with no frames -> displays compilation message
        state.render(area, &mut buf, true, true, BorderType::Rounded, &palette);
        let mut content_compiling = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(c) = buf.cell((x, y)) {
                    content_compiling.push_str(c.symbol());
                }
            }
        }
        assert!(content_compiling.contains("Compiling Scene"));
        assert!(content_compiling.contains("Live playback will begin"));

        // Step 2: Frames arrive (simulate temp frame)
        let temp_dir = tempfile::tempdir().unwrap();
        let frame_path = temp_dir.path().join("frame_000.png");
        // Create 1x1 png image
        let img = image::RgbaImage::new(16, 9);
        img.save(&frame_path).unwrap();

        state.set_frames(vec![frame_path]);
        assert_eq!(state.frames.len(), 1);

        // Render again -> inner buffer must NOT contain compilation message
        state.render(area, &mut buf, true, false, BorderType::Rounded, &palette);
        let mut content_playback = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(c) = buf.cell((x, y)) {
                    content_playback.push_str(c.symbol());
                }
            }
        }
        assert!(!content_playback.contains("Compiling Scene"));
        assert!(!content_playback.contains("Live playback will begin"));
    }

    #[test]
    fn test_seek_ratio() {
        let mut state = PreviewState::new(Some("halfblocks"), 30);
        state.frames = vec![
            PathBuf::from("frame0.png"),
            PathBuf::from("frame1.png"),
            PathBuf::from("frame2.png"),
            PathBuf::from("frame3.png"),
            PathBuf::from("frame4.png"),
        ];
        state.seek_ratio(0.0);
        assert_eq!(state.current_frame_idx, 0);
        state.seek_ratio(0.5);
        assert_eq!(state.current_frame_idx, 2);
        state.seek_ratio(1.0);
        assert_eq!(state.current_frame_idx, 4);
    }
}

