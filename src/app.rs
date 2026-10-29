use crate::{
    pty::PtyManager,
    runner::{BuildEvent, BuildRunner, Quality},
    ui::{
        console::{BuildState, ConsoleAction, ConsoleState},
        format_breadcrumb,
        layout::{AppLayout, H_SPLIT_MAX, H_SPLIT_MIN, V_SPLIT_MAX, V_SPLIT_MIN},
        preview::{clear_kitty_images, PreviewState},
        render_editor_container, render_status_bar,
        settings::{quality_label, render_settings_overlay, SettingsAction, SettingsState},
        theme::{ThemeId, ThemePalette},
        FocusPane,
    },
    watcher::FileWatcher,
};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Widget},
    Terminal,
};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct AppConfig {
    pub file_path: PathBuf,
    pub scene_name: Option<String>,
    pub editor_cmd: String,
    pub quality: Quality,
    pub fps: u32,
    pub protocol: Option<String>,
    pub manim_bin: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScenePaletteState {
    pub is_open: bool,
    pub available_scenes: Vec<String>,
    pub selected_idx: usize,
    pub search_query: String,
}

impl ScenePaletteState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            available_scenes: Vec::new(),
            selected_idx: 0,
            search_query: String::new(),
        }
    }

    pub fn filtered_scenes(&self) -> Vec<&String> {
        let q = self.search_query.trim().to_lowercase();
        if q.is_empty() {
            self.available_scenes.iter().collect()
        } else {
            self.available_scenes
                .iter()
                .filter(|s| s.to_lowercase().contains(&q))
                .collect()
        }
    }
}

/// Which separator is being dragged, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    /// Vertical separator between editor and right column.
    HSep,
    /// Horizontal separator between preview and console.
    VSep,
}

pub struct App {
    pub config: AppConfig,
    pub focus: FocusPane,
    pub pty: Option<PtyManager>,
    pub _watcher: Option<FileWatcher>,
    pub runner: BuildRunner,
    pub console_state: ConsoleState,
    pub preview_state: PreviewState,
    pub rx_watch: Receiver<PathBuf>,
    pub rx_build: Receiver<BuildEvent>,
    should_quit: bool,
    last_editor_size: (u16, u16),
    last_editor_inner: Rect,
    last_editor_area: Rect,
    last_preview_area: Rect,
    last_console_area: Rect,
    /// Fraction [0,1] of terminal width given to the editor pane.
    h_split: f64,
    /// Fraction [0,1] of right-column height given to the preview pane.
    v_split: f64,
    /// Currently dragged separator, if any.
    drag_target: Option<DragTarget>,
    /// Settings overlay state
    pub settings_state: SettingsState,
    /// Whether the settings overlay modal is currently displayed
    pub is_settings_open: bool,
    /// Scene palette state
    pub scene_palette: ScenePaletteState,
    /// Active theme color palette
    pub palette: ThemePalette,
    /// Whether the preview pane is currently expanded to fullscreen
    pub is_preview_fullscreen: bool,
    /// Last known cursor position inside the editor pane
    pub last_cursor_pos: Option<(u16, u16)>,
}

impl App {
    pub fn new(config: AppConfig) -> Result<Self> {
        let (tx_watch, rx_watch) = unbounded();
        let (tx_build, rx_build) = unbounded();

        let watcher = FileWatcher::new(
            &config.file_path,
            Duration::from_millis(300),
            tx_watch,
        )
        .ok();

        let runner = BuildRunner::new(tx_build);
        let preview_state = PreviewState::new(config.protocol.as_deref(), config.fps);
        let console_state = ConsoleState::new();

        // Spawn PTY with chosen editor
        let pty = PtyManager::spawn(&config.editor_cmd, &config.file_path, 80, 24).ok();

        let settings_state = SettingsState::new(&config.editor_cmd, config.quality, config.fps);
        let palette = ThemeId::CatppuccinMocha.palette();

        Ok(Self {
            config,
            focus: FocusPane::Editor,
            pty,
            _watcher: watcher,
            runner,
            console_state,
            preview_state,
            rx_watch,
            rx_build,
            should_quit: false,
            last_editor_size: (80, 24),
            last_editor_inner: Rect::default(),
            last_editor_area: Rect::default(),
            last_preview_area: Rect::default(),
            last_console_area: Rect::default(),
            h_split: 0.55,
            v_split: 0.50,
            drag_target: None,
            settings_state,
            is_settings_open: false,
            scene_palette: ScenePaletteState::new(),
            palette,
            is_preview_fullscreen: false,
            last_cursor_pos: None,
        })
    }

    pub fn trigger_rebuild(&mut self) {
        self.runner.trigger_build(
            &self.config.file_path,
            self.config.scene_name.as_deref(),
            self.config.quality,
            self.config.fps,
            self.config.manim_bin.as_deref(),
        );
    }

    pub fn switch_editor(&mut self, new_cmd: &str) -> Result<()> {
        let trimmed = new_cmd.trim();
        if trimmed.is_empty() || self.config.editor_cmd == trimmed {
            return Ok(());
        }
        self.config.editor_cmd = trimmed.to_string();
        let (cols, rows) = self.last_editor_size;
        match PtyManager::spawn(&self.config.editor_cmd, &self.config.file_path, cols, rows) {
            Ok(pty) => {
                self.pty = Some(pty);
                self.console_state.add_line(format!("Switched editor to '{}'", trimmed));
            }
            Err(e) => {
                self.console_state.add_line(format!("✖ Failed to launch editor '{}': {}", trimmed, e));
            }
        }
        Ok(())
    }

    pub fn save_settings(&self) {
        let mut store = crate::config::SettingsStore::load();
        let _ = store.set_for_file(&self.config.file_path, self.config.quality, self.config.fps);
    }
}

fn apply_backdrop_blur(buf: &mut Buffer, focused_area: Option<Rect>) {
    let dim_rgb = |r: u8, g: u8, b: u8, factor: f32, tint: (u8, u8, u8)| -> (u8, u8, u8) {
        let nr = ((r as f32 * factor) + (tint.0 as f32 * (1.0 - factor))) as u8;
        let ng = ((g as f32 * factor) + (tint.1 as f32 * (1.0 - factor))) as u8;
        let nb = ((b as f32 * factor) + (tint.2 as f32 * (1.0 - factor))) as u8;
        (nr, ng, nb)
    };

    let dim_color = |color: ratatui::style::Color, factor: f32, tint: (u8, u8, u8)| -> ratatui::style::Color {
        match color {
            ratatui::style::Color::Rgb(r, g, b) => {
                let (nr, ng, nb) = dim_rgb(r, g, b, factor, tint);
                ratatui::style::Color::Rgb(nr, ng, nb)
            }
            ratatui::style::Color::Indexed(idx) => {
                if idx == 0 {
                    ratatui::style::Color::Rgb(tint.0, tint.1, tint.2)
                } else {
                    ratatui::style::Color::DarkGray
                }
            }
            ratatui::style::Color::Black | ratatui::style::Color::Reset => {
                ratatui::style::Color::Rgb(tint.0, tint.1, tint.2)
            }
            ratatui::style::Color::White | ratatui::style::Color::Gray => {
                let (nr, ng, nb) = dim_rgb(180, 180, 180, factor, tint);
                ratatui::style::Color::Rgb(nr, ng, nb)
            }
            _ => ratatui::style::Color::DarkGray,
        }
    };

    let tint = (15, 17, 26);
    let area = *buf.area();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let is_inside_focus = if let Some(f) = focused_area {
                x >= f.left() && x < f.right() && y >= f.top() && y < f.bottom()
            } else {
                false
            };

            if let Some(cell) = buf.cell_mut((x, y)) {
                if is_inside_focus {
                    // Inside focused section: a bit more blurry / darker tint so background image is suppressed
                    cell.fg = dim_color(cell.fg, 0.15, tint);
                    cell.bg = dim_color(cell.bg, 0.10, tint);
                    cell.modifier.insert(ratatui::style::Modifier::DIM);
                } else {
                    // Surroundings: less blurry so surroundings remain visible
                    cell.fg = dim_color(cell.fg, 0.70, tint);
                    cell.bg = dim_color(cell.bg, 0.55, tint);
                }
            }
        }
    }
}

impl App {
    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let mut needs_redraw = true;

        while !self.should_quit {
            // Drain background file watcher events
            let mut got_save = false;
            while let Ok(_saved_path) = self.rx_watch.try_recv() {
                got_save = true;
            }

            if got_save && !matches!(self.console_state.build_state, BuildState::Building { .. }) {
                self.console_state.add_line("─── File saved: Automatically rendering scene ───".to_string());
                self.trigger_rebuild();
                needs_redraw = true;
            }

            // Drain build runner events
            while let Ok(event) = self.rx_build.try_recv() {
                needs_redraw = true;
                match event {
                    BuildEvent::Started { command } => {
                        self.console_state.build_state = BuildState::Building {
                            start_time: Instant::now(),
                            progress: 0.0,
                            current_step: "Starting compilation...".to_string(),
                        };
                        self.console_state.add_line(format!("$ {}", command));
                    }
                    BuildEvent::OutputLine(line) => {
                        self.console_state.add_line(line);
                    }
                    BuildEvent::Progress { percentage, message } => {
                        self.console_state.set_progress(percentage, message);
                    }
                    BuildEvent::Finished {
                        success,
                        duration,
                        output_path,
                        artifacts,
                    } => {
                        if success {
                            self.console_state.build_state = BuildState::Success { duration };
                            if let Some(ref path) = output_path {
                                self.console_state.add_line(format!("✔ Build complete: {}", path.display()));
                            }
                            self.preview_state.set_artifacts(artifacts);
                            self.preview_state.is_playing = true;
                            self.preview_state.is_scrubbing = false;
                            self.preview_state.current_frame_idx = 0;
                            self.preview_state.last_tick = Instant::now();
                            self.preview_state.force_redraw = true;
                        } else {
                            self.console_state.build_state = BuildState::Failed {
                                duration,
                                error_summary: Some("Build failed. Check error log above.".to_string()),
                            };
                            self.console_state.add_line("✖ Build failed with errors.".to_string());
                            // Section 9.3: Emit terminal bell on build failure
                            print!("\x07");
                            let _ = std::io::stdout().flush();
                        }
                    }
                }
            }

            // Advance frame animation if playing video
            if self.preview_state.tick() {
                needs_redraw = true;
            }

            // Check if PTY virtual screen has updated
            if let Some(ref pty) = self.pty {
                if pty.is_dirty() {
                    // Small yield lets any multi-chunk escape sequences from Vim
                    // (such as statusline updates followed by cursor repositioning)
                    // finish processing into vt100 before rendering.
                    std::thread::sleep(Duration::from_millis(2));
                    needs_redraw = true;
                }
            }

            // Render TUI only when state is dirty
            if needs_redraw {
                let term_size = terminal.size().map_err(|e| anyhow::anyhow!("Terminal size error: {e}"))?;
                let layout = if self.is_preview_fullscreen {
                    AppLayout::compute_fullscreen_preview(Rect::new(0, 0, term_size.width, term_size.height))
                } else {
                    AppLayout::compute_with_splits(
                        Rect::new(0, 0, term_size.width, term_size.height),
                        self.h_split,
                        self.v_split,
                    )
                };
                self.last_editor_area = layout.editor_area;
                self.last_preview_area = layout.preview_area;
                self.last_console_area = layout.console_area;

                let mut out = std::io::stdout();
                // 1. Begin Synchronized Update (DECSET 2026) and hide cursor.
                // This instructs terminal emulators (Kitty, WezTerm, Ghostty) to buffer all screen updates
                // and defer cursor trail calculations until the matching 2026l sequence.
                write!(out, "\x1b[?2026h\x1b[?25l")?;
                out.flush()?;

                let mut cursor_pos: Option<(u16, u16)> = None;
                let mut recorded_editor_inner = None;

                let is_hi_contrast = self.palette == ThemeId::HighContrast.palette();
                let (border_type_focused, border_type_unfocused) = if is_hi_contrast {
                    (ratatui::widgets::BorderType::Plain, ratatui::widgets::BorderType::Plain)
                } else {
                    (ratatui::widgets::BorderType::Thick, ratatui::widgets::BorderType::Rounded)
                };

                let editor_focused = self.focus == FocusPane::Editor && !self.is_settings_open && !self.scene_palette.is_open;
                let preview_focused = (self.focus == FocusPane::Preview || self.is_preview_fullscreen) && !self.is_settings_open && !self.scene_palette.is_open;
                let console_focused = self.focus == FocusPane::Console && !self.is_settings_open && !self.scene_palette.is_open;

                terminal.draw(|frame| {
                    let layout = if self.is_preview_fullscreen {
                        AppLayout::compute_fullscreen_preview(frame.area())
                    } else {
                        AppLayout::compute_with_splits(frame.area(), self.h_split, self.v_split)
                    };

                    // 1. Editor Pane (skipped when in fullscreen preview)
                    if !self.is_preview_fullscreen && layout.editor_area.width >= 5 {
                        let editor_border = if editor_focused { border_type_focused } else { border_type_unfocused };
                        let breadcrumb = format_breadcrumb(&self.config.file_path, self.config.scene_name.as_deref());
                        let editor_inner = render_editor_container(
                            layout.editor_area,
                            frame.buffer_mut(),
                            &breadcrumb,
                            editor_focused,
                            editor_border,
                            &self.palette,
                        );
                        recorded_editor_inner = Some(editor_inner);

                        if let Some(ref mut pty) = self.pty {
                            if editor_inner.width >= 10 && editor_inner.height >= 5 {
                                let cur_size = (editor_inner.width, editor_inner.height);
                                if self.last_editor_size != cur_size {
                                    pty.resize(editor_inner.width, editor_inner.height);
                                    self.last_editor_size = cur_size;
                                }
                            }
                            let pty_cursor = pty.render_to_buffer(editor_inner, frame.buffer_mut());
                            if editor_focused {
                                cursor_pos = pty_cursor;
                            }
                        }
                    }

                    // 2. Preview Pane
                    // While modal is open, clear Kitty graphics and draw empty pane so nothing overlays modal
                    let preview_border = if preview_focused { border_type_focused } else { border_type_unfocused };
                    let is_building = matches!(self.console_state.build_state, BuildState::Building { .. });
                    if !self.is_settings_open && !self.scene_palette.is_open {
                        self.preview_state.is_fullscreen = self.is_preview_fullscreen;
                        self.preview_state.render(
                            layout.preview_area,
                            frame.buffer_mut(),
                            preview_focused,
                            is_building,
                            preview_border,
                            &self.palette,
                        );
                    } else {
                        clear_kitty_images();
                        let block = ratatui::widgets::Block::default()
                            .title(" ❪ Live Preview ❫ ")
                            .borders(ratatui::widgets::Borders::ALL)
                            .border_type(preview_border)
                            .border_style(ratatui::style::Style::default().fg(self.palette.border_unfocused));
                        let inner = block.inner(layout.preview_area);
                        frame.render_widget(block, layout.preview_area);
                        for r in 0..inner.height {
                            for c in 0..inner.width {
                                if let Some(cell) = frame.buffer_mut().cell_mut((inner.x + c, inner.y + r)) {
                                    cell.set_symbol(" ");
                                    cell.set_style(ratatui::style::Style::default().bg(self.palette.bg_dark));
                                }
                            }
                        }
                    }

                    // 3. Console Pane (skipped when in fullscreen preview)
                    if !self.is_preview_fullscreen && layout.console_area.height >= 2 {
                        let console_border = if console_focused { border_type_focused } else { border_type_unfocused };
                        self.console_state.render(
                            layout.console_area,
                            frame.buffer_mut(),
                            console_focused,
                            console_border,
                            &self.palette,
                        );
                    }

                    // 4. Status Bar
                    let quality_label = quality_label(self.config.quality);
                    render_status_bar(
                        layout.status_bar_area,
                        frame.buffer_mut(),
                        self.focus,
                        quality_label,
                        self.config.fps,
                        self.console_state.active_error.as_ref(),
                        &self.palette,
                    );

                    // 5. Settings App Overlay (if open)
                    if self.is_settings_open {
                        let modal_area = crate::ui::settings::calculate_modal_area(frame.area());
                        apply_backdrop_blur(frame.buffer_mut(), Some(modal_area));
                        render_settings_overlay(
                            &self.settings_state,
                            frame.area(),
                            frame.buffer_mut(),
                            &self.palette,
                        );
                    } else if self.scene_palette.is_open {
                        let palette_w = 60.min(frame.area().width.saturating_sub(4));
                        let palette_h = 16.min(frame.area().height.saturating_sub(4));
                        let off_x = (frame.area().width.saturating_sub(palette_w)) / 2;
                        let off_y = (frame.area().height.saturating_sub(palette_h)) / 2;
                        let palette_area = Rect::new(frame.area().x + off_x, frame.area().y + off_y, palette_w, palette_h);
                        apply_backdrop_blur(frame.buffer_mut(), Some(palette_area));
                        render_scene_palette_overlay(
                            &self.scene_palette,
                            self.config.scene_name.as_deref(),
                            frame.area(),
                            frame.buffer_mut(),
                            &self.palette,
                        );
                    }
                })
                .map_err(|e| anyhow::anyhow!("Terminal draw error: {e}"))?;

                if let Some(inner) = recorded_editor_inner {
                    self.last_editor_inner = inner;
                }

                // Explicitly position the hardware cursor inside the synchronized block.
                // In Editor focus, move the cursor to (cx, cy) and show it.
                // In other panes or overlays, keep the hardware cursor hidden.
                if editor_focused {
                    if let Some((cx, cy)) = cursor_pos {
                        self.last_cursor_pos = Some((cx, cy));
                        crossterm::execute!(
                            out,
                            crossterm::cursor::MoveTo(cx, cy),
                            crossterm::cursor::Show
                        )?;
                    } else if let Some((lx, ly)) = self.last_cursor_pos {
                        crossterm::execute!(
                            out,
                            crossterm::cursor::MoveTo(lx, ly),
                            crossterm::cursor::Hide
                        )?;
                    } else {
                        crossterm::execute!(out, crossterm::cursor::Hide)?;
                    }
                } else {
                    crossterm::execute!(out, crossterm::cursor::Hide)?;
                }

                // 2. End Synchronized Update (DECSET 2026).
                // Kitty receives the entire frame atomically. The cursor trail shader calculates
                // movement between the start and end of the frame without any intermediate flicker!
                write!(out, "\x1b[?2026l")?;
                out.flush()?;

                needs_redraw = false;
            }

            // Poll for keyboard / terminal events (5ms when animating to ensure smooth playback)
            let poll_duration = if self.preview_state.is_playing && self.preview_state.frames.len() > 1 {
                Duration::from_millis(5)
            } else {
                Duration::from_millis(25)
            };

            if event::poll(poll_duration)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key)?;
                        needs_redraw = true;
                    }
                    Event::Mouse(mouse) => {
                        if !matches!(mouse.kind, MouseEventKind::Moved) {
                            self.handle_mouse(mouse)?;
                            needs_redraw = true;
                        }
                    }
                    Event::Resize(_, _) => {
                        clear_kitty_images();
                        self.preview_state.force_redraw = true;
                        needs_redraw = true;
                    }
                    _ => {}
                }
            }

            // Check if child editor process exited
            if let Some(ref mut pty) = self.pty {
                if !pty.is_alive() {
                    // Editor exited - cleanly terminate app
                    self.should_quit = true;
                }
            }
        }

        clear_kitty_images();
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Global quit hotkey (Ctrl+Q is the one and only quitting option):
        // NOTE: KeyCode::Char('c') is explicitly NOT handled here so Ctrl+C is passed to micro/vim!
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
        {
            self.should_quit = true;
            return Ok(());
        }

        // If Scene Palette overlay is open, route all input to scene_palette
        if self.scene_palette.is_open {
            match key.code {
                KeyCode::Esc | KeyCode::F(8) => {
                    self.scene_palette.is_open = false;
                    self.preview_state.force_redraw = true;
                }
                KeyCode::Up => {
                    self.scene_palette.selected_idx = self.scene_palette.selected_idx.saturating_sub(1);
                }
                KeyCode::Down => {
                    let max_idx = self.scene_palette.filtered_scenes().len().saturating_sub(1);
                    self.scene_palette.selected_idx = (self.scene_palette.selected_idx + 1).min(max_idx);
                }
                KeyCode::Enter => {
                    let selected_scene = {
                        let filtered = self.scene_palette.filtered_scenes();
                        filtered.get(self.scene_palette.selected_idx).copied().cloned()
                    };
                    if let Some(sc) = selected_scene {
                        self.config.scene_name = Some(sc.clone());
                        self.scene_palette.is_open = false;
                        self.console_state.add_line(format!("Switched scene to: {}", sc));
                        self.trigger_rebuild();
                        self.preview_state.force_redraw = true;
                    }
                }
                KeyCode::Backspace => {
                    self.scene_palette.search_query.pop();
                    self.scene_palette.selected_idx = 0;
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.scene_palette.search_query.push(c);
                    self.scene_palette.selected_idx = 0;
                }
                _ => {}
            }
            return Ok(());
        }

        // If Settings overlay is open, route all input to SettingsState
        if self.is_settings_open {
            match self.settings_state.handle_key(key) {
                SettingsAction::None => {}
                SettingsAction::Close => {
                    self.is_settings_open = false;
                    crate::ui::settings::clear_settings_logo();
                    self.preview_state.force_redraw = true;
                }
                SettingsAction::Quit => {
                    self.is_settings_open = false;
                    crate::ui::settings::clear_settings_logo();
                    self.should_quit = true;
                }
                SettingsAction::ApplyTheme(theme_id) => {
                    self.palette = theme_id.palette();
                }
                SettingsAction::ApplyEditor(cmd) => {
                    let _ = self.switch_editor(&cmd);
                }
                SettingsAction::ApplyQuality(q) => {
                    self.config.quality = q;
                    self.save_settings();
                    self.preview_state.force_redraw = true;
                }
                SettingsAction::ApplyFps(fps) => {
                    self.config.fps = fps;
                    self.preview_state.fps = fps;
                    self.save_settings();
                    self.preview_state.force_redraw = true;
                }
                SettingsAction::FocusPane(target) => {
                    self.is_settings_open = false;
                    crate::ui::settings::clear_settings_logo();
                    self.focus = target;
                    self.preview_state.force_redraw = true;
                }
                SettingsAction::TriggerRebuild => {
                    self.is_settings_open = false;
                    crate::ui::settings::clear_settings_logo();
                    self.preview_state.force_redraw = true;
                    self.console_state.add_line("─── Triggering manual rebuild (F5 full video) ───".to_string());
                    self.trigger_rebuild();
                }
                SettingsAction::OpenScenes => {
                    self.is_settings_open = false;
                    crate::ui::settings::clear_settings_logo();
                    let discovered = crate::ui::console::detect_all_scenes(&self.config.file_path);
                    let active = self.config.scene_name.as_deref();
                    let sel = discovered.iter().position(|s| Some(s.as_str()) == active).unwrap_or(0);
                    self.scene_palette.available_scenes = discovered;
                    self.scene_palette.selected_idx = sel;
                    self.scene_palette.search_query.clear();
                    self.scene_palette.is_open = true;
                    clear_kitty_images();
                }
            }
            return Ok(());
        }

        // If preview is currently fullscreen, Esc / f / F / F11 restores the normal layout
        if self.is_preview_fullscreen
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::F(11))
        {
            self.is_preview_fullscreen = false;
            clear_kitty_images();
            self.preview_state.force_redraw = true;
            return Ok(());
        }

        // Global hotkey F4 toggles settings overlay from any pane
        if key.code == KeyCode::F(4) {
            self.is_settings_open = !self.is_settings_open;
            if self.is_settings_open {
                clear_kitty_images();
            } else {
                crate::ui::settings::clear_settings_logo();
                self.preview_state.force_redraw = true;
            }
            return Ok(());
        }

        // Global hotkey F8 or Ctrl+P toggles Scene Palette overlay
        if key.code == KeyCode::F(8)
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')))
        {
            if self.scene_palette.is_open {
                self.scene_palette.is_open = false;
                self.preview_state.force_redraw = true;
            } else {
                let discovered = crate::ui::console::detect_all_scenes(&self.config.file_path);
                let active = self.config.scene_name.as_deref();
                let sel = discovered.iter().position(|s| Some(s.as_str()) == active).unwrap_or(0);
                self.scene_palette.available_scenes = discovered;
                self.scene_palette.selected_idx = sel;
                self.scene_palette.search_query.clear();
                self.scene_palette.is_open = true;
                clear_kitty_images();
            }
            return Ok(());
        }

        // Global hotkey F7 jumps to active compiler error line
        if key.code == KeyCode::F(7) {
            if let Some(ref err) = self.console_state.active_error {
                let line_no = err.line_number;
                if let Some(ref mut pty) = self.pty {
                    let _ = pty.jump_to_line(line_no, &self.config.editor_cmd);
                    self.focus = FocusPane::Editor;
                    self.console_state.add_line(format!("Jumped editor to line {}", line_no));
                }
            }
            return Ok(());
        }

        // Backend test navigation: Ctrl+1 -> Editor, Ctrl+2 -> Preview, Ctrl+3 -> Console
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('1') => {
                    self.focus = FocusPane::Editor;
                    return Ok(());
                }
                KeyCode::Char('2') => {
                    self.focus = FocusPane::Preview;
                    return Ok(());
                }
                KeyCode::Char('3') => {
                    self.focus = FocusPane::Console;
                    return Ok(());
                }
                _ => {}
            }
        }

        // Global direct jump binds requested by user:
        // F1 -> Code Editor, F2 -> Live Preview, F3 -> Terminal Console
        match key.code {
            KeyCode::F(1) => {
                self.focus = FocusPane::Editor;
                return Ok(());
            }
            KeyCode::F(2) => {
                self.focus = FocusPane::Preview;
                return Ok(());
            }
            KeyCode::F(3) => {
                self.focus = FocusPane::Console;
                return Ok(());
            }
            _ => {}
        }

        // Global cycling hotkeys with Alt: Alt+Right/Down -> next pane, Alt+Left/Up -> prev pane
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Right | KeyCode::Down => {
                    self.focus = match self.focus {
                        FocusPane::Editor => FocusPane::Preview,
                        FocusPane::Preview => FocusPane::Console,
                        FocusPane::Console => FocusPane::Editor,
                    };
                    return Ok(());
                }
                KeyCode::Left | KeyCode::Up => {
                    self.focus = match self.focus {
                        FocusPane::Editor => FocusPane::Console,
                        FocusPane::Preview => FocusPane::Editor,
                        FocusPane::Console => FocusPane::Preview,
                    };
                    return Ok(());
                }
                _ => {}
            }
        }

        match self.focus {
            FocusPane::Editor => {
                if key.code == KeyCode::F(6) {
                    self.focus = FocusPane::Preview;
                    return Ok(());
                }

                if key.code == KeyCode::F(5) {
                    self.console_state.add_line("─── Triggering manual rebuild (F5 full video) ───".to_string());
                    self.trigger_rebuild();
                    return Ok(());
                }

                // Forward all keystrokes (including Tab, Ctrl+C, Esc, etc.) directly to the editor PTY
                if let Some(ref mut pty) = self.pty {
                    let _ = pty.handle_key_event(key);
                }
            }
            FocusPane::Preview => {
                match key.code {
                    // Open settings overlay
                    KeyCode::Char('?') => {
                        self.is_settings_open = true;
                        clear_kitty_images();
                    }

                    // Navigation & Focus (Tab / F6 cycles to Console, BackTab to Editor)
                    KeyCode::Tab | KeyCode::F(6) => {
                        self.focus = FocusPane::Console;
                    }
                    KeyCode::BackTab => {
                        self.focus = FocusPane::Editor;
                    }
                    KeyCode::Char('i') | KeyCode::Enter => {
                        self.focus = FocusPane::Editor;
                    }

                    // Build actions
                    KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::F(11) => {
                        self.is_preview_fullscreen = !self.is_preview_fullscreen;
                        clear_kitty_images();
                        self.preview_state.force_redraw = true;
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') | KeyCode::F(5) => {
                        self.console_state.add_line("─── Triggering manual rebuild (full video) ───".to_string());
                        self.trigger_rebuild();
                    }

                    // Preview & Scrubbing Controls (Section 5.1)
                    KeyCode::Char(' ') => {
                        if self.preview_state.frames.len() <= 1 {
                            self.console_state.add_line("─── Triggering full video animation build (Space) ───".to_string());
                            self.trigger_rebuild();
                        } else {
                            self.preview_state.toggle_play();
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('L') {
                            self.preview_state.seek_by(10);
                        } else {
                            self.preview_state.step_next();
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('H') {
                            self.preview_state.seek_by(-10);
                        } else {
                            self.preview_state.step_prev();
                        }
                    }
                    KeyCode::Char('L') => {
                        self.preview_state.seek_by(10);
                    }
                    KeyCode::Char('H') => {
                        self.preview_state.seek_by(-10);
                    }
                    KeyCode::Char('0') | KeyCode::Home => {
                        self.preview_state.seek_to_start();
                    }
                    KeyCode::Char('$') | KeyCode::End => {
                        self.preview_state.seek_to_end();
                    }
                    KeyCode::Char('[') => {
                        self.preview_state.cycle_speed(false);
                    }
                    KeyCode::Char(']') => {
                        self.preview_state.cycle_speed(true);
                    }

                    // Console Scrolling
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.console_state.scroll_up(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.console_state.scroll_down(1);
                    }
                    KeyCode::PageUp => {
                        self.console_state.scroll_up(10);
                    }
                    KeyCode::PageDown => {
                        self.console_state.scroll_down(10);
                    }
                    _ => {}
                }
            }
            FocusPane::Console => {
                match key.code {
                    KeyCode::Tab | KeyCode::F(6) => {
                        self.focus = FocusPane::Editor;
                    }
                    KeyCode::BackTab => {
                        self.focus = FocusPane::Preview;
                    }
                    KeyCode::Esc => {
                        self.console_state.input_buffer.clear();
                        self.console_state.cursor_idx = 0;
                    }
                    _ => {
                        let action = self.console_state.handle_key(
                            key,
                            &self.config.file_path,
                            self.config.scene_name.as_deref(),
                            self.config.quality,
                            self.config.fps,
                        );
                        match action {
                            ConsoleAction::None => {}
                            ConsoleAction::OpenSettings => {
                                self.is_settings_open = true;
                                clear_kitty_images();
                            }
                            ConsoleAction::TriggerRebuild { scene } => {
                                if let Some(s) = scene {
                                    self.config.scene_name = Some(s);
                                }
                                self.trigger_rebuild();
                            }
                            ConsoleAction::SetScene(scene) => {
                                self.config.scene_name = Some(scene);
                            }
                            ConsoleAction::SetQuality(q) => {
                                self.config.quality = q;
                                self.settings_state.quality = q;
                                self.save_settings();
                            }
                            ConsoleAction::SetFps(fps) => {
                                self.config.fps = fps;
                                self.settings_state.fps = fps;
                                self.preview_state.fps = fps;
                                self.save_settings();
                            }
                            ConsoleAction::SetQualityAndFps(q, fps) => {
                                self.config.quality = q;
                                self.settings_state.quality = q;
                                self.config.fps = fps;
                                self.settings_state.fps = fps;
                                self.preview_state.fps = fps;
                                self.save_settings();
                            }
                            ConsoleAction::TogglePlay => {
                                self.preview_state.toggle_play();
                            }
                            ConsoleAction::Quit => {
                                self.should_quit = true;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if matches!(mouse.kind, MouseEventKind::Moved) {
            return Ok(());
        }

        // If Settings or Scene Palette overlay is open, mouse clicks inside modal are ignored
        if self.is_settings_open || self.scene_palette.is_open {
            return Ok(());
        }

        let col = mouse.column;
        let row = mouse.row;

        // ── Separator drag handling ──────────────────────────────────────────
        // We track which separator the user grabbed on MouseDown and update splits on Drag.

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Detect if the click landed on the vertical separator (between editor and right column)
                // or on the horizontal separator (between preview and console, in the right column).
                let on_h_sep = (col as i32 - self.last_preview_area.x as i32).abs() <= 1
                    || (col as i32 - self.last_editor_area.right() as i32).abs() <= 1;

                let in_right_col = col >= self.last_preview_area.x.saturating_sub(1);
                let on_v_sep = in_right_col
                    && ((row as i32 - self.last_console_area.y as i32).abs() <= 1
                        || (row as i32 - self.last_preview_area.bottom() as i32).abs() <= 1);

                if on_h_sep {
                    self.drag_target = Some(DragTarget::HSep);
                    return Ok(());
                } else if on_v_sep {
                    self.drag_target = Some(DragTarget::VSep);
                    return Ok(());
                } else {
                    self.drag_target = None;
                }
            }

            MouseEventKind::Up(_) => {
                if self.drag_target.take().is_some() {
                    clear_kitty_images();
                    self.preview_state.force_redraw = true;
                    return Ok(());
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(target) = self.drag_target {
                    let term_w = self.last_editor_area.width + self.last_preview_area.width;
                    let right_h = self.last_preview_area.height + self.last_console_area.height;

                    match target {
                        DragTarget::HSep => {
                            if term_w > 0 {
                                let new_split = (col as f64) / (term_w as f64);
                                let clamped = new_split.clamp(H_SPLIT_MIN, H_SPLIT_MAX);
                                if (clamped - self.h_split).abs() >= 0.005 {
                                    self.h_split = clamped;
                                    self.preview_state.force_redraw = true;
                                }
                            }
                        }
                        DragTarget::VSep => {
                            if right_h > 0 {
                                let row_rel = row.saturating_sub(self.last_preview_area.y);
                                let new_split = (row_rel as f64) / (right_h as f64);
                                let clamped = new_split.clamp(V_SPLIT_MIN, V_SPLIT_MAX);
                                if (clamped - self.v_split).abs() >= 0.005 {
                                    self.v_split = clamped;
                                    self.preview_state.force_redraw = true;
                                }
                            }
                        }
                    }
                    return Ok(());
                }
            }
            _ => {}
        }

        // ── Normal pane interaction ──────────────────────────────────────────

        let in_editor_inner = col >= self.last_editor_inner.x
            && col < self.last_editor_inner.right()
            && row >= self.last_editor_inner.y
            && row < self.last_editor_inner.bottom();

        let in_editor_area = col >= self.last_editor_area.x
            && col < self.last_editor_area.right()
            && row >= self.last_editor_area.y
            && row < self.last_editor_area.bottom();

        let in_preview_area = col >= self.last_preview_area.x
            && col < self.last_preview_area.right()
            && row >= self.last_preview_area.y
            && row < self.last_preview_area.bottom();

        let in_console_area = col >= self.last_console_area.x
            && col < self.last_console_area.right()
            && row >= self.last_console_area.y
            && row < self.last_console_area.bottom();

        let is_drag_or_release = matches!(
            mouse.kind,
            MouseEventKind::Drag(_) | MouseEventKind::Up(_)
        );

        let is_click = matches!(
            mouse.kind,
            MouseEventKind::Down(_)
        );

        if in_editor_inner || (self.focus == FocusPane::Editor && is_drag_or_release) {
            if is_click && self.focus != FocusPane::Editor {
                self.focus = FocusPane::Editor;
            }
            if let Some(ref mut pty) = self.pty {
                let _ = pty.handle_mouse_event(
                    mouse,
                    self.last_editor_inner.x,
                    self.last_editor_inner.y,
                );
            }
        } else if in_editor_area && is_click {
            self.focus = FocusPane::Editor;
        } else if in_preview_area || self.is_preview_fullscreen {
            if is_click {
                self.focus = FocusPane::Preview;
            }

            // 1. Check click on Fullscreen button
            if let Some(btn_rect) = self.preview_state.last_fullscreen_btn_area {
                if col >= btn_rect.x
                    && col < btn_rect.right()
                    && row >= btn_rect.y
                    && row < btn_rect.bottom()
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                {
                    self.is_preview_fullscreen = !self.is_preview_fullscreen;
                    clear_kitty_images();
                    self.preview_state.force_redraw = true;
                    return Ok(());
                }
            }

            // 2. Check click or drag on interactive Timeline Scrubber
            if let Some(hud) = self.preview_state.last_hud_area {
                let scrub_row = hud.bottom().saturating_sub(1);
                if (row == scrub_row || row == hud.y)
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left))
                {
                    if let Some((track_x, track_w)) = self.preview_state.last_scrub_track {
                        if col >= track_x && col < track_x + track_w && track_w > 0 {
                            let ratio = (col.saturating_sub(track_x) as f64) / (track_w as f64);
                            self.preview_state.seek_ratio(ratio);
                            return Ok(());
                        } else if col < track_x && col >= hud.x {
                            self.preview_state.seek_ratio(0.0);
                            return Ok(());
                        } else if col >= track_x + track_w && col < hud.right().saturating_sub(18) {
                            self.preview_state.seek_ratio(1.0);
                            return Ok(());
                        }
                    }
                }
            }

            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.preview_state.toggle_play();
                }
                MouseEventKind::ScrollUp => {
                    self.preview_state.step_prev();
                }
                MouseEventKind::ScrollDown => {
                    self.preview_state.step_next();
                }
                _ => {}
            }
        } else if in_console_area {
            if is_click {
                self.focus = FocusPane::Console;
            }
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.console_state.scroll_up(3);
                }
                MouseEventKind::ScrollDown => {
                    self.console_state.scroll_down(3);
                }
                _ => {}
            }
        }

        Ok(())
    }
}

pub fn render_scene_palette_overlay(
    palette_state: &ScenePaletteState,
    active_scene: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
    palette: &ThemePalette,
) {
    let modal_w = 60.min(area.width.saturating_sub(4));
    let modal_h = 16.min(area.height.saturating_sub(4));

    let offset_x = (area.width.saturating_sub(modal_w)) / 2;
    let offset_y = (area.height.saturating_sub(modal_h)) / 2;
    let modal_area = Rect::new(area.x + offset_x, area.y + offset_y, modal_w, modal_h);

    Clear.render(modal_area, buf);

    let border_type = if palette.is_high_contrast() {
        BorderType::Plain
    } else {
        BorderType::Rounded
    };

    let block = Block::default()
        .title(" ❪ Scene Palette (F8 / Ctrl+P) ❫ ")
        .title_alignment(Alignment::Center)
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(border_type)
        .border_style(
            Style::default()
                .fg(palette.border_focused)
                .add_modifier(Modifier::BOLD),
        );

    let inner_area = block.inner(modal_area);
    block.render(modal_area, buf);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search input box
            Constraint::Min(4),    // Filtered scenes list
            Constraint::Length(1), // Footer hotkey hints
        ])
        .split(inner_area);

    // 1. Search input block
    let search_block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(palette.accent_sapphire));

    let search_inner = search_block.inner(chunks[0]);
    search_block.render(chunks[0], buf);

    let query_display = format!("> {}█", palette_state.search_query);
    let search_p = Paragraph::new(Line::from(vec![
        Span::styled(
            query_display,
            Style::default()
                .fg(palette.text_primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    search_p.render(search_inner, buf);

    // 2. Filtered scenes list
    let filtered = palette_state.filtered_scenes();
    let list_area = chunks[1];

    if filtered.is_empty() {
        let empty_msg = Paragraph::new(Line::from(Span::styled(
            "  No matching scenes found",
            Style::default().fg(palette.text_dimmed),
        )));
        empty_msg.render(list_area, buf);
    } else {
        let max_items = list_area.height as usize;
        let scroll_offset = if palette_state.selected_idx >= max_items {
            palette_state.selected_idx - max_items + 1
        } else {
            0
        };

        let mut lines = Vec::new();
        for (idx, &scene_name) in filtered.iter().enumerate().skip(scroll_offset).take(max_items) {
            let is_selected = idx == palette_state.selected_idx;
            let is_active = active_scene == Some(scene_name.as_str());

            let mut spans = Vec::new();
            let prefix = if is_selected { " ▶ " } else { "   " };
            let marker = if is_active { " * " } else { "   " };

            let name_style = if is_selected {
                Style::default()
                    .fg(palette.accent_blue)
                    .bg(palette.bg_surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text_primary)
            };

            spans.push(Span::styled(
                prefix,
                if is_selected {
                    Style::default()
                        .fg(palette.accent_blue)
                        .bg(palette.bg_surface)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.text_dimmed)
                },
            ));
            spans.push(Span::styled(
                marker,
                if is_active {
                    Style::default()
                        .fg(palette.success_sage)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.text_dimmed)
                },
            ));
            spans.push(Span::styled(scene_name, name_style));

            if is_active {
                spans.push(Span::styled(
                    " (Active)",
                    Style::default().fg(palette.success_sage),
                ));
            }

            lines.push(Line::from(spans));
        }

        let list_p = Paragraph::new(lines);
        list_p.render(list_area, buf);
    }

    // 3. Footer hints
    let footer_line = Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
        Span::styled(" Select  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("Enter", Style::default().fg(palette.accent_sapphire).add_modifier(Modifier::BOLD)),
        Span::styled(" Switch  •  ", Style::default().fg(palette.text_dimmed)),
        Span::styled("Esc", Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(palette.text_dimmed)),
    ]);
    let footer_p = Paragraph::new(footer_line).alignment(Alignment::Center);
    footer_p.render(chunks[2], buf);
}
