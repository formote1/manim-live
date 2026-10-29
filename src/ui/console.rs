use crate::runner::Quality;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Widget},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
    time::{Duration, Instant},
};

static PY_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"File\s+["']([^"']+)["'],\s+line\s+(\d+)(?:,\s+in\s+(.*))?"#).unwrap()
});
static LATEX_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"!\s+LaTeX Error:\s+(.*)"#).unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticError {
    pub file_path: PathBuf,
    pub line_number: u32,
    pub error_message: String,
    pub is_latex: bool,
}

#[allow(dead_code)]
pub const ALLOWED_FPS: [u32; 4] = [15, 24, 30, 60];


pub fn parse_fps(val: &str) -> Result<u32, String> {
    match val.parse::<u32>() {
        Ok(n) if n > 0 && n <= 120 => Ok(n),
        _ => Err(format!(
            "✖ Invalid FPS '{}'. Allowed values are positive integers (e.g. 15, 24, 30, 60).",
            val
        )),
    }
}

pub fn parse_quality(val: &str) -> Option<Quality> {
    match val.to_lowercase().as_str() {
        "low" | "l" | "480" | "480p" => Some(Quality::Low),
        "medium" | "med" | "m" | "720" | "720p" => Some(Quality::Medium),
        "high" | "h" | "1080" | "1080p" => Some(Quality::High),
        "4k" | "fourk" | "k" | "2160" | "2160p" => Some(Quality::FourK),
        _ => None,
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleAction {
    None,
    TriggerRebuild { scene: Option<String> },
    SetScene(String),
    SetQuality(Quality),
    SetFps(u32),
    SetQualityAndFps(Quality, u32),
    TogglePlay,
    OpenSettings,
    Quit,
}

#[derive(Debug, Clone)]
pub enum BuildState {
    Idle,
    Building {
        start_time: Instant,
        progress: f32,
        current_step: String,
    },
    Success {
        duration: Duration,
    },
    Failed {
        duration: Duration,
        error_summary: Option<String>,
    },
}

pub struct ConsoleState {
    pub build_state: BuildState,
    pub lines: Vec<String>,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub active_error: Option<DiagnosticError>,

    // Interactive Terminal State
    pub input_buffer: String,
    pub cursor_idx: usize,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub saved_input: String,
    pub cursor_screen_pos: Option<(u16, u16)>,
}

impl ConsoleState {
    pub fn new() -> Self {
        let mut state = Self {
            build_state: BuildState::Idle,
            lines: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            active_error: None,
            input_buffer: String::new(),
            cursor_idx: 0,
            history: Vec::new(),
            history_idx: None,
            saved_input: String::new(),
            cursor_screen_pos: None,
        };

        state.add_line("─── manim-live interactive studio ───".to_string());
        state.add_line("Terminal active. Type 'help' for commands, 'render' to compile.".to_string());
        state.add_line("Press Tab to cycle between Editor, Preview, and Console.".to_string());
        state
    }

    pub fn add_line(&mut self, line: String) {
        if let Some(caps) = PY_RE.captures(&line) {
            let file = PathBuf::from(&caps[1]);
            let line_no = caps[2].parse::<u32>().unwrap_or(1);
            let msg = caps
                .get(3)
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "Error in construct".to_string());
            self.active_error = Some(DiagnosticError {
                file_path: file,
                line_number: line_no,
                error_message: msg,
                is_latex: false,
            });
        } else if let Some(caps) = LATEX_RE.captures(&line) {
            if let Some(ref mut err) = self.active_error {
                err.error_message = format!("LaTeX: {}", &caps[1]);
                err.is_latex = true;
            } else {
                self.active_error = Some(DiagnosticError {
                    file_path: PathBuf::new(),
                    line_number: 1,
                    error_message: format!("LaTeX: {}", &caps[1]),
                    is_latex: true,
                });
            }
        }

        self.lines.push(line);
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    pub fn set_progress(&mut self, pct: f32, message: String) {
        if let BuildState::Building {
            ref mut progress,
            ref mut current_step,
            ..
        } = self.build_state
        {
            *progress = progress.max(pct.clamp(0.0, 1.0));
            *current_step = message;
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.lines.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
        if self.scroll_offset == max_scroll {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.lines.len().saturating_sub(1);
        self.auto_scroll = true;
    }

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        file_path: &Path,
        current_scene: Option<&str>,
        current_quality: Quality,
        current_fps: u32,
    ) -> ConsoleAction {
        match key.code {
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_idx = 0;
                ConsoleAction::None
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_idx = self.input_buffer.len();
                ConsoleAction::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_buffer.clear();
                self.cursor_idx = 0;
                ConsoleAction::None
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_buffer.truncate(self.cursor_idx);
                ConsoleAction::None
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.cursor_idx > 0 {
                    let before = &self.input_buffer[..self.cursor_idx];
                    let trimmed = before.trim_end();
                    let new_idx = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    let after = self.input_buffer[self.cursor_idx..].to_string();
                    let mut result = before[..new_idx].to_string();
                    result.push_str(&after);
                    self.input_buffer = result;
                    self.cursor_idx = new_idx;
                }
                ConsoleAction::None
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.lines.clear();
                self.scroll_offset = 0;
                ConsoleAction::None
            }
            KeyCode::Char('?') if self.input_buffer.is_empty() => {
                ConsoleAction::OpenSettings
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    self.input_buffer.insert(self.cursor_idx, c);
                    self.cursor_idx += 1;
                }
                ConsoleAction::None
            }
            KeyCode::Backspace => {
                if self.cursor_idx > 0 {
                    self.input_buffer.remove(self.cursor_idx - 1);
                    self.cursor_idx -= 1;
                }
                ConsoleAction::None
            }
            KeyCode::Delete => {
                if self.cursor_idx < self.input_buffer.len() {
                    self.input_buffer.remove(self.cursor_idx);
                }
                ConsoleAction::None
            }
            KeyCode::Left => {
                self.cursor_idx = self.cursor_idx.saturating_sub(1);
                ConsoleAction::None
            }
            KeyCode::Right => {
                if self.cursor_idx < self.input_buffer.len() {
                    self.cursor_idx += 1;
                }
                ConsoleAction::None
            }
            KeyCode::Home => {
                self.cursor_idx = 0;
                ConsoleAction::None
            }
            KeyCode::End => {
                self.cursor_idx = self.input_buffer.len();
                ConsoleAction::None
            }
            KeyCode::Up => {
                if !self.history.is_empty() {
                    if self.history_idx.is_none() {
                        self.saved_input = self.input_buffer.clone();
                        let new_idx = self.history.len() - 1;
                        self.history_idx = Some(new_idx);
                        self.input_buffer = self.history[new_idx].clone();
                        self.cursor_idx = self.input_buffer.len();
                    } else if let Some(idx) = self.history_idx {
                        if idx > 0 {
                            let new_idx = idx - 1;
                            self.history_idx = Some(new_idx);
                            self.input_buffer = self.history[new_idx].clone();
                            self.cursor_idx = self.input_buffer.len();
                        }
                    }
                }
                ConsoleAction::None
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.history.len() {
                        let new_idx = idx + 1;
                        self.history_idx = Some(new_idx);
                        self.input_buffer = self.history[new_idx].clone();
                        self.cursor_idx = self.input_buffer.len();
                    } else {
                        self.history_idx = None;
                        self.input_buffer = self.saved_input.clone();
                        self.cursor_idx = self.input_buffer.len();
                    }
                }
                ConsoleAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(5);
                ConsoleAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(5);
                ConsoleAction::None
            }
            KeyCode::Enter => {
                let raw = std::mem::take(&mut self.input_buffer);
                self.cursor_idx = 0;
                self.history_idx = None;
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return ConsoleAction::None;
                }
                self.add_line(format!("> {}", trimmed));
                self.history.push(trimmed.to_string());
                self.execute_command(
                    trimmed,
                    file_path,
                    current_scene,
                    current_quality,
                    current_fps,
                )
            }
            _ => ConsoleAction::None,
        }
    }

    fn execute_command(
        &mut self,
        cmd_line: &str,
        file_path: &Path,
        current_scene: Option<&str>,
        current_quality: Quality,
        current_fps: u32,
    ) -> ConsoleAction {
        let parts: Vec<&str> = cmd_line.split_whitespace().collect();
        if parts.is_empty() {
            return ConsoleAction::None;
        }

        let command = parts[0].to_lowercase();
        let args = &parts[1..];

        // Check if command is a direct quality shortcut (e.g. 'low 30', '4k 60', 'medium', 'high 120')
        if let Some(q) = parse_quality(&command) {
            if args.is_empty() {
                self.add_line(format!("Quality updated to {:?}", q));
                self.add_line("Type 'render' or press 'r' to recompile.".to_string());
                return ConsoleAction::SetQuality(q);
            } else {
                match parse_fps(args[0]) {
                    Ok(fps) => {
                        self.add_line(format!("Quality updated to {:?} at {} FPS", q, fps));
                        self.add_line("Type 'render' or press 'r' to recompile.".to_string());
                        return ConsoleAction::SetQualityAndFps(q, fps);
                    }
                    Err(e) => {
                        self.add_line(e);
                        return ConsoleAction::None;
                    }
                }
            }
        }

        match command.as_str() {
            "help" => {
                self.add_line("Commands:".to_string());
                self.add_line("  render [scene]   Trigger compile & preview (e.g. 'render', 'render MyScene')".to_string());
                self.add_line("  scene <name>     Set active scene class to render".to_string());
                self.add_line("  scenes / list    List all Scene classes discovered in python file".to_string());
                self.add_line("  quality <q> [fps] Set render quality & optional fps (e.g. 'quality high 60')".to_string());
                self.add_line("  fps <number>     Set render/playback FPS (e.g. 'fps 30', 'fps 60')".to_string());
                self.add_line("  settings / ?     Open interactive settings app modal".to_string());
                self.add_line("  play / pause     Toggle video playback".to_string());
                self.add_line("  status / info    Show current scene, quality, fps & build status".to_string());
                self.add_line("  exec <bin> [args] Direct process execution (e.g. 'exec ls -la', 'exec python --version')".to_string());
                self.add_line("  clear / cls      Clear console logs".to_string());
                self.add_line("  quit / exit      Exit manim-live".to_string());
                self.add_line("Navigation: Tab cycles focus across panes | Up/Down cycles history".to_string());
                ConsoleAction::None
            }
            "settings" | "?" => {
                ConsoleAction::OpenSettings
            }
            "render" | "build" | "r" => {
                let scene = if !args.is_empty() {
                    Some(args[0].to_string())
                } else {
                    None
                };
                if let Some(ref s) = scene {
                    self.add_line(format!("Triggering build for scene: {}", s));
                } else {
                    self.add_line("Triggering build for current scene...".to_string());
                }
                ConsoleAction::TriggerRebuild { scene }
            }
            "scene" => {
                if args.is_empty() {
                    let active = current_scene.unwrap_or("Auto-detect");
                    self.add_line(format!("Active scene: {}", active));
                    self.add_line("To switch scene, type: scene <SceneClassName>".to_string());
                    ConsoleAction::None
                } else {
                    let new_scene = args[0].to_string();
                    self.add_line(format!("Active scene set to: {}", new_scene));
                    self.add_line("Type 'render' or press 'r' to compile with new scene.".to_string());
                    ConsoleAction::SetScene(new_scene)
                }
            }
            "scenes" | "list" => {
                let scenes = detect_all_scenes(file_path);
                if scenes.is_empty() {
                    self.add_line(format!("No Scene classes found in {}", file_path.display()));
                } else {
                    let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
                    self.add_line(format!("Found {} scene(s) in {}:", scenes.len(), file_name));
                    for sc in &scenes {
                        if current_scene == Some(sc.as_str()) {
                            self.add_line(format!("  * {} (active)", sc));
                        } else {
                            self.add_line(format!("  - {}", sc));
                        }
                    }
                    self.add_line("Type 'render <scene>' or 'scene <scene>' to switch.".to_string());
                }
                ConsoleAction::None
            }
            "quality" | "q" => {
                if args.is_empty() {
                    self.add_line(format!("Current quality: {:?} at {} FPS", current_quality, current_fps));
                    self.add_line("Usage: quality <low|medium|high|4k> [fps]".to_string());
                    ConsoleAction::None
                } else {
                    let q = parse_quality(args[0]);
                    if let Some(new_q) = q {
                        if args.len() > 1 {
                            match parse_fps(args[1]) {
                                Ok(fps) => {
                                    self.add_line(format!("Quality updated to {:?} at {} FPS", new_q, fps));
                                    self.add_line("Type 'render' or press 'r' to recompile.".to_string());
                                    ConsoleAction::SetQualityAndFps(new_q, fps)
                                }
                                Err(e) => {
                                    self.add_line(e);
                                    ConsoleAction::None
                                }
                            }
                        } else {
                            self.add_line(format!("Quality updated to {:?}", new_q));
                            self.add_line("Type 'render' or press 'r' to recompile.".to_string());
                            ConsoleAction::SetQuality(new_q)
                        }
                    } else {
                        self.add_line(format!("Invalid quality '{}'. Choose: low, medium, high, 4k", args[0]));
                        ConsoleAction::None
                    }
                }
            }
            "fps" => {
                if args.is_empty() {
                    self.add_line(format!("Current preview/render FPS: {}", current_fps));
                    self.add_line("Usage: fps <15|24|30|60>".to_string());
                    ConsoleAction::None
                } else {
                    match parse_fps(args[0]) {
                        Ok(fps) => {
                            self.add_line(format!("FPS set to {}", fps));
                            self.add_line("Type 'render' or press 'r' to recompile.".to_string());
                            ConsoleAction::SetFps(fps)
                        }
                        Err(e) => {
                            self.add_line(e);
                            ConsoleAction::None
                        }
                    }
                }
            }

            "play" | "pause" | "toggle" => {
                ConsoleAction::TogglePlay
            }
            "status" | "info" => {
                self.add_line("─── Session Status ───".to_string());
                self.add_line(format!("  File:    {}", file_path.display()));
                self.add_line(format!("  Scene:   {}", current_scene.unwrap_or("Auto-detect")));
                self.add_line(format!("  Quality: {:?}", current_quality));
                self.add_line(format!("  FPS:     {}", current_fps));
                let status_str = match &self.build_state {
                    BuildState::Idle => "Idle (ready)".to_string(),
                    BuildState::Building { progress, current_step, .. } => {
                        format!("Building: {:.0}% ({})", progress * 100.0, current_step)
                    }
                    BuildState::Success { duration } => {
                        format!("Success ({:.2}s)", duration.as_secs_f32())
                    }
                    BuildState::Failed { duration, error_summary } => {
                        format!("Failed ({:.2}s): {}", duration.as_secs_f32(), error_summary.as_deref().unwrap_or("unknown"))
                    }
                };
                self.add_line(format!("  State:   {}", status_str));
                ConsoleAction::None
            }
            "exec" | "run" => {
                if args.is_empty() {
                    self.add_line("Usage: exec <binary> [args...] (e.g. 'exec ls -la', 'exec python --version')".to_string());
                } else {
                    let binary = args[0];
                    let binary_args = &args[1..];
                    match Command::new(binary).args(binary_args).output() {
                        Ok(output) => {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            for line in stdout.lines() {
                                self.add_line(line.to_string());
                            }
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            for line in stderr.lines() {
                                self.add_line(format!("[stderr] {}", line));
                            }
                            if !output.status.success() {
                                if let Some(code) = output.status.code() {
                                    self.add_line(format!("[exited with status code {}]", code));
                                }
                            }
                        }
                        Err(e) => {
                            self.add_line(format!("Execution failed: {}", e));
                        }
                    }
                }
                ConsoleAction::None
            }
            "clear" | "cls" => {
                self.lines.clear();
                self.scroll_offset = 0;
                ConsoleAction::None
            }
            "quit" | "exit" | "q!" => {
                ConsoleAction::Quit
            }
            _ => {
                self.add_line(format!("Unknown command: '{}'. Type 'help' for available commands.", command));
                ConsoleAction::None
            }
        }
    }

    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        is_focused: bool,
        border_type: BorderType,
        palette: &theme::ThemePalette,
    ) {
        let border_color = if is_focused {
            palette.accent_mauve
        } else {
            palette.border_unfocused
        };

        // Determine title & status indicator
        let (status_title, status_style) = match &self.build_state {
            BuildState::Idle => {
                if is_focused {
                    (
                        " █ Console & Terminal [ACTIVE] ".to_string(),
                        Style::default().fg(border_color).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        " ❪ Console & Terminal [F3 to focus] ❫ ".to_string(),
                        Style::default().fg(border_color),
                    )
                }
            }
            BuildState::Building { start_time, progress, .. } => {
                let elapsed = start_time.elapsed().as_secs_f32();
                if is_focused {
                    (
                        format!(" █ Console & Terminal [⚙ {:.1}s - {:.0}%] [ACTIVE] ", elapsed, progress * 100.0),
                        Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        format!(" ❪ Console & Terminal [⚙ {:.1}s - {:.0}%] ❫ ", elapsed, progress * 100.0),
                        Style::default().fg(palette.warn_peach).add_modifier(Modifier::BOLD),
                    )
                }
            }
            BuildState::Success { duration } => {
                if is_focused {
                    (
                        format!(" █ Console & Terminal [✔ SUCCESS {:.2}s] [ACTIVE] ", duration.as_secs_f32()),
                        Style::default().fg(palette.success_sage).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        format!(" ❪ Console & Terminal [✔ SUCCESS {:.2}s] ❫ ", duration.as_secs_f32()),
                        Style::default().fg(palette.success_sage).add_modifier(Modifier::BOLD),
                    )
                }
            }
            BuildState::Failed { duration, error_summary } => {
                let suffix = error_summary.as_deref().unwrap_or("errors");
                if is_focused {
                    (
                        format!(" █ Console & Terminal [✖ FAILED {:.2}s ({})] [ACTIVE] ", duration.as_secs_f32(), suffix),
                        Style::default().fg(palette.error_coral).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        format!(" ❪ Console & Terminal [✖ FAILED {:.2}s ({})] ❫ ", duration.as_secs_f32(), suffix),
                        Style::default().fg(palette.error_coral).add_modifier(Modifier::BOLD),
                    )
                }
            }
        };


        let block = Block::default()
            .title(status_title)
            .title_alignment(Alignment::Left)
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(status_style);

        let inner_area = block.inner(area);
        block.render(area, buf);

        if inner_area.height == 0 || inner_area.width == 0 {
            self.cursor_screen_pos = None;
            return;
        }

        // Layout inner area:
        // Top: Optional progress gauge (1 line if building)
        // Middle: Output log lines (flexible)
        // Bottom: Command input prompt (1 line)
        let is_building = matches!(self.build_state, BuildState::Building { .. });
        let constraints = if is_building {
            vec![
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Min(1),
                Constraint::Length(1),
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner_area);

        let (gauge_area, logs_area, prompt_area) = if is_building {
            (Some(chunks[0]), chunks[1], chunks[2])
        } else {
            (None, chunks[0], chunks[1])
        };

        // Render progress gauge if building
        if let (Some(g_area), BuildState::Building { progress, .. }) = (gauge_area, &self.build_state) {
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(palette.accent_blue).bg(palette.bg_surface))
                .ratio(*progress as f64)
                .label(format!("{:.0}%", progress * 100.0));
            gauge.render(g_area, buf);
        }

        // Format and render log lines
        let visible_height = logs_area.height as usize;
        let total_lines = self.lines.len();

        let start_line = if self.auto_scroll {
            total_lines.saturating_sub(visible_height)
        } else {
            self.scroll_offset.min(total_lines.saturating_sub(visible_height))
        };

        let rendered_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(start_line)
            .take(visible_height)
            .map(|raw| format_log_line(raw, palette))
            .collect();

        let paragraph = Paragraph::new(rendered_lines);
        paragraph.render(logs_area, buf);

        // Render interactive terminal prompt line with crisp in-buffer visual cursor
        let prompt_prefix = Span::styled(
            "❯ ",
            Style::default()
                .fg(if is_focused { palette.border_focused } else { palette.border_unfocused })
                .add_modifier(Modifier::BOLD),
        );
        let mut prompt_spans = vec![prompt_prefix];

        if is_focused {
            let before = &self.input_buffer[..self.cursor_idx];
            if !before.is_empty() {
                prompt_spans.push(Span::styled(before, Style::default().fg(palette.text_primary)));
            }
            if self.cursor_idx < self.input_buffer.len() {
                let c = &self.input_buffer[self.cursor_idx..self.cursor_idx + 1];
                prompt_spans.push(Span::styled(
                    c,
                    Style::default().fg(palette.bg_crust).bg(palette.border_focused),
                ));
                let after = &self.input_buffer[self.cursor_idx + 1..];
                if !after.is_empty() {
                    prompt_spans.push(Span::styled(after, Style::default().fg(palette.text_primary)));
                }
            } else {
                prompt_spans.push(Span::styled(
                    "█",
                    Style::default().fg(palette.border_focused),
                ));
            }
        } else {
            prompt_spans.push(Span::styled(
                &self.input_buffer,
                Style::default().fg(palette.text_muted),
            ));
        }

        let prompt_p = Paragraph::new(Line::from(prompt_spans));
        prompt_p.render(prompt_area, buf);

        // Note: We intentionally leave cursor_screen_pos as None so Ratatui
        // never moves the physical terminal cursor to row 52. This completely
        // prevents Kitty's cursor_trail shader from detecting jumps between row 2 and 52!
        self.cursor_screen_pos = None;
    }
}

pub fn detect_all_scenes(file_path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let re = match regex::Regex::new(r"class\s+([A-Za-z0-9_]+)\s*\([^)]*Scene[^)]*\):") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    re.captures_iter(&content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

fn format_log_line(raw: &str, palette: &theme::ThemePalette) -> Line<'static> {
    let lower = raw.to_lowercase();
    let style = if lower.contains("error") || lower.contains("traceback") || lower.contains("exception") || raw.starts_with("✖") {
        Style::default().fg(palette.error_coral).add_modifier(Modifier::BOLD)
    } else if lower.contains("warning") {
        Style::default().fg(palette.warn_peach)
    } else if lower.contains("animation") || lower.contains("rendering") || lower.contains("writing") {
        Style::default().fg(palette.accent_sapphire)
    } else if lower.contains("file ready at") || lower.contains("output file") || lower.contains("success") || lower.starts_with("✔") {
        Style::default().fg(palette.success_sage).add_modifier(Modifier::BOLD)
    } else if raw.starts_with('>') || raw.starts_with('$') || raw.starts_with('❯') {
        Style::default().fg(palette.accent_mauve).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.text_muted)
    };

    Line::from(Span::styled(raw.to_string(), style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_console_text_input_and_cursor() {
        let mut console = ConsoleState::new();
        let path = PathBuf::from("dummy.py");

        let key_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty());
        let key_b = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::empty());
        let key_left = KeyEvent::new(KeyCode::Left, KeyModifiers::empty());
        let key_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty());

        console.handle_key(key_a, &path, None, Quality::Low, 30);
        console.handle_key(key_b, &path, None, Quality::Low, 30);
        assert_eq!(console.input_buffer, "ab");
        assert_eq!(console.cursor_idx, 2);

        console.handle_key(key_left, &path, None, Quality::Low, 30);
        assert_eq!(console.cursor_idx, 1);

        console.handle_key(key_x, &path, None, Quality::Low, 30);
        assert_eq!(console.input_buffer, "axb");
        assert_eq!(console.cursor_idx, 2);

        let key_bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty());
        console.handle_key(key_bs, &path, None, Quality::Low, 30);
        assert_eq!(console.input_buffer, "ab");
        assert_eq!(console.cursor_idx, 1);
    }

    #[test]
    fn test_console_commands_and_actions() {
        let mut console = ConsoleState::new();
        let path = PathBuf::from("/home/chuck/Animaitons/manimaitons/scene.py");

        // Test render command
        console.input_buffer = "render PureManimSpirograph".to_string();
        console.cursor_idx = console.input_buffer.len();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(
            action,
            ConsoleAction::TriggerRebuild {
                scene: Some("PureManimSpirograph".to_string())
            }
        );

        // Test quality and fps combined command: 'quality low 30'
        console.input_buffer = "quality low 30".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetQualityAndFps(Quality::Low, 30));

        // Test direct shortcut '4k 60'
        console.input_buffer = "4k 60".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetQualityAndFps(Quality::FourK, 60));

        // Test direct shortcut 'medium 45'
        console.input_buffer = "medium 45".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetQualityAndFps(Quality::Medium, 45));

        // Test direct shortcut 'high 120'
        console.input_buffer = "high 120".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetQualityAndFps(Quality::High, 120));

        // Test valid fps (24)
        console.input_buffer = "fps 24".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetFps(24));

        // Test fps 60
        console.input_buffer = "fps 60".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetFps(60));

        // Test scene command
        console.input_buffer = "scene Spirograph2".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::SetScene("Spirograph2".to_string()));

        // Test sandboxed direct execution command
        console.input_buffer = "exec echo 'live-test'".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::None);
        assert!(console.lines.iter().any(|l| l.contains("live-test")));

        // Test settings and ? commands
        console.input_buffer = "settings".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::OpenSettings);

        console.input_buffer = "?".to_string();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::OpenSettings);

        // Test typing '?' on empty input buffer immediately opens settings
        console.input_buffer.clear();
        let action = console.handle_key(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(action, ConsoleAction::OpenSettings);

        // Test scene discovery
        let scenes = detect_all_scenes(&path);
        assert!(scenes.contains(&"PureManimSpirograph".to_string()));
    }

    #[test]
    fn test_diagnostic_error_parsing() {
        let mut console = ConsoleState::new();
        assert!(console.active_error.is_none());

        console.add_line(r#"File "scene.py", line 42, in construct"#.to_string());
        assert!(console.active_error.is_some());
        let err = console.active_error.as_ref().unwrap();
        assert_eq!(err.file_path, PathBuf::from("scene.py"));
        assert_eq!(err.line_number, 42);
        assert_eq!(err.error_message, "construct");
        assert!(!err.is_latex);

        console.add_line(r#"! LaTeX Error: File `amsmath.sty' not found."#.to_string());
        let err2 = console.active_error.as_ref().unwrap();
        assert!(err2.is_latex);
        assert!(err2.error_message.contains("amsmath.sty"));
    }

    #[test]
    fn test_console_history_navigation() {
        let mut console = ConsoleState::new();
        let path = PathBuf::from("scene.py");

        console.input_buffer = "cmd1".to_string();
        console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );

        console.input_buffer = "cmd2".to_string();
        console.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );

        // Press Up -> cmd2
        console.handle_key(
            KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(console.input_buffer, "cmd2");

        // Press Up again -> cmd1
        console.handle_key(
            KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(console.input_buffer, "cmd1");

        // Press Down -> cmd2
        console.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(console.input_buffer, "cmd2");

        // Press Down -> empty (restored saved input)
        console.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            &path,
            None,
            Quality::Low,
            30,
        );
        assert_eq!(console.input_buffer, "");
    }
}
