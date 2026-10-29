mod app;
mod pty;
mod runner;
mod ui;
mod watcher;

use anyhow::{Context, Result};
use app::{App, AppConfig};
use clap::Parser;
use crossterm::{
    cursor::Show,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
    Terminal,
};
use runner::Quality;
use std::{
    fs,
    io::stdout,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(
    name = "manim-live",
    version,
    about = "Interactive TUI live-reloading studio for Python's Manim animation engine"
)]
pub struct Cli {
    /// Python file containing Manim scene(s)
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Optional Scene name to render
    #[arg(value_name = "SCENE")]
    pub scene: Option<String>,

    /// Editor command (defaults to micro, fallback to $EDITOR, then nano)
    #[arg(short, long)]
    pub editor: Option<String>,

    /// Manim quality profile (low, medium, high, 4k)
    #[arg(short, long, default_value = "low")]
    pub quality: String,

    /// Legacy format option (hidden; manim-live renders full animation video)
    #[arg(short, long, default_value = "mp4", hide = true)]
    pub format: String,

    /// Frame rate cap for live previews
    #[arg(long, default_value_t = 30)]
    pub fps: u32,

    /// Terminal image protocol (auto, kitty, halfblocks)
    #[arg(long, default_value = "auto")]
    pub protocol: String,

    /// Custom path or command for manim executable
    #[arg(long, value_name = "PATH")]
    pub manim: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyReport {
    pub python_path: Option<PathBuf>,
    pub python_version: Option<String>,
    pub manim_path: Option<PathBuf>,
    pub manim_version: Option<String>,
    pub ffmpeg_path: Option<PathBuf>,
    pub dvisvgm_path: Option<PathBuf>,
}

pub fn probe_environment(target_file: &Path, explicit_manim: Option<&str>) -> DependencyReport {
    let resolved_manim = runner::resolve_manim(explicit_manim, target_file);
    let manim_path = resolved_manim.map(|r| r.bin_path);
    let manim_version = manim_path
        .as_ref()
        .and_then(|p| get_cmd_version(p.to_str().unwrap_or("manim"), &["--version"]))
        .or_else(|| get_cmd_version("manim", &["--version"]));

    let python_path = which::which("python3").or_else(|_| which::which("python")).ok();
    let python_version = python_path
        .as_ref()
        .and_then(|p| get_cmd_version(p.to_str().unwrap_or("python3"), &["--version"]))
        .or_else(|| get_cmd_version("python3", &["--version"]));

    let ffmpeg_path = which::which("ffmpeg").ok();
    let dvisvgm_path = which::which("dvisvgm").ok();

    DependencyReport {
        python_path,
        python_version,
        manim_path,
        manim_version,
        ffmpeg_path,
        dvisvgm_path,
    }
}

fn get_cmd_version(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let out = String::from_utf8_lossy(&output.stdout);
                let first_line = out.lines().next().unwrap_or("").trim().to_string();
                if first_line.is_empty() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    Some(err.lines().next().unwrap_or("").trim().to_string())
                } else {
                    Some(first_line)
                }
            } else {
                None
            }
        })
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Check if target python file exists, or create starter template with sanitized scene name
    ensure_target_file(&cli.file, cli.scene.as_deref())?;

    // Auto-detect scene name from python file if not provided
    let scene_name = cli.scene.or_else(|| detect_first_scene_name(&cli.file));

    // Resolve editor command
    let editor_cmd = resolve_editor(cli.editor.as_deref());

    // Parse quality (capped to Medium / 720p MAX for live preview)
    let quality = match cli.quality.to_lowercase().as_str() {
        "m" | "medium" | "med" => Quality::Medium,
        "h" | "high" | "k" | "4k" | "fourk" => Quality::Medium, // 720p MAX
        _ => Quality::Low,
    };

    // Protocol override
    let protocol = match cli.protocol.to_lowercase().as_str() {
        "kitty" => Some("kitty".to_string()),
        "halfblocks" | "halfblock" => Some("halfblocks".to_string()),
        _ => None,
    };

    let config = AppConfig {
        file_path: cli.file,
        scene_name,
        editor_cmd,
        quality,
        fps: cli.fps.clamp(1, 30),
        protocol,
        manim_bin: cli.manim,
    };

    // Install panic hook to ensure terminal state is always restored
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to initialize alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal backend")?;

    // Pre-flight dependency check (Phase 4)
    let report = probe_environment(&config.file_path, config.manim_bin.as_deref());
    let can_proceed = if report.manim_path.is_none() || report.ffmpeg_path.is_none() {
        run_diagnostic_screen(&mut terminal, &config.file_path, config.manim_bin.as_deref())?
    } else {
        true
    };

    let run_result = if can_proceed {
        // Initialize & run application
        let mut app = App::new(config)?;
        app.run(&mut terminal)
    } else {
        Ok(())
    };

    // Teardown terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
    let _ = terminal.show_cursor();

    run_result
}

fn run_diagnostic_screen<B: Backend>(
    terminal: &mut Terminal<B>,
    target_file: &Path,
    explicit_manim: Option<&str>,
) -> Result<bool> {
    loop {
        let report = probe_environment(target_file, explicit_manim);
        if report.manim_path.is_some() && report.ffmpeg_path.is_some() {
            return Ok(true);
        }

        terminal
            .draw(|f| {
                render_diagnostic_screen(&report, f.area(), f.buffer_mut());
            })
            .map_err(|e| anyhow::anyhow!("Diagnostic screen render error: {e}"))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Enter => {
                            // Re-probe on next iteration
                            continue;
                        }
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                            return Ok(false);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(false);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn render_diagnostic_screen(report: &DependencyReport, area: Rect, buf: &mut Buffer) {
    let modal_w = 78.min(area.width.saturating_sub(2));
    let modal_h = 20.min(area.height.saturating_sub(2));

    let offset_x = (area.width.saturating_sub(modal_w)) / 2;
    let offset_y = (area.height.saturating_sub(modal_h)) / 2;
    let modal_area = Rect::new(area.x + offset_x, area.y + offset_y, modal_w, modal_h);

    Clear.render(modal_area, buf);

    let block = Block::default()
        .title(" System Readiness Check ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(Color::Rgb(243, 139, 168))
                .add_modifier(Modifier::BOLD),
        );

    let inner_area = block.inner(modal_area);
    block.render(modal_area, buf);

    let check_mark = Span::styled(
        " ✔ ",
        Style::default()
            .fg(Color::Rgb(166, 227, 161))
            .add_modifier(Modifier::BOLD),
    );
    let cross_mark = Span::styled(
        " ✖ ",
        Style::default()
            .fg(Color::Rgb(243, 139, 168))
            .add_modifier(Modifier::BOLD),
    );

    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    // 1. Python
    let mut py_spans = Vec::new();
    if let Some(ref path) = report.python_path {
        py_spans.push(check_mark.clone());
        py_spans.push(Span::styled(
            "Python:       ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        let ver = report.python_version.as_deref().unwrap_or("");
        py_spans.push(Span::styled(
            format!("{} ({})", path.display(), ver),
            Style::default().fg(Color::Rgb(205, 214, 244)),
        ));
    } else {
        py_spans.push(cross_mark.clone());
        py_spans.push(Span::styled(
            "Python:       ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        py_spans.push(Span::styled(
            "python3 executable not found in PATH",
            Style::default().fg(Color::Rgb(243, 139, 168)),
        ));
    }
    lines.push(Line::from(py_spans));

    // 2. Manim
    let mut manim_spans = Vec::new();
    if let Some(ref path) = report.manim_path {
        manim_spans.push(check_mark.clone());
        manim_spans.push(Span::styled(
            "Manim:        ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        let ver = report.manim_version.as_deref().unwrap_or("");
        manim_spans.push(Span::styled(
            format!("{} ({})", path.display(), ver),
            Style::default().fg(Color::Rgb(205, 214, 244)),
        ));
    } else {
        manim_spans.push(cross_mark.clone());
        manim_spans.push(Span::styled(
            "Manim:        ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        manim_spans.push(Span::styled(
            "Executable 'manim' not found in PATH or virtualenvs",
            Style::default().fg(Color::Rgb(243, 139, 168)),
        ));
    }
    lines.push(Line::from(manim_spans));

    // 3. FFmpeg
    let mut ffmpeg_spans = Vec::new();
    if let Some(ref path) = report.ffmpeg_path {
        ffmpeg_spans.push(check_mark.clone());
        ffmpeg_spans.push(Span::styled(
            "FFmpeg:       ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        ffmpeg_spans.push(Span::styled(
            path.display().to_string(),
            Style::default().fg(Color::Rgb(205, 214, 244)),
        ));
    } else {
        ffmpeg_spans.push(cross_mark.clone());
        ffmpeg_spans.push(Span::styled(
            "FFmpeg:       ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        ffmpeg_spans.push(Span::styled(
            "Executable 'ffmpeg' not found in PATH",
            Style::default().fg(Color::Rgb(243, 139, 168)),
        ));
    }
    lines.push(Line::from(ffmpeg_spans));

    // 4. LaTeX/dvisvgm
    let mut latex_spans = Vec::new();
    if let Some(ref path) = report.dvisvgm_path {
        latex_spans.push(check_mark.clone());
        latex_spans.push(Span::styled(
            "LaTeX/dvisvgm:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        latex_spans.push(Span::styled(
            format!(" {}", path.display()),
            Style::default().fg(Color::Rgb(205, 214, 244)),
        ));
    } else {
        latex_spans.push(cross_mark.clone());
        latex_spans.push(Span::styled(
            "LaTeX/dvisvgm:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ));
        latex_spans.push(Span::styled(
            " Not found (LaTeX equations will fail to compile)",
            Style::default().fg(Color::Rgb(250, 179, 135)),
        ));
    }
    lines.push(Line::from(latex_spans));

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Recommended Remediation Steps:",
        Style::default()
            .fg(Color::Rgb(180, 190, 254))
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  1. Create and activate a Python virtual environment:",
        Style::default().fg(Color::Rgb(205, 214, 244)),
    )));
    lines.push(Line::from(Span::styled(
        "       python3 -m venv .venv && source .venv/bin/activate",
        Style::default().fg(Color::Rgb(166, 227, 161)),
    )));
    lines.push(Line::from(Span::styled(
        "  2. Install Manim Community Edition:",
        Style::default().fg(Color::Rgb(205, 214, 244)),
    )));
    lines.push(Line::from(Span::styled(
        "       pip install manim",
        Style::default().fg(Color::Rgb(166, 227, 161)),
    )));
    lines.push(Line::from(Span::styled(
        "  3. Install LaTeX support (Ubuntu/Debian):",
        Style::default().fg(Color::Rgb(205, 214, 244)),
    )));
    lines.push(Line::from(Span::styled(
        "       sudo apt install texlive-latex-extra dvisvgm",
        Style::default().fg(Color::Rgb(166, 227, 161)),
    )));
    lines.push(Line::raw(""));

    let footer = Line::from(vec![
        Span::styled(" [ Press ", Style::default().fg(Color::Rgb(147, 153, 178))),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Rgb(137, 180, 250))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to retry detection ]", Style::default().fg(Color::Rgb(147, 153, 178))),
        Span::raw("        "),
        Span::styled("[ Press ", Style::default().fg(Color::Rgb(147, 153, 178))),
        Span::styled(
            "Esc / q",
            Style::default()
                .fg(Color::Rgb(243, 139, 168))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to exit ] ", Style::default().fg(Color::Rgb(147, 153, 178))),
    ]);
    lines.push(footer);

    let p = Paragraph::new(lines);
    p.render(inner_area, buf);
}

fn resolve_editor(explicit: Option<&str>) -> String {
    if let Some(e) = explicit {
        if !e.trim().is_empty() {
            return e.to_string();
        }
    }

    if let Ok(e) = std::env::var("EDITOR") {
        if !e.trim().is_empty() {
            return e;
        }
    }

    if is_command_available("micro") {
        return "micro".to_string();
    }

    if is_command_available("nano") {
        return "nano".to_string();
    }

    "vi".to_string()
}

fn is_command_available(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn ensure_target_file(path: &Path, scene_name: Option<&str>) -> Result<()> {
    if !path.exists() {
        let raw_scene = scene_name.unwrap_or("LiveScene");
        let sanitized_scene: String = raw_scene
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let sanitized_scene = if sanitized_scene.is_empty()
            || sanitized_scene.chars().next().unwrap().is_ascii_digit()
        {
            format!("Scene_{}", sanitized_scene)
        } else {
            sanitized_scene
        };

        let starter_code = format!(
            r#"from manim import *

class {sanitized_scene}(Scene):
    def construct(self):
        title = Text("Hello from manim-live!", font_size=40)
        circle = Circle(radius=2.0, color=BLUE)
        circle.set_fill(PINK, opacity=0.5)

        self.play(Write(title))
        self.wait(0.5)
        self.play(Transform(title, circle))
        self.wait(1)
"#
        );
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, starter_code)
            .with_context(|| format!("Failed to create starter scene at {}", path.display()))?;
    }
    Ok(())
}

fn detect_first_scene_name(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let scene_re = regex::Regex::new(r"class\s+([A-Za-z0-9_]+)\s*\([^)]*Scene[^)]*\):").ok()?;
    if let Some(caps) = scene_re.captures(&content) {
        return Some(caps[1].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitized_scene_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target = temp_dir.path().join("test_scene.py");
        ensure_target_file(&target, Some("123-Bad Name!@#")).unwrap();
        let content = fs::read_to_string(&target).unwrap();
        assert!(content.contains("class Scene_123BadName(Scene):"));
    }

    #[test]
    fn test_probe_environment() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target = temp_dir.path().join("dummy.py");
        let report = probe_environment(&target, None);
        // Python and ffmpeg are expected on this Linux testing system
        assert!(report.python_path.is_some());
    }
}
