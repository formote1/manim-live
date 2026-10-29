use anyhow::{Context, Result};
use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::{
    io::{Read, Write},
    num::NonZeroU16,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use vt100::{Color as VtColor, Parser};

pub struct PtyManager {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<Parser>>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
    dirty: Arc<AtomicBool>,
}

impl PtyManager {
    pub fn spawn<P: AsRef<Path>>(
        editor_cmd: &str,
        file_path: P,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let cols_safe = cols.max(10);
        let rows_safe = rows.max(5);

        let pty_size = PtySize {
            rows: rows_safe,
            cols: cols_safe,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(pty_size)
            .context("Failed to create pseudo-terminal")?;

        // Parse editor command and arguments using robust shell_words tokenization
        let parts = shell_words::split(editor_cmd).unwrap_or_else(|_| vec![editor_cmd.to_string()]);
        let prog = parts.first().map(|s| s.as_str()).unwrap_or("micro");
        let mut cmd = CommandBuilder::new(prog);
        for arg in parts.iter().skip(1) {
            cmd.arg(arg);
        }
        cmd.arg(file_path.as_ref().as_os_str());

        // Setup terminal environment variables
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn editor process inside PTY")?;

        let rows_nz = NonZeroU16::new(rows_safe).unwrap_or(NonZeroU16::new(24).unwrap());
        let cols_nz = NonZeroU16::new(cols_safe).unwrap_or(NonZeroU16::new(80).unwrap());
        let parser = Arc::new(Mutex::new(Parser::new(rows_nz, cols_nz, 1000)));

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to clone PTY master reader")?;
        let parser_clone = Arc::clone(&parser);
        let dirty = Arc::new(AtomicBool::new(true));
        let dirty_clone = Arc::clone(&dirty);

        // Background thread reads raw output from the PTY and passes it into vt100 parser
        thread::Builder::new()
            .name("pty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser_clone.lock() {
                                p.process(&buf[..n]);
                                dirty_clone.store(true, Ordering::SeqCst);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .context("Failed to spawn PTY reader thread")?;

        let writer = pair
            .master
            .take_writer()
            .context("Failed to acquire PTY master writer")?;

        Ok(Self {
            master: pair.master,
            writer,
            parser,
            child,
            size: (cols_safe, rows_safe),
            dirty,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::SeqCst)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(10);
        let rows = rows.max(5);
        if self.size == (cols, rows) {
            return;
        }
        self.size = (cols, rows);

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = self.master.resize(pty_size);

        if let (Some(r), Some(c)) = (NonZeroU16::new(rows), NonZeroU16::new(cols)) {
            if let Ok(mut parser) = self.parser.lock() {
                parser.set_size(r, c);
            }
        }
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn write_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

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

    pub fn handle_key_event(&mut self, key: KeyEvent) -> std::io::Result<()> {
        let bytes = key_event_to_bytes(key);
        if !bytes.is_empty() {
            self.write_raw(&bytes)?;
        }
        Ok(())
    }

    pub fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        offset_x: u16,
        offset_y: u16,
    ) -> std::io::Result<()> {
        let col = (mouse.column.saturating_sub(offset_x) + 1).min(self.size.0);
        let row = (mouse.row.saturating_sub(offset_y) + 1).min(self.size.1);

        let (base_code, is_release) = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => (0, false),
            MouseEventKind::Down(MouseButton::Middle) => (1, false),
            MouseEventKind::Down(MouseButton::Right) => (2, false),
            MouseEventKind::Up(MouseButton::Left) => (0, true),
            MouseEventKind::Up(MouseButton::Middle) => (1, true),
            MouseEventKind::Up(MouseButton::Right) => (2, true),
            MouseEventKind::Drag(MouseButton::Left) => (32, false),
            MouseEventKind::Drag(MouseButton::Middle) => (33, false),
            MouseEventKind::Drag(MouseButton::Right) => (34, false),
            MouseEventKind::ScrollUp => (64, false),
            MouseEventKind::ScrollDown => (65, false),
            MouseEventKind::ScrollLeft => (66, false),
            MouseEventKind::ScrollRight => (67, false),
            MouseEventKind::Moved => return Ok(()),
        };

        let mut code = base_code;
        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
            code += 4;
        }
        if mouse.modifiers.contains(KeyModifiers::ALT) {
            code += 8;
        }
        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
            code += 16;
        }

        let suffix = if is_release { 'm' } else { 'M' };
        let sgr = format!("\x1b[<{};{};{}{}", code, col, row, suffix);
        self.write_raw(sgr.as_bytes())
    }

    /// Renders the vt100 virtual screen buffer into the specified Ratatui rect buffer.
    /// Returns cursor coordinates (x, y) if the cursor should be visible.
    pub fn render_to_buffer(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        let parser = match self.parser.lock() {
            Ok(p) => p,
            Err(_) => return None,
        };
        let screen = parser.screen();

        let height = area.height;
        let width = area.width;

        for row in 0..height {
            for col in 0..width {
                let cell_x = area.x + col;
                let cell_y = area.y + row;

                if cell_x >= buf.area.right() || cell_y >= buf.area.bottom() {
                    continue;
                }

                if let Some(target_cell) = buf.cell_mut((cell_x, cell_y)) {
                    if let Some(cell) = screen.cell(row, col) {
                        let contents = cell.contents();
                        if !contents.is_empty() {
                            target_cell.set_symbol(contents);
                        } else {
                            target_cell.set_symbol(" ");
                        }

                        let mut style = Style::default();
                        style = style.fg(convert_vt_color(cell.fgcolor()));
                        style = style.bg(convert_vt_color(cell.bgcolor()));

                        let mut modifier = Modifier::empty();
                        if cell.bold() {
                            modifier |= Modifier::BOLD;
                        }
                        if cell.italic() {
                            modifier |= Modifier::ITALIC;
                        }
                        if cell.underline() {
                            modifier |= Modifier::UNDERLINED;
                        }
                        if cell.inverse() {
                            modifier |= Modifier::REVERSED;
                        }
                        style = style.add_modifier(modifier);
                        target_cell.set_style(style);
                    } else {
                        target_cell.set_symbol(" ");
                        target_cell.set_style(Style::default());
                    }
                }
            }
        }

        if !screen.hide_cursor() {
            let (cursor_row, cursor_col) = screen.cursor_position();
            if cursor_row < height && cursor_col < width {
                Some((area.x + cursor_col, area.y + cursor_row))
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn convert_vt_color(color: VtColor) -> Color {
    match color {
        VtColor::Default => Color::Reset,
        VtColor::Idx(i) => Color::Indexed(i),
        VtColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn key_event_to_bytes(key: KeyEvent) -> Vec<u8> {
    let mut bytes = Vec::new();

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char(c) => {
                let ascii = c.to_ascii_lowercase() as u8;
                if ascii.is_ascii_lowercase() {
                    bytes.push(ascii - b'a' + 1);
                } else if c == '@' || c == ' ' {
                    bytes.push(0);
                } else if c == '[' {
                    bytes.push(0x1b);
                } else if c == '\\' {
                    bytes.push(0x1c);
                } else if c == ']' {
                    bytes.push(0x1d);
                } else if c == '^' {
                    bytes.push(0x1e);
                } else if c == '_' {
                    bytes.push(0x1f);
                }
            }
            KeyCode::Enter => bytes.push(b'\n'),
            _ => {}
        }
        return bytes;
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.push(0x1b);
    }

    match key.code {
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::BackTab => bytes.extend_from_slice(b"\x1b[Z"),
        KeyCode::Esc => bytes.push(0x1b),
        KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(1) => bytes.extend_from_slice(b"\x1bOP"),
        KeyCode::F(2) => bytes.extend_from_slice(b"\x1bOQ"),
        KeyCode::F(3) => bytes.extend_from_slice(b"\x1bOR"),
        KeyCode::F(4) => bytes.extend_from_slice(b"\x1bOS"),
        KeyCode::F(5) => bytes.extend_from_slice(b"\x1b[15~"),
        KeyCode::F(6) => bytes.extend_from_slice(b"\x1b[17~"),
        KeyCode::F(7) => bytes.extend_from_slice(b"\x1b[18~"),
        KeyCode::F(8) => bytes.extend_from_slice(b"\x1b[19~"),
        KeyCode::F(9) => bytes.extend_from_slice(b"\x1b[20~"),
        KeyCode::F(10) => bytes.extend_from_slice(b"\x1b[21~"),
        KeyCode::F(11) => bytes.extend_from_slice(b"\x1b[22~"),
        KeyCode::F(12) => bytes.extend_from_slice(b"\x1b[24~"),
        _ => {}
    }

    bytes
}
