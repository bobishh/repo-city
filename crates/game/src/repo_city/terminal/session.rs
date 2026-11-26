use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, Processor, Rgb},
};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};

pub(super) const DEFAULT_BACKGROUND: [u8; 3] = [13, 17, 23];

#[derive(Clone)]
struct Events(SyncSender<Event>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        let _ = self.0.try_send(event);
    }
}

#[derive(Clone, Copy)]
pub(super) struct Size {
    pub rows: u16,
    pub cols: u16,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

pub(super) struct Cell {
    pub text: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub underline: bool,
}
pub(super) struct Frame {
    pub size: Size,
    pub cells: Vec<Cell>,
    pub cursor: Option<(usize, usize)>,
}

pub(super) struct Session {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    input: Option<SyncSender<Vec<u8>>>,
    output: Receiver<Vec<u8>>,
    events: Receiver<Event>,
    term: Term<Events>,
    parser: Processor,
    background: [u8; 3],
    pub size: Size,
    pub exited: bool,
    pub title: String,
}

impl Session {
    pub fn open(root: &Path, shell: Option<&str>) -> Result<Self, String> {
        let program = shell.map(str::to_owned).unwrap_or_else(|| {
            if cfg!(windows) {
                "powershell.exe".into()
            } else {
                std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
            }
        });
        let mut command = CommandBuilder::new(&program);
        if cfg!(windows) {
            command.arg("-NoLogo");
        } else {
            command.arg("-i");
        }
        Self::open_program(root, command, &program, DEFAULT_BACKGROUND)
    }

    pub fn open_program(
        root: &Path,
        mut command: CommandBuilder,
        program: &str,
        background: [u8; 3],
    ) -> Result<Self, String> {
        let size = Size {
            rows: 30,
            cols: 100,
        };
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("Could not start {program}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let (input, requests) = mpsc::sync_channel::<Vec<u8>>(32);
        let (sender, output) = mpsc::sync_channel(64);
        thread::Builder::new()
            .name("vibepad-pty-read".into())
            .spawn(move || {
                let mut bytes = [0; 4096];
                loop {
                    match reader.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            if sender.send(bytes[..count].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        thread::Builder::new()
            .name("vibepad-pty-write".into())
            .spawn(move || {
                for bytes in requests {
                    if writer
                        .write_all(&bytes)
                        .and_then(|_| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let (event_sender, events) = mpsc::sync_channel(64);
        Ok(Self {
            master: pair.master,
            child: Some(child),
            input: Some(input),
            output,
            events,
            term: Term::new(
                Config {
                    scrolling_history: 5000,
                    ..Config::default()
                },
                &size,
                Events(event_sender),
            ),
            parser: Processor::new(),
            background,
            size,
            exited: false,
            title: program.into(),
        })
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.input
            .as_ref()
            .ok_or("The shell has exited.")?
            .try_send(bytes)
            .map_err(|_| "Terminal input is busy; try again.".into())
    }
    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        let text = text.replace("\x1b[201~", "");
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            self.send(format!("\x1b[200~{text}\x1b[201~").into_bytes())
        } else {
            self.send(text.replace('\n', "\r").into_bytes())
        }
    }
    pub fn application_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }
    pub fn resize(&mut self, size: Size) -> Result<(), String> {
        if size.rows == self.size.rows && size.cols == self.size.cols {
            return Ok(());
        }
        self.master
            .resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: size.cols * 12,
                pixel_height: size.rows * 24,
            })
            .map_err(|e| e.to_string())?;
        self.term.resize(size);
        self.size = size;
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..64 {
            match self.output.try_recv() {
                Ok(bytes) => {
                    self.parser.advance(&mut self.term, &bytes);
                    changed = true;
                }
                Err(_) => break,
            }
        }
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::PtyWrite(text) => {
                    let _ = self.send(text.into_bytes());
                }
                Event::Title(title) => {
                    self.title = title;
                    changed = true;
                }
                Event::TextAreaSizeRequest(format) => {
                    let _ = self.send(
                        format(WindowSize {
                            num_lines: self.size.rows,
                            num_cols: self.size.cols,
                            cell_width: 12,
                            cell_height: 24,
                        })
                        .into_bytes(),
                    );
                }
                Event::ColorRequest(index, format) => {
                    let [r, g, b] = palette(index, self.background);
                    let _ = self.send(format(Rgb { r, g, b }).into_bytes());
                }
                _ => {}
            }
        }
        if !self.exited
            && let Some(child) = self.child.as_mut()
            && child.try_wait().ok().flatten().is_some()
        {
            self.exited = true;
            self.input = None;
            changed = true;
        }
        changed
    }
    pub fn scroll(&mut self, rows: i32) {
        self.term.scroll_display(Scroll::Delta(rows));
    }
    pub fn bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }
    pub fn at_empty_prompt(&self) -> bool {
        let grid = self.term.grid();
        let row = &grid[grid.cursor.point.line];
        let text: String = (0..grid.columns())
            .map(|column| row[Column(column)].c)
            .collect();
        text.trim() == ">"
    }

    /// Screen rows at the bottom of the live terminal, independent of scrollback.
    pub fn screen_rows(&self) -> impl Iterator<Item = String> + '_ {
        let grid = self.term.grid();
        (0..grid.screen_lines() as i32).rev().map(|line| {
            let row = &grid[Line(line)];
            (0..grid.columns())
                .map(|column| row[Column(column)].c)
                .collect()
        })
    }
    pub fn frame(&self) -> Frame {
        let content = self.term.renderable_content();
        let color_at = |index| palette(index, self.background);
        let mut cells = (0..self.size.rows as usize * self.size.cols as usize)
            .map(|_| Cell {
                text: " ".into(),
                fg: color_at(256),
                bg: self.background,
                underline: false,
            })
            .collect::<Vec<_>>();
        let color = |color: Color| match color {
            Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
            Color::Indexed(index) => color_at(index as usize),
            Color::Named(name) => color_at(name as usize),
        };
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + content.display_offset as i32;
            let col = indexed.point.column.0;
            if row < 0 || row >= self.size.rows as i32 || col >= self.size.cols as usize {
                continue;
            }
            let cell = indexed.cell;
            let mut text = if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::HIDDEN)
            {
                String::new()
            } else {
                cell.c.to_string()
            };
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra);
            }
            let (fg, bg) = if cell.flags.contains(Flags::INVERSE) {
                (color(cell.bg), color(cell.fg))
            } else {
                (color(cell.fg), color(cell.bg))
            };
            cells[row as usize * self.size.cols as usize + col] = Cell {
                text,
                fg,
                bg,
                underline: cell.flags.contains(Flags::UNDERLINE),
            };
        }
        let point = content.cursor.point;
        let row = point.line.0 + content.display_offset as i32;
        let cursor = (content.cursor.shape != CursorShape::Hidden
            && row >= 0
            && row < self.size.rows as i32)
            .then_some((row as usize, point.column.0));
        Frame {
            size: self.size,
            cells,
            cursor,
        }
    }
    pub fn text(&self) -> String {
        self.frame()
            .cells
            .chunks(self.size.cols as usize)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.input = None;
        #[cfg(unix)]
        if let Some(group) = self
            .master
            .process_group_leader()
            .filter(|&group| group > 1)
        {
            // This process group belongs to the PTY created above, never to the app's terminal.
            unsafe {
                libc::kill(-group, libc::SIGHUP);
            }
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = thread::Builder::new()
                .name("vibepad-pty-reap".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

fn palette(index: usize, background: [u8; 3]) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [16, 22, 29],
        [248, 112, 124],
        [151, 216, 143],
        [238, 204, 128],
        [120, 171, 240],
        [198, 148, 232],
        [102, 207, 222],
        [217, 225, 233],
        [98, 111, 128],
        [255, 139, 149],
        [175, 235, 167],
        [255, 225, 158],
        [150, 196, 255],
        [224, 176, 255],
        [139, 234, 245],
        [245, 249, 255],
    ];
    match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = index - 16;
            let component = |v: usize| if v == 0 { 0 } else { 55 + v as u8 * 40 };
            [component(n / 36), component(n / 6 % 6), component(n % 6)]
        }
        232..=255 => [8 + (index - 232) as u8 * 10; 3],
        257 => background,
        _ => [225, 233, 241],
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn await_text(session: &mut Session, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            session.poll();
            if session.text().contains(marker) {
                break;
            }
            assert!(Instant::now() < deadline, "PTY did not produce marker");
            thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn persistent_pty_runs_in_the_checkout_resizes_and_interrupts_a_foreground_job() {
        let root = std::env::temp_dir();
        let mut session = Session::open(&root, Some("/bin/sh")).unwrap();
        session
            .send(b"printf '%s%s\\n' SESSION_ READY; pwd\r".to_vec())
            .unwrap();
        await_text(&mut session, "SESSION_READY");
        await_text(&mut session, root.canonicalize().unwrap().to_str().unwrap());
        session.resize(Size { rows: 20, cols: 90 }).unwrap();
        assert_eq!(session.master.get_size().unwrap().cols, 90);
        session.send(b"sleep 30\r".to_vec()).unwrap();
        thread::sleep(Duration::from_millis(100));
        session.send(vec![3]).unwrap();
        session
            .send(b"printf '%s%s\\n' AFTER_ INTERRUPT\r".to_vec())
            .unwrap();
        await_text(&mut session, "AFTER_INTERRUPT");
        assert!(!session.exited);
    }
}
