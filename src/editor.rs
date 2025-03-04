use crate::buffer::{Buffer, BufferType, Mark};
use crate::display::Display;
use crate::editor::EditorMode::{Normal, SaveMode};
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::event::Event::Key;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, DisableLineWrap, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, event, execute, ExecutableCommand};
use std::cmp::PartialEq;
use std::fs::OpenOptions;
use std::io::{Error, Read, Write};
use std::time::Duration;
use log::{error, info};

const TAB_SIZE: u16 = 4;

#[derive(Debug)]
pub struct Editor {
    pub display: Display,
    pub exit: bool,
    pub current_buffer: usize,
    pub previous_buffer: usize,
    pub buffer_list: Vec<Buffer>,
    pub mode: EditorMode
}

#[derive(PartialEq)]
pub enum CursorMovement {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, PartialEq)]
pub enum EditorMode {
    Normal,
    SaveMode
}

impl Editor {
    pub fn default() -> Self {
        let option_buffer = Self::init_option_buffer();
        Self {
            display: Display::default(),
            exit: false,
            previous_buffer: 0,
            current_buffer: 1,
            buffer_list: vec! [option_buffer, Buffer::default()],
            mode: Normal,
        }
    }

    pub fn init(&mut self, file_path: Option<String>) ->Result<(), Error> {
        if let Some(file) = file_path.as_ref() {
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(file)?;

            let mut content = String::new();
            file.read_to_string(&mut content)?;

            if let Some(buffer) = self.buffer_list.get_mut(self.current_buffer) {
                buffer.content = content;
                buffer.file_name = file_path.clone();
                info!("File {:?} is loaded or created", buffer.file_name);
            } else {
                error!("Invalid buffer index: {}", self.current_buffer);
            }
        }
        Ok(())
    }

    pub fn init_option_buffer() -> Buffer {
        Buffer {
            content: String::new(),
            point: Mark::new(String::from("Point"), 0),
            mark_list: vec![],
            file_name: None,
            buffer_type: BufferType::OPTION,
        }
    }

    pub fn run(&mut self) -> Result<(), std::io::Error> {
        self.display.stdout.execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        self.display.stdout.execute(DisableLineWrap)?;
        self.display_current_buffer()?;
        self.display.stdout.execute(MoveTo(0, 0))?;
        self.handle_key_events()?;
        disable_raw_mode()?;
        self.display.stdout.execute(LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn handle_key_events(&mut self) -> Result<(), Error> {
        loop {
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Resize(width, height) => {
                        self.handle_resizing(width, height)?;
                    }
                    Key(KeyEvent { code, modifiers, .. }) => {
                        match code {
                            KeyCode::Char('q') if modifiers.contains(KeyModifiers::CONTROL) => {
                                self.exit = true;
                            },
                            KeyCode::Char('x') if modifiers.contains(KeyModifiers::CONTROL) && self.mode == Normal => {
                                self.handle_save_mode_input()?;
                            }
                            KeyCode::Char(c) if modifiers.is_empty() || modifiers ==KeyModifiers::SHIFT => {
                                self.handle_char_input(c)?;
                            }
                            KeyCode::Right => self.handle_cursor_movement(CursorMovement::Right)?,
                            KeyCode::Left => self.handle_cursor_movement(CursorMovement::Left)?,
                            KeyCode::Up => self.handle_cursor_movement(CursorMovement::Up)?,
                            KeyCode::Down => self.handle_cursor_movement(CursorMovement::Down)?,
                            KeyCode::Backspace => self.handle_backspace_input()?,
                            KeyCode::Enter => self.handle_enter_input()?,
                            KeyCode::Tab => self.handle_tab_input()?,
                            _ => (),
                        }
                    }
                    _ => (),
                }
            }
            if self.exit {
                break;
            }
        }
        Ok(())
    }

    fn handle_resizing(&mut self, width: u16, height: u16) -> Result<(), Error> {
        self.display.height = height;
        self.display.width = width;
        if let Some((row, col)) = self.buffer_list[self.current_buffer].get_point_line_and_column() {
            self.display.clear_and_print(self.buffer_list[self.current_buffer].content.clone())?;
            execute!(self.display.stdout, MoveTo(col, row))?;
        }
        Ok(())
    }

    pub fn handle_cursor_movement(&mut self, movement: CursorMovement) -> Result<(), Error> {
        let (col, row) = cursor::position()?;
        match movement {
            CursorMovement::Up => {
                self.handle_cursor_up(col, row)?;
            }
            CursorMovement::Down => {
                self.handle_cursor_down(col, row)?;
            }
            CursorMovement::Left => {
                self.handle_cursor_left(col, row)?;
            }
            CursorMovement::Right => {
                self.handle_cursor_right(col, row)?;
            }
        }
        Ok(())
    }

    fn handle_cursor_right(&mut self, col: u16, row: u16) -> Result<(), Error> {
        if let Some((new_row, new_col)) = self.get_cursor_valid_position(
            row + self.display.first_line_visible,
            col + 1,
            CursorMovement::Right
        ) {
            self.buffer_list[self.current_buffer].move_point_to(new_row, new_col);
            if new_row - self.display.first_line_visible >= self.display.height {
                self.display.first_line_visible = self.display.first_line_visible + 1;
            }
            self.display_current_buffer()?;
            self.display.stdout.execute(MoveTo(new_col, new_row - self.display.first_line_visible))?;
        }
        Ok(())
    }

    fn handle_cursor_left(&mut self, col: u16, row: u16) -> Result<(), Error> {
        if col >= 1 {
            if let Some((new_row, new_col)) = self.get_cursor_valid_position(
                row + self.display.first_line_visible,
                col - 1,
                CursorMovement::Left
            ) {
                self.buffer_list[self.current_buffer].move_point_to(new_row, new_col);
                self.display.stdout.execute(MoveTo(new_col, new_row - self.display.first_line_visible))?;
            }
        }
        Ok(())
    }

    fn handle_cursor_down(&mut self, col: u16, row: u16) -> Result<(), Error> {
        if let Some((new_row, new_col)) = self.get_cursor_valid_position(
            row + self.display.first_line_visible + 1,
            col,
            CursorMovement::Down
        ) {
            self.buffer_list[self.current_buffer].move_point_to(new_row, new_col);
            if new_row - self.display.first_line_visible >= self.display.height {
                self.display.first_line_visible = self.display.first_line_visible + 1;
            }
            self.display_current_buffer()?;
            self.display.stdout.execute(MoveTo(new_col, new_row - self.display.first_line_visible))?;
        }
        Ok(())
    }

    fn handle_cursor_up(&mut self, col: u16, row: u16) -> Result<(), Error> {
        if row >= 1 || self.display.first_line_visible != 0 {
            if let Some((new_row, new_col)) = self.get_cursor_valid_position(
                (self.display.first_line_visible + row) - 1,
                col,
                CursorMovement::Up
            ) {
                if new_row < self.display.first_line_visible {
                    self.display.first_line_visible = self.display.first_line_visible - 1;
                }
                self.buffer_list[self.current_buffer].move_point_to(new_row, new_col);
                self.display_current_buffer()?;
                self.display.stdout.execute(MoveTo(new_col, new_row - self.display.first_line_visible))?;
            }
        }
        Ok(())
    }

    pub fn get_cursor_valid_position(&self, row: u16, col: u16, movement: CursorMovement) -> Option<(u16, u16)> {
        let occupied_positions: Vec<Option<u16>> = self.buffer_list[self.current_buffer].get_last_visible_char_position();
        if occupied_positions.is_empty() {
            return Some((row, col))
        }

        if row >= occupied_positions.len() as u16 {
            return None;
        }

        match occupied_positions.get(row as usize) {
            Some(Some(occupied)) => {
                if col <= occupied + 1 {
                    Some((row, col))
                } else {
                    match movement {
                        CursorMovement::Up => {
                            Some((row, *occupied))
                        },
                        CursorMovement::Down => {
                            Some((row, *occupied))
                        },
                        CursorMovement::Left => {
                            if row > 0 {
                                let last_position = occupied_positions[(row - 1) as usize];
                                if let Some(last_position) = last_position {
                                    Some((row - 1, last_position))
                                } else {
                                    Some((row - 1, 0))
                                }
                            } else {
                                None
                            }
                        },
                        CursorMovement::Right => {
                            if (row + 1) < occupied_positions.len() as u16 {
                                Some((row + 1, 0))
                            } else {
                                None
                            }
                        }
                    }
                }
            },
            Some(None) => {
                if (row + 1) < occupied_positions.len() as u16 && movement == CursorMovement::Right {
                    Some((row + 1, 0))
                } else {
                    Some((row, 0))
                }
            },
            None => None,
        }
    }

    pub fn is_cursor_position_valid(&self, row: u16, col: u16) -> bool {
        let occupied_positions: Vec<Option<u16>> = self.buffer_list[self.current_buffer].get_last_visible_char_position();

        if occupied_positions.is_empty() {
            return true;
        }

        if row >= occupied_positions.len() as u16 {
            return false;
        }

        match occupied_positions.get(row as usize) {
            Some(Some(occupied)) => col <= occupied + 1,
            Some(None) => col == 0,
            None => false,
        }
    }

    pub fn handle_char_input(&mut self, c: char) -> Result<(), Error> {
        self.buffer_list[self.current_buffer].write_char(c)?;
        let (col, row) = cursor::position()?;
        self.display_current_buffer()?;
        self.buffer_list[self.current_buffer].move_point_to(row + self.display.first_line_visible, col + 1);
        self.display.stdout.execute(MoveTo(col + 1, row))?;
        Ok(())
    }

    pub fn handle_enter_input(&mut self) -> Result<(), Error> {
        if self.mode == Normal {
            let (_, row) = cursor::position()?;
            self.buffer_list[self.current_buffer].write_char('\n')?;
            if row + 1 == self.display.height {
                self.display.first_line_visible = self.display.first_line_visible + 1;
            }
            self.buffer_list[self.current_buffer].move_point_to(self.display.first_line_visible + row + 1, 0);
            self.display_current_buffer()?;
            self.display.stdout.execute(MoveTo(0, row + 1))?;
        } else if self.mode == SaveMode {
            self.buffer_list[self.previous_buffer].file_name = Some(self.buffer_list[0].content.clone());
            self.current_buffer = self.previous_buffer;
            self.previous_buffer = 0;
            self.handle_save_file()?;
        }
        Ok(())
    }

    pub fn handle_backspace_input(&mut self) -> Result<(), Error> {
        let (col, row) = cursor::position()?;
        let first_visible_row = self.display.first_line_visible;
        if row > 0 && col == 0 { // remove last character from previous line
            let new_row = row - 1;
            let new_col = self.buffer_list[self.current_buffer].get_last_column(new_row);
            self.buffer_list[self.current_buffer].move_point_to(new_row + first_visible_row, new_col);
            self.buffer_list[self.current_buffer].remove_char()?;
            self.display_current_buffer()?;
            self.display.stdout.execute(MoveTo(new_col - 1, new_row))?;
        } else if col > 0 {
            self.buffer_list[self.current_buffer].move_point_to(row + first_visible_row, col - 1);
            self.buffer_list[self.current_buffer].remove_char()?;
            self.display_current_buffer()?;
            self.display.stdout.execute(MoveTo(col -1, row))?;
        }
        Ok(())
    }

    pub fn handle_tab_input(&mut self) -> Result<(), Error> {
        let (col, row) = cursor::position()?;
        for _i in 0..TAB_SIZE {
            self.buffer_list[self.current_buffer].write_char(' ')?
        }
        self.display_current_buffer()?;
        self.buffer_list[self.current_buffer].move_point_to(row + self.display.first_line_visible, col + TAB_SIZE);
        self.display.stdout.execute(MoveTo(col + TAB_SIZE, row))?;
        Ok(())
    }

    pub fn display_current_buffer(&mut self) -> Result<(), Error> {
        let (start, end) = self.display.get_displayable_lines()?;
        let part = self.buffer_list[self.current_buffer].get_buffer_part(start, end)?;
        self.display.clear_and_print(part)?;
        Ok(())
    }

    pub fn get_buffer_row(cursor_row: u16, visible_row: u16) -> u16 {
        cursor_row + visible_row
    }

    pub fn handle_save_mode_input(&mut self) -> Result<(), Error> {
        execute!(self.display.stdout, SavePosition)?;
        self.display.print_save_validation()?;

        loop {
            match event::read()? {
                Key(KeyEvent { code, .. }) if matches!(code, KeyCode::Char('Y') | KeyCode::Char('y')) => {
                    return self.handle_save_file();
                }
                Key(KeyEvent { code, .. }) if matches!(code, KeyCode::Char('N') | KeyCode::Char('n')) => {
                    self.handle_cancel_save()?;
                    execute!(self.display.stdout, RestorePosition)?;
                    return Ok(());
                }
                _ => continue,
            }
        }
    }


    pub fn handle_save_file(&mut self) -> Result<(), Error> {
        self.display.clear_all_display()?;
        if self.current_buffer != 0 {
            if let Some(filename) = self.buffer_list[self.current_buffer].file_name.clone() {
                let mut file  = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(filename)?;
                file.write_all(self.buffer_list[self.current_buffer].content.clone().as_bytes())?;
                self.mode = Normal;
                self.display_current_buffer()?;
                execute!(self.display.stdout, RestorePosition)?;
            } else {
                self.previous_buffer = self.current_buffer;
                self.current_buffer = 0;
                self.mode = SaveMode;
                self.display.print_filename_input()?;
                execute!(self.display.stdout, MoveTo(0, 0))?;
            }
        }
        Ok(())
    }

    pub fn handle_cancel_save(&mut self) -> Result<(), Error> {
        self.display_current_buffer()?;
        Ok(())
    }
}