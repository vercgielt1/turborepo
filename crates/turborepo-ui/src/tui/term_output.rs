use std::io::Write;

use turborepo_vt100 as vt100;

use super::{
    app::Direction,
    event::{CacheResult, OutputLogs, TaskResult},
    Error,
};

pub struct TerminalOutput<W> {
    rows: u16,
    cols: u16,
    pub parser: vt100::Parser,
    pub stdin: Option<W>,
    pub status: Option<String>,
    pub output_logs: Option<OutputLogs>,
    pub task_result: Option<TaskResult>,
    pub cache_result: Option<CacheResult>,
    selection: Option<SelectionState>,
}

#[derive(Debug, Clone, Copy)]
enum LogBehavior {
    Full,
    Status,
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionState {
    start: Pos,
    end: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pos {
    pub x: u16,
    pub y: u16,
}

impl SelectionState {
    pub fn new(event: crossterm::event::MouseEvent) -> Self {
        let start = Pos {
            x: event.column,
            y: event.row,
        };
        let end = start;
        Self { start, end }
    }

    pub fn update(&mut self, event: crossterm::event::MouseEvent) {
        self.end = Pos {
            x: event.column,
            y: event.row,
        };
    }
}

impl<W> TerminalOutput<W> {
    pub fn new(rows: u16, cols: u16, stdin: Option<W>) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 1024),
            stdin,
            rows,
            cols,
            status: None,
            output_logs: None,
            task_result: None,
            cache_result: None,
            selection: None,
        }
    }

    pub fn title(&self, task_name: &str) -> String {
        match self.status.as_deref() {
            Some(status) => format!(" {task_name} > {status} "),
            None => format!(" {task_name} > "),
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.rows != rows || self.cols != cols {
            self.parser.screen_mut().set_size(rows, cols);
        }
        self.rows = rows;
        self.cols = cols;
    }

    pub fn scroll(&mut self, direction: Direction) -> Result<(), Error> {
        let scrollback = self.parser.screen().scrollback();
        let new_scrollback = match direction {
            Direction::Up => scrollback + 1,
            Direction::Down => scrollback.saturating_sub(1),
        };
        self.parser.screen_mut().set_scrollback(new_scrollback);
        Ok(())
    }

    fn persist_behavior(&self) -> LogBehavior {
        match self.output_logs.unwrap_or(OutputLogs::Full) {
            OutputLogs::Full => LogBehavior::Full,
            OutputLogs::None => LogBehavior::Nothing,
            OutputLogs::HashOnly => LogBehavior::Status,
            OutputLogs::NewOnly => {
                if matches!(self.cache_result, Some(super::event::CacheResult::Miss),) {
                    LogBehavior::Full
                } else {
                    LogBehavior::Status
                }
            }
            OutputLogs::ErrorsOnly => {
                if matches!(self.task_result, Some(TaskResult::Failure)) {
                    LogBehavior::Full
                } else {
                    LogBehavior::Nothing
                }
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub fn persist_screen(&self, task_name: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        let title = self.title(task_name);
        match self.persist_behavior() {
            LogBehavior::Full => {
                let screen = self.parser.entire_screen();
                stdout.write_all("┌".as_bytes())?;
                stdout.write_all(title.as_bytes())?;
                stdout.write_all(b"\r\n")?;
                for row in screen.rows_formatted(0, self.cols) {
                    stdout.write_all("│ ".as_bytes())?;
                    stdout.write_all(&row)?;
                    stdout.write_all(b"\r\n")?;
                }
                stdout.write_all("└────>\r\n".as_bytes())?;
            }
            LogBehavior::Status => {
                stdout.write_all(title.as_bytes())?;
                stdout.write_all(b"\r\n")?;
            }
            LogBehavior::Nothing => (),
        }
        Ok(())
    }

    pub fn handle_mouse(&mut self, event: crossterm::event::MouseEvent) -> Result<(), Error> {
        match event.kind {
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                // Here we enter copy mode with this position
                let selection = SelectionState::new(event);
                // we now store this in the task
                self.selection = Some(selection);
            }
            crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                // Here we change an endpoint of the selection
                // Should be noted that it can go backwards
                // If we didn't catch the start of a selection, use the current position
                let selection = self
                    .selection
                    .get_or_insert_with(|| SelectionState::new(event));
                selection.update(event);
                // Update selection of underlying parser
                self.parser.screen_mut().set_selection(
                    selection.start.y,
                    selection.start.x,
                    selection.end.y,
                    selection.end.x,
                );
            }
            // Scrolling is handled elsewhere
            crossterm::event::MouseEventKind::ScrollDown => (),
            crossterm::event::MouseEventKind::ScrollUp => (),
            // I think we can ignore this?
            crossterm::event::MouseEventKind::Moved => (),
            // Don't care about other mouse buttons
            crossterm::event::MouseEventKind::Down(_) => (),
            crossterm::event::MouseEventKind::Drag(_) => (),
            // We don't support horizontal scroll
            crossterm::event::MouseEventKind::ScrollLeft
            | crossterm::event::MouseEventKind::ScrollRight => (),
            // Cool, person stopped holding down mouse
            crossterm::event::MouseEventKind::Up(_) => (),
        }
        Ok(())
    }

    pub fn copy_selection(&self) -> Option<String> {
        self.parser.screen().selected_text()
    }
}
