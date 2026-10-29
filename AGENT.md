# MISSION: Build `manim-live` in Rust

You are a senior systems engineer building a standalone, single-binary Terminal User Interface (TUI) application named `manim-live` written in Rust.

## Objective
`manim-live` provides a real-time, interactive development environment for Python's Manim animation engine directly inside modern terminal emulators. It automatically detects file edits, triggers low-overhead Manim builds, displays live frame/video previews inside the terminal pane using modern terminal image protocols (primarily Kitty graphics protocol), and streams build logs in an adjacent pane.

---

## 1. Interface & Tiling Architecture
The application must draw an exact 3-pane TUI using `ratatui` and `crossterm`:
- **Left Pane (Master, ~55% width):** Code view / Active editor buffer. If launching an external editor like `micro` or `$EDITOR`, embed it cleanly using a PTY (`portable-pty`) or configure standard dual-mode operation.
- **Top-Right Pane (~45% width, ~50% height):** Live Preview viewport. Displays either the rendered `.mp4` video (via looping terminal escapes or frame stepping) or the latest rendered `.png` frame using Kitty graphics protocol (`<ESC>_G...`) and Sixel fallback (consider using `ratatui-image`).
- **Bottom-Right Pane (~45% width, ~50% height):** Build Status & Console. Displays real-time compile timers, frame render percentages, Manim stdout/stderr output, and clear error logs.

---

## 2. Core Functional Requirements

### A. CLI Interface (`clap`)
Accept CLI invocations such as:
- `manim-live <file.py> [SceneName]`
- Flags:
  - `-e, --editor <EDITOR>`: Editor command (defaults to `micro`, fallback to `$EDITOR`, then `nano`).
  - `-q, --quality <low|medium|high>`: Manim quality profile (defaults to `low` / `-ql` for speed).
  - `-f, --format <mp4|png>`: Default preview format (image of final frame vs full video render).
  - `--fps <FPS>`: Frame rate cap for live previews.

### B. File Watcher & Debounce
- Use the `notify` or `notify-debouncer-mini` crate.
- Watch the target `.py` file for disk write/save events.
- Debounce saves by 250–400ms to avoid re-triggering compilation mid-save.

### C. Build Worker
- On file save, spawn the Manim compiler as an asynchronous subprocess:
  `manim -ql <file.py> <SceneName>` (or `-qs` for quick single-frame snapshot).
- Stream stdout and stderr continuously into the Bottom-Right pane buffer without blocking the UI thread.
- Track render duration and compute compile time statistics.

### D. Preview Renderer
- Detect terminal image support (check for Kitty protocol support via `$TERM`, `$KITTY_WINDOW_ID`, or terminal capability query).
- Render preview assets cleanly constrained within the coordinates/bounding box of the Top-Right pane.
- Clear previous graphics buffer before repainting new renders.

---

## 3. Project Structure
Organize the Rust codebase cleanly:
```text
manim-live/
├── Cargo.toml
├── src/
│   ├── main.rs            # Entry point & CLI parsing
│   ├── app.rs             # Application state machine
│   ├── ui/                # Ratatui layout & pane rendering
│   │   ├── mod.rs
│   │   ├── layout.rs
│   │   ├── preview.rs     # Kitty/Sixel image rasterizer
│   │   └── console.rs     # Stdout/log renderer
│   ├── watcher.rs         # File watching & debouncing
│   ├── runner.rs          # Subprocess executor for Manim
│   └── pty.rs             # PTY handling for embedded micro/editor
├── install.sh             # curl | bash installation script
└── .github/
    └── workflows/
        └── release.yml    # Cross-compilation & GitHub releases
```
