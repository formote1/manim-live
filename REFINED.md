# SPECIFICATION & IMPLEMENTATION BLUEPRINT: `manim-live` REFINEMENT

> **Target Codebase:** `manim-live` (Single-binary Rust TUI studio for Python Manim)  
> **Audience:** Autonomous Coding Agent / Systems Engineer  
> **Objective:** Comprehensive implementation blueprint for performance optimization, dual-speed rendering, scrubbing ergonomics, diagnostic intelligence, security hardening, and aesthetic modernization.

---

## Table of Contents
1. [Architecture Overview & State Flow](#1-architecture-overview--state-flow)
2. [Dependency Updates (`Cargo.toml`)](#2-dependency-updates-cargotoml)
3. [Phase 0: Critical Resource Leaks & High-Performance Graphics (P0)](#3-phase-0-critical-resource-leaks--high-performance-graphics-p0)
   - [3.1 RAII Temp Directory Cleanup](#31-raii-temp-directory-cleanup)
   - [3.2 Zero-Copy Kitty Graphics Transmission (`t=f`)](#32-zero-copy-kitty-graphics-transmission-tf)
   - [3.3 Ratatui Cell Skipping & Buffer Protection](#33-ratatui-cell-skipping--buffer-protection)
   - [3.4 Memory-Backed Shared Storage (`/dev/shm`)](#34-memory-backed-shared-storage-devshm)
4. [Phase 1: Dual-Speed Compilation Engine (P1)](#4-phase-1-dual-speed-compilation-engine-p1)
   - [4.1 Architecture: Fast Snapshot vs. Full Video](#41-architecture-fast-snapshot-vs-full-video)
   - [4.2 Runner & App State Machine Modifications](#42-runner--app-state-machine-modifications)
5. [Phase 2: Interactive Timeline Scrubber & Frame Inspection (P2)](#5-phase-2-interactive-timeline-scrubber--frame-inspection-p2)
   - [5.1 Scrubbing State & Keybind Ergonomics](#51-scrubbing-state--keybind-ergonomics)
   - [5.2 Timeline & Overlay Widget Implementation](#52-timeline--overlay-widget-implementation)
6. [Phase 3: Compiler Diagnostics & PTY Editor Line Jump (P3)](#6-phase-3-compiler-diagnostics--pty-editor-line-jump-p3)
   - [6.1 Python Traceback & LaTeX Error Parsing](#61-python-traceback--latex-error-parsing)
   - [6.2 Auto-Jump Escape Dispatch via PTY](#62-auto-jump-escape-dispatch-via-pty)
7. [Phase 4: Pre-Flight Dependency Checker & Onboarding (P4)](#7-phase-4-pre-flight-dependency-checker--onboarding-p4)
   - [7.1 Environment Probe Routine](#71-environment-probe-routine)
   - [7.2 Actionable Diagnostic TUI Screen](#72-actionable-diagnostic-tui-screen)
8. [Phase 5: Interactive Scene Palette & Fuzzy Switcher (P5)](#8-phase-5-interactive-scene-palette--fuzzy-switcher-p5)
   - [8.1 Scene Discovery State Machine](#81-scene-discovery-state-machine)
   - [8.2 Palette Overlay Rendering & Hotkeys](#82-palette-overlay-rendering--hotkeys)
9. [Phase 6: Visual Hierarchy, Border Polish & Accessibility (P6)](#9-phase-6-visual-hierarchy-border-polish--accessibility-p6)
   - [9.1 Dynamic Border Thickness & Focus Signatures](#91-dynamic-border-thickness--focus-signatures)
   - [9.2 Breadcrumb Header & Status Bar Refinements](#92-breadcrumb-header--status-bar-refinements)
   - [9.3 Accessibility & Terminal Bell Support](#93-accessibility--terminal-bell-support)
10. [Phase 7: Security Hardening & Robustness (P7)](#10-phase-7-security-hardening--robustness-p7)
    - [10.1 Elimination of Arbitrary `sh -c` Shell Execution](#101-elimination-of-arbitrary-sh--c-shell-execution)
    - [10.2 Robust PTY Command Argument Tokenization](#102-robust-pty-command-argument-tokenization)
    - [10.3 Python Identifier Sanitization](#103-python-identifier-sanitization)
11. [Verification & Test Matrix](#11-verification--test-matrix)

---

## 1. Architecture Overview & State Flow

```
                                  ┌───────────────────────────┐
                                  │      FileWatcher          │
                                  │ (Debounced mtime + hash)  │
                                  └─────────────┬─────────────┘
                                                │ (On Disk Write)
                                                ▼
┌───────────────────────────┐      ┌───────────────────────────┐      ┌───────────────────────────┐
│     Code Editor (PTY)     │◄────►│         App Loop          │◄────►│       Live Preview        │
│  (Micro / Vim / Neovim)   │      │  (Event Routing & Layout) │      │  (Kitty t=f / Halfblocks) │
└───────────────────────────┘      └─────────────┬─────────────┘      └───────────────────────────┘
                                                │
                                                │ Triggers
                                                ▼
                                  ┌───────────────────────────┐
                                  │        BuildRunner        │
                                  │ Tier 1: Fast Snapshot (-s)│
                                  │ Tier 2: Full MP4 Pipeline │
                                  └─────────────┬─────────────┘
                                                │
                         ┌──────────────────────┴──────────────────────┐
                         ▼                                             ▼
            ┌─────────────────────────┐                   ┌─────────────────────────┐
            │   RAM Disk (/dev/shm)   │                   │    Console & Terminal   │
            │  Frames & RAII Cleaners │                   │  Diagnostics & Trace    │
            └─────────────────────────┘                   └─────────────────────────┘
```

---

## 2. Dependency Updates (`Cargo.toml`)

Add `tempfile` for RAII filesystem lifecycle management and `shell-words` for secure argument parsing:

```toml
[dependencies]
clap = { version = "4.5", features = ["derive", "env"] }
ratatui = { version = "0.30", default-features = false, features = ["crossterm"] }
crossterm = "0.29"
portable-pty = "0.9"
notify = "6.1"
notify-debouncer-mini = "0.7"
vt100 = "0.18"
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
base64 = "0.22"
regex = "1.10"
anyhow = "1.0"
crossbeam-channel = "0.5"
tempfile = "3.14"
shell-words = "1.1"
```

---

## 3. Phase 0: Critical Resource Leaks & High-Performance Graphics (P0)

### 3.1 RAII Temp Directory Cleanup
* **Target File:** `src/runner.rs`
* **Issue:** `extract_video_frames()` uses `let temp_dir = std::env::temp_dir().join(format!("manim_live_{}", unique_id));` with zero cleanup. Over an extended session, gigabytes of PNG files accumulate in `/tmp`.
* **Fix:** Wrap the frame extraction directory in `tempfile::TempDir` and persist the directory handle inside `BuildRunner` or `PreviewState` so that old frames are deleted immediately when a new build completes.

#### Data Structure Addition in `src/runner.rs`:
```rust
#[derive(Debug)]
pub struct FrameArtifacts {
    pub frames: Vec<PathBuf>,
    pub temp_dir_guard: Option<Arc<tempfile::TempDir>>,
}
```

Update `BuildEvent::Finished` in `src/runner.rs`:
```rust
pub enum BuildEvent {
    Started { command: String },
    OutputLine(String),
    Progress { percentage: f32, message: String },
    Finished {
        success: bool,
        duration: Duration,
        output_path: Option<PathBuf>,
        artifacts: FrameArtifacts,
    },
}
```

### 3.2 Zero-Copy Kitty Graphics Transmission (`t=f`)
* **Target File:** `src/ui/preview.rs`
* **Issue:** `emit_kitty_image()` currently re-reads PNG bytes, encodes them to Base64, and splits them into 4096-byte chunks pushed over the PTY on every single frame tick. For 30 FPS playback, this generates several megabytes per second of terminal wire traffic, causing cursor lag and high CPU utilization.
* **Fix:** Use Kitty's direct file-path transmission (`t=f`). Kitty loads the image directly from disk into GPU memory without transmitting pixel data over the wire.

#### Implementation in `src/ui/preview.rs`:
```rust
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
```

### 3.3 Ratatui Cell Skipping & Buffer Protection
* **Target File:** `src/ui/preview.rs`
* **Issue:** When Kitty renders an image, Ratatui's draw pass attempts to diff and repaint background spaces, causing graphic flicker.
* **Fix:** Iterate across all cells in the inner preview area covered by the image and call `cell.set_skip(true)`:
```rust
for r in 0..fit_rows {
    for c in 0..fit_cols {
        let screen_x = fit_x + c;
        let screen_y = fit_y + r;
        if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
            cell.set_skip(true);
        }
    }
}
```

### 3.4 Memory-Backed Shared Storage (`/dev/shm`)
* **Target File:** `src/runner.rs`
* **Implementation:** When creating temporary build directories on Linux systems, prefer `/dev/shm` (RAM disk) to completely bypass physical disk I/O:
```rust
pub fn create_fast_temp_dir() -> std::io::Result<tempfile::TempDir> {
    let shm = Path::new("/dev/shm");
    if shm.is_dir() && shm.exists() {
        tempfile::Builder::new().prefix("manim_live_").tempdir_in(shm)
    } else {
        tempfile::Builder::new().prefix("manim_live_").tempdir()
    }
}
```

---

## 4. Phase 1: Dual-Speed Compilation Engine (P1)

### 4.1 Architecture: Fast Snapshot vs. Full Video
Manim supports `--save_last_frame` (`-s`). In snapshot mode, Manim evaluates Python scene definitions, renders only the terminal frame, and skips video container assembly.

```
Compilation Modes:
1. Fast Snapshot (Interactive Mode):
   - Command: `manim -s -ql <file.py> <Scene>`
   - Latency: ~250ms – 400ms
   - Trigger: Triggered automatically on editor file save (`Ctrl+S` or watcher event)
   - Result: 1 frame immediately loaded into Preview viewport

2. Full Animation (Playback Mode):
   - Command: `manim -ql --fps <fps> <file.py> <Scene>`
   - Latency: ~3s – 12s
   - Trigger: Triggered when user presses Space (Play), F5 (Rebuild), or after 2.5s of editor inactivity
   - Result: Full vector of sequential frames extracted for real-time scrubbing and loop playback
```

### 4.2 Runner & App State Machine Modifications

#### In `src/runner.rs`:
Add an explicit build mode enum:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    SnapshotOnly,
    FullVideo,
}
```

Modify `trigger_build()` to accept `BuildMode`:
```rust
pub fn trigger_build(
    &self,
    file_path: &Path,
    scene_name: Option<&str>,
    quality: Quality,
    fps: u32,
    mode: BuildMode,
    explicit_manim: Option<&str>,
    quality_cap: Quality,
    fps_cap: u32,
)
```
When `mode == BuildMode::SnapshotOnly`:
- Add `-s` argument to the `Command`.
- Omit `--fps` argument (not needed for static snapshot).
- Skip the FFmpeg frame extraction step; locate the generated `.png` directly and populate `artifacts.frames` with the single image path.

#### In `src/app.rs`:
Handle file watcher events by requesting `BuildMode::SnapshotOnly`:
```rust
if got_save {
    if !matches!(self.console_state.build_state, BuildState::Building { .. }) {
        self.console_state.add_line("─── Quick save: Rendering fast snapshot (-s) ───".to_string());
        self.runner.trigger_build(
            &self.config.file_path,
            self.config.scene_name.as_deref(),
            self.config.quality,
            self.config.fps,
            BuildMode::SnapshotOnly,
            self.config.manim_bin.as_deref(),
            self.settings_state.quality_cap,
            self.settings_state.fps_cap,
        );
        needs_redraw = true;
    }
}
```
When user presses `F5` or `Space` in the Preview pane, trigger `BuildMode::FullVideo`.

---

## 5. Phase 2: Interactive Timeline Scrubber & Frame Inspection (P2)

### 5.1 Scrubbing State & Keybind Ergonomics
Animators need frame-accurate seeking to inspect geometric positioning and text alignments.

#### In `src/ui/preview.rs`:
Expand `PreviewState` to support rich timeline metrics:
```rust
pub struct PreviewState {
    pub protocol: ImageProtocol,
    pub frames: Vec<PathBuf>,
    pub current_frame_idx: usize,
    pub is_playing: bool,
    pub fps: u32,
    pub last_tick: Instant,
    pub frame_guard: Option<Arc<tempfile::TempDir>>,
    // Timeline metrics
    pub is_scrubbing: bool,
    pub force_redraw: bool,
}
```

#### New Navigation Hotkeys when `FocusPane::Preview`:
| Key | Action | Behavior |
| :--- | :--- | :--- |
| `Space` | Toggle Play/Pause | Pauses frame progression or starts looping |
| `h` / `Left` | Step -1 Frame | Steps backward by exactly 1 frame |
| `l` / `Right` | Step +1 Frame | Steps forward by exactly 1 frame |
| `H` / `Shift+Left` | Seek -10 Frames | Rapid back-scrub |
| `L` / `Shift+Right` | Seek +10 Frames | Rapid forward-scrub |
| `0` / `Home` | Seek to Start | Jumps to Frame 0 (`t = 0.00s`) |
| `$` / `End` | Seek to End | Jumps to Last Frame |
| `[` / `]` | Speed Control | Cycles playback speed (0.5x, 1.0x, 2.0x) |

### 5.2 Timeline & Overlay Widget Implementation
Render a floating control HUD at the bottom of the preview pane inside `PreviewState::render()`:

```rust
// Layout: Reserve 2 rows at the bottom of inner_area for Timeline HUD
let hud_area = Rect::new(inner_area.x, inner_area.bottom().saturating_sub(2), inner_area.width, 2);
let graphic_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, inner_area.height.saturating_sub(2));

// Calculate timeline progress
let total_frames = self.frames.len();
let current_idx = self.current_frame_idx;
let fps = self.fps.max(1) as f64;
let cur_secs = current_idx as f64 / fps;
let total_secs = total_frames as f64 / fps;

// Format track: 00:01.20 [ ═══════●════════════ ] 00:03.40 (Frame 18/51)
let track_width = (hud_area.width as usize).saturating_sub(38).max(10);
let progress_ratio = if total_frames > 1 { current_idx as f64 / (total_frames - 1) as f64 } else { 0.0 };
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

let hud_line = Line::from(vec![
    Span::styled(format!("{:02}:{:05.2} ", (cur_secs / 60.0).floor() as u32, cur_secs % 60.0), Style::default().fg(palette.accent_blue).add_modifier(Modifier::BOLD)),
    Span::styled("[ ", Style::default().fg(palette.text_dimmed)),
    Span::styled(track_bar, Style::default().fg(palette.accent_mauve)),
    Span::styled(" ] ", Style::default().fg(palette.text_dimmed)),
    Span::styled(format!("{:02}:{:05.2} ", (total_secs / 60.0).floor() as u32, total_secs % 60.0), Style::default().fg(palette.text_muted)),
    Span::styled(format!("(F:{}/{})", current_idx + 1, total_frames), Style::default().fg(palette.text_dimmed)),
]);
```

---

## 6. Phase 3: Compiler Diagnostics & PTY Editor Line Jump (P3)

### 6.1 Python Traceback & LaTeX Error Parsing
* **Target File:** `src/ui/console.rs`
* **Issue:** When a render fails, the user must read the traceback in the console and manually navigate their editor to the target line.
* **Fix:** Build an active diagnostic parser in `ConsoleState` that inspects incoming log lines.

#### In `src/ui/console.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticError {
    pub file_path: PathBuf,
    pub line_number: u32,
    pub error_message: String,
    pub is_latex: bool,
}

pub struct ConsoleState {
    // ... existing fields ...
    pub active_error: Option<DiagnosticError>,
}
```

Add parsing logic inside `ConsoleState::add_line()`:
```rust
// Regex for Python tracebacks: File "scene.py", line 42, in construct
let py_re = regex::Regex::new(r#"File\s+["']([^"']+)["'],\s+line\s+(\d+)(?:,\s+in\s+(.*))?"#).unwrap();
// Regex for LaTeX errors: ! LaTeX Error: File `amsmath.sty' not found.
let latex_re = regex::Regex::new(r#"!\s+LaTeX Error:\s+(.*)"#).unwrap();

if let Some(caps) = py_re.captures(&line) {
    let file = PathBuf::from(&caps[1]);
    let line_no = caps[2].parse::<u32>().unwrap_or(1);
    let msg = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_else(|| "Error in construct".to_string());
    self.active_error = Some(DiagnosticError {
        file_path: file,
        line_number: line_no,
        error_message: msg,
        is_latex: false,
    });
} else if let Some(caps) = latex_re.captures(&line) {
    if let Some(ref mut err) = self.active_error {
        err.error_message = format!("LaTeX: {}", &caps[1]);
        err.is_latex = true;
    }
}
```

### 6.2 Auto-Jump Escape Dispatch via PTY
When an active error is detected, display a prominent jump action in the status bar:
` [F7: Jump to Line 42] `

When `F7` is pressed, send the appropriate cursor repositioning escape sequence to the PTY via `PtyManager`:
- For **Vim / Neovim**: Send `\x1b:42\r`
- For **Micro**: Send `\x1b:42\r` or `\x05goto 42\r`
- For **Nano**: Send `\x1b_42\r`

#### In `src/pty.rs`:
```rust
impl PtyManager {
    pub fn jump_to_line(&mut self, line_number: u32, editor_cmd: &str) -> std::io::Result<()> {
        let cmd = editor_cmd.to_lowercase();
        if cmd.contains("vim") || cmd.contains("nvim") || cmd.contains("micro") {
            // Send Escape to ensure normal mode, then colon command
            let seq = format!("\x1b:set nonu\r\x1b:{}\r", line_number);
            self.write_raw(seq.as_bytes())?;
        } else if cmd.contains("nano") {
            // Nano shortcut for line jump: Ctrl+_ then line number
            let seq = format!("\x1f{}\r", line_number);
            self.write_raw(seq.as_bytes())?;
        }
        Ok(())
    }
}
```

---

## 7. Phase 4: Pre-Flight Dependency Checker & Onboarding (P4)

### 7.1 Environment Probe Routine
* **Target File:** `src/main.rs`
* **Implementation:** Before initializing raw mode and launching `App`, run a non-destructive environment probe checking all critical dependencies:

```rust
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
    
    let python_path = which::which("python3").or_else(|_| which::which("python")).ok();
    let ffmpeg_path = which::which("ffmpeg").ok();
    let dvisvgm_path = which::which("dvisvgm").ok();

    DependencyReport {
        python_path,
        python_version: get_cmd_version("python3", &["--version"]),
        manim_path,
        manim_version: get_cmd_version("manim", &["--version"]),
        ffmpeg_path,
        dvisvgm_path,
    }
}
```

### 7.2 Actionable Diagnostic TUI Screen
If any essential dependency (`manim` or `ffmpeg`) is missing, render an actionable onboarding screen instead of a crash:

```text
┌─────────────────────────── System Readiness Check ───────────────────────────┐
│                                                                              │
│  ✔ Python:       /usr/bin/python3 (v3.12.3)                                  │
│  ✖ Manim:        Executable 'manim' not found in PATH or virtualenvs         │
│  ✔ FFmpeg:       /usr/bin/ffmpeg                                             │
│  ✖ LaTeX/dvisvgm: Not found (LaTeX equations will fail to compile)           │
│                                                                              │
│  Recommended Remediation Steps:                                              │
│  1. Create and activate a Python virtual environment:                        │
│       python3 -m venv .venv && source .venv/bin/activate                     │
│  2. Install Manim Community Edition:                                         │
│       pip install manim                                                      │
│  3. Install LaTeX support (Ubuntu/Debian):                                   │
│       sudo apt install texlive-latex-extra dvisvgm                           │
│                                                                              │
│  [ Press Enter to retry detection ]                  [ Press Esc / q to exit ]│
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 8. Phase 5: Interactive Scene Palette & Fuzzy Switcher (P5)

### 8.1 Scene Discovery State Machine
* **Target File:** `src/app.rs`
* **Trigger:** Global hotkey `F8` or `Ctrl+P` opens the Scene Palette overlay.

```rust
pub struct ScenePaletteState {
    pub is_open: bool,
    pub available_scenes: Vec<String>,
    pub selected_idx: usize,
    pub search_query: String,
}
```

### 8.2 Palette Overlay Rendering & Hotkeys
Render a centered modal popup (similar to VS Code's command palette):
- **Input Field:** Type characters to filter available scenes in real time.
- **List Box:** Shows filtered scene classes. The currently active scene is marked with an asterisk: `* SpirographScene (Active)`.
- **Keyboard Handling:**
  - `Up` / `Down`: Move selection highlight.
  - `Enter`: Switch `config.scene_name` to selected scene, close palette, and immediately trigger `BuildMode::SnapshotOnly`.
  - `Esc`: Dismiss modal without changes.

---

## 9. Phase 6: Visual Hierarchy, Border Polish & Accessibility (P6)

### 9.1 Dynamic Border Thickness & Focus Signatures
* **Target File:** `src/app.rs` & `src/ui/layout.rs`
* **Styling Policy:**
  - **Focused Pane:** `BorderType::Thick` (or `BorderType::Double`), border color set to `palette.accent_mauve`.
  - **Unfocused Panes:** `BorderType::Rounded`, border color set to `palette.border_unfocused`.
  - **Active Header Badge:** Left-aligned highlighted badge on the active pane header:
    ` █ Code: scene.py [ACTIVE] `

### 9.2 Breadcrumb Header & Status Bar Refinements
* **Target File:** `src/ui/mod.rs`
* Replace plain file names with full contextual breadcrumbs:
  ` ❪ ~/Projects/Research/scene.py › SpirographScene ❫ `
* In the status bar ([`render_status_bar`](file:///home/chuck/Projects/manim-live/src/ui/mod.rs#L55-L121)), center-align the action hints with distinct high-contrast keyboard pill badges:
  ` [F1 Code]  [F2 Preview]  [F3 Console]  [F4 Settings]  [F5 Build]  [F8 Scenes]  [Ctrl+Q Quit] `

### 9.3 Accessibility & Terminal Bell Support
* In `src/app.rs`: When a build finishes in the background:
  - If `success == true`: emit visual status update.
  - If `success == false`: emit terminal bell `\x07` to audibly notify the user even if their terminal window is minimized or focused on another pane.
* Add high-contrast mode toggle in `src/ui/theme.rs` with `BorderType::Plain` and pure black/white high-visibility tokens.

---

## 10. Phase 7: Security Hardening & Robustness (P7)

### 10.1 Elimination of Arbitrary `sh -c` Shell Execution
* **Target File:** `src/ui/console.rs` (Lines 502–528)
* **Vulnerability:** Passing untrusted strings directly into `Command::new("sh").arg("-c").arg(&sh_cmd)` allows arbitrary command execution and subshell injection.
* **Remediation:** Replace with direct executable execution without invoking a shell interpreter:

```rust
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
            }
            Err(e) => {
                self.add_line(format!("Execution failed: {}", e));
            }
        }
    }
    ConsoleAction::None
}
```

### 10.2 Robust PTY Command Argument Tokenization
* **Target File:** `src/pty.rs` (Lines 55–61)
* **Issue:** `editor_cmd.split_whitespace().collect()` breaks when editor executable paths contain spaces (e.g., `/opt/my editors/micro`).
* **Remediation:** Use `shell_words::split(editor_cmd)` for standards-compliant quote-aware argument tokenization:

```rust
let parts = shell_words::split(editor_cmd).unwrap_or_else(|_| vec![editor_cmd.to_string()]);
let prog = parts.first().map(|s| s.as_str()).unwrap_or("micro");
let mut cmd = CommandBuilder::new(prog);
for arg in parts.iter().skip(1) {
    cmd.arg(arg);
}
cmd.arg(file_path.as_ref().as_os_str());
```

### 10.3 Python Identifier Sanitization
* **Target File:** `src/main.rs` (Lines 173–198)
* **Issue:** `ensure_target_file()` interpolates scene names directly into Python template strings: `format!(r#"class {scene}(Scene):..."#)`. If `scene` contains hyphens or symbols, it creates invalid Python syntax.
* **Fix:** Sanitize scene names:
```rust
let safe_scene = scene.chars()
    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
    .collect::<String>();
let final_scene = if safe_scene.is_empty() || safe_scene.chars().next().unwrap().is_ascii_digit() {
    format!("Scene_{}", safe_scene)
} else {
    safe_scene
};
```

---

## 11. Verification & Test Matrix

To verify that the modifications are functioning correctly, execute these automated and manual verification procedures:

### Automated Unit & Integration Tests
1. **Zero Leak Verification:**
   ```bash
   cargo test -- --nocapture test_raii_temp_cleanup
   ```
   Verify that `/dev/shm/manim_live_*` or `/tmp/manim_live_*` directories are removed immediately after a build finishes or when the process exits.
2. **Kitty Protocol Verification:**
   Run `manim-live` inside Kitty or Ghostty. Verify that `emit_kitty_image_path` outputs `\x1b_Ga=T,f=100,t=f...` without intermediate Base64 pixel stream chunks.
3. **Dual-Speed Compilation Benchmark:**
   - Save file: Verify that build duration completes in $<400\text{ms}$ with `-s` flag.
   - Press Space: Verify full animation compile kicks off in background without blocking editor input.

### Manual UX Checklist
- [ ] Save scene file in Micro/Vim -> Final frame snapshot appears in $<400\text{ms}$.
- [ ] Press `Space` in Preview -> Full video compiles and begins looping at target FPS.
- [ ] Press `Left` / `Right` -> Steps exactly 1 frame forward/backward with interactive timeline track indicator.
- [ ] Introduce a Python syntax error -> Error appears in console; pressing `F7` jumps the editor cursor to that exact line.
- [ ] Press `F8` -> Scene Palette pops up; typing filters scenes; pressing `Enter` switches active scene instantly.
- [ ] Type `exec ls` in console -> Executes safely without invoking `/bin/sh`.
