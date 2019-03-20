use termion::event::{Event, Key};
use unicode_width::UnicodeWidthStr;

use std::path::{Path, PathBuf};

use crate::files::{File, Files};
use crate::fail::{HResult, ErrorLog};
use crate::term;
use crate::widget::{Widget, WidgetCore};
use crate::dirty::Dirtyable;

pub trait Listable {
    fn len(&self) -> usize;
    fn render(&self) -> Vec<String>;
    fn render_header(&self) -> HResult<String> { Ok("".to_string()) }
    fn render_footer(&self) -> HResult<String> { Ok("".to_string()) }
    fn on_refresh(&mut self) -> HResult<()> { Ok(()) }
    fn on_key(&mut self, _key: Key) -> HResult<()> { Ok(()) }
}

impl Listable for ListView<Files> {
    fn len(&self) -> usize {
        self.content.len()
    }

    fn render(&self)-> Vec<String> {
        self.render()
    }

    fn on_refresh(&mut self) -> HResult<()> {
        let visible_file_num = self.selection + self.get_coordinates()?.ysize() as usize;
        self.content.meta_upto(visible_file_num);

        if self.content.is_dirty() {
            self.core.set_dirty();
            self.content.set_clean();
        }
        Ok(())
    }

    fn on_key(&mut self, key: Key) -> HResult<()> {
        match key {
            Key::Up | Key::Char('p') => {
                self.move_up();
                self.refresh()?;
            }
            Key::Char('P') => { for _ in 0..10 { self.move_up() } self.refresh()?; }
            Key::Char('N') => { for _ in 0..10 { self.move_down() } self.refresh()?; }
            Key::Down | Key::Char('n') => {
                self.move_down();
                self.refresh()?;
            },
            Key::Ctrl('s') => { self.find_file().ok(); }
            Key::Char('F') => { self.filter().log(); }
            Key::Left => self.goto_grand_parent()?,
            Key::Right => self.goto_selected()?,
            Key::Char(' ') => self.multi_select_file(),
            Key::Char('t') => self.toggle_tag()?,
            Key::Char('h') => self.toggle_hidden(),
            Key::Char('r') => self.reverse_sort(),
            Key::Char('s') => self.cycle_sort(),
            Key::Char('K') => self.select_next_mtime(),
            Key::Char('k') => self.select_prev_mtime(),
            Key::Char('d') => self.toggle_dirs_first(),
            _ => { self.bad(Event::Key(key))?; }
        }
        Ok(())
    }
}

#[derive(PartialEq)]
pub struct ListView<T> where ListView<T>: Listable
{
    pub content: T,
    pub lines: usize,
    selection: usize,
    pub offset: usize,
    pub buffer: Vec<String>,
    pub core: WidgetCore,
    seeking: bool,
}

impl<T> ListView<T>
where
    ListView<T>: Widget,
    ListView<T>: Listable
{
    pub fn new(core: &WidgetCore, content: T) -> ListView<T> {
        let view = ListView::<T> {
            content: content,
            lines: 0,
            selection: 0,
            offset: 0,
            buffer: Vec::new(),
            core: core.clone(),
            seeking: false
        };
        view
    }

    pub fn move_up(&mut self) {
        if self.selection == 0 {
            return;
        }

        if self.selection - self.offset <= 0 {
            self.offset -= 1;
        }

        self.selection -= 1;
        self.seeking = false;
    }
    pub fn move_down(&mut self) {
        let lines = self.lines;
        let y_size = self.get_coordinates().unwrap().ysize() as usize;

        if self.lines == 0 || self.selection == lines - 1 {
            return;
        }

        if self.selection + 1 >= y_size && self.selection + 1 - self.offset >= y_size {
            self.offset += 1;
        }

        self.selection += 1;
        self.seeking = false;
    }

    pub fn get_selection(&self) -> usize {
        self.selection
    }

    pub fn set_selection(&mut self, position: usize) {
        let ysize = self.get_coordinates().unwrap().ysize() as usize;
        let mut offset = 0;

        while position + 2
            >= ysize + offset {
            offset += 1
        }

        self.offset = offset;
        self.selection = position;
    }

}

impl ListView<Files>
{
    pub fn selected_file(&self) -> &File {
        let selection = self.selection;
        let file = &self.content[selection];
        file
    }

    pub fn selected_file_mut(&mut self) -> &mut File {
        let selection = self.selection;
        let file = &mut self.content.files[selection];
        file
    }

    pub fn clone_selected_file(&self) -> File {
        let selection = self.selection;
        let file = self.content[selection].clone();
        file
    }

    pub fn grand_parent(&self) -> Option<PathBuf> {
        self.selected_file().grand_parent()
    }

    pub fn goto_grand_parent(&mut self) -> HResult<()> {
        match self.grand_parent() {
            Some(grand_parent) => self.goto_path(&grand_parent),
            None => { self.show_status("Can't go further!") },
        }
    }

    fn goto_selected(&mut self) -> HResult<()> {
        let path = self.selected_file().path();

        self.goto_path(&path)
    }

    pub fn goto_path(&mut self, path: &Path) -> HResult<()> {
        match crate::files::Files::new_from_path(path) {
            Ok(files) => {
                self.content = files;
                self.selection = 0;
                self.offset = 0;
                self.refresh()
            }
            Err(err) => {
                self.show_status(&format!("Can't open this path: {}", err))
            }
        }
    }

    pub fn select_file(&mut self, file: &File) {
        let pos = self
            .content
            .files
            .iter()
            .position(|item| item == file)
            .unwrap_or(0);
        self.set_selection(pos);
    }

    fn cycle_sort(&mut self) {
        let file = self.clone_selected_file();
        self.content.cycle_sort();
        self.content.sort();
        self.select_file(&file);
        self.refresh().log();
        self.show_status(&format!("Sorting by: {}", self.content.sort)).log();
    }

    fn reverse_sort(&mut self) {
        let file = self.clone_selected_file();
        self.content.reverse_sort();
        self.content.sort();
        self.select_file(&file);
        self.refresh().log();
        self.show_status(&format!("Reversed sorting by: {}", self.content.sort)).log();
    }

    fn select_next_mtime(&mut self) {
        let file = self.clone_selected_file();
        let dir_settings = self.content.dirs_first;
        let sort_settings = self.content.sort;

        self.content.dirs_first = false;
        self.content.sort = crate::files::SortBy::MTime;
        self.content.sort();

        self.select_file(&file);

        if self.seeking == false || self.selection + 1 == self.content.len() {
            self.selection = 0;
            self.offset = 0;
        } else {
            self.move_down();
         }

        let file = self.clone_selected_file();
        self.content.dirs_first = dir_settings;
        self.content.sort = sort_settings;
        self.content.sort();
        self.select_file(&file);
        self.seeking = true;

        self.refresh().log();
    }

    fn select_prev_mtime(&mut self) {
        let file = self.clone_selected_file();
        let dir_settings = self.content.dirs_first;
        let sort_settings = self.content.sort;

        self.content.dirs_first = false;
        self.content.sort = crate::files::SortBy::MTime;
        self.content.sort();

        self.select_file(&file);

        if self.seeking == false || self.selection == 0 {
            self.set_selection(self.content.len() - 1);
        } else {
            self.move_up();
        }

        let file = self.clone_selected_file();
        self.content.dirs_first = dir_settings;
        self.content.sort = sort_settings;
        self.content.sort();
        self.select_file(&file);
        self.seeking = true;

        self.refresh().log();
    }

    fn toggle_hidden(&mut self) {
        let file = self.clone_selected_file();
        self.content.toggle_hidden();
        self.content.reload_files();
        self.select_file(&file);
        self.refresh().log();
    }

    fn toggle_dirs_first(&mut self) {
        let file = self.clone_selected_file();
        self.content.dirs_first = !self.content.dirs_first;
        self.content.sort();
        self.select_file(&file);
        self.refresh().log();
        self.show_status(&format!("Direcories first: {}",
                                  self.content.dirs_first)).log();
    }

    fn multi_select_file(&mut self) {
        self.selected_file_mut().toggle_selection();

        let selection = self.get_selection();
        let line = self.render_line(self.selected_file());
        self.buffer[selection] = line;

        self.move_down();
    }

    fn toggle_tag(&mut self) -> HResult<()> {
        self.selected_file_mut().toggle_tag()?;

        let selection = self.get_selection();
        let line = self.render_line(self.selected_file());
        self.buffer[selection] = line;

        self.move_down();
        Ok(())
    }

    fn find_file(&mut self) -> HResult<()> {
        let name = self.minibuffer("find")?;
        let file = self.content.files.iter().find(|file| {
            if file.name.to_lowercase().contains(&name) {
                true
            } else {
                false
            }
        })?.clone();

        self.select_file(&file);
        Ok(())
    }

    fn filter(&mut self) -> HResult<()> {
        let filter = self.minibuffer("filter").ok();
        self.content.set_filter(filter);


        if self.get_selection() > self.len() {
            self.set_selection(self.len());
        }
        Ok(())
    }

    fn render_line(&self, file: &File) -> String {
        let name = &file.name;
        let (size, unit) = file.calculate_size().unwrap_or((0, "".to_string()));
        let tag = match file.is_tagged() {
            Ok(true) => term::color_red() + "*",
            _ => "".to_string()
        };
        let tag_len = if tag != "" { 1 } else { 0 };

        let selection_gap = "  ".to_string();
        let (name, selection_color) =  if file.is_selected() {
            (selection_gap + name, crate::term::color_yellow())
        } else { (name.clone(), "".to_string()) };

        let (link_indicator, link_indicator_len) = if file.target.is_some() {
            (format!("{}{}{}",
                     term::color_yellow(),
                     "--> ".to_string(),
                     term::highlight_color()),
             4)
        } else { ("".to_string(), 0) };

        let xsize = self.get_coordinates().unwrap().xsize();
        let sized_string = term::sized_string(&name, xsize);
        let size_pos = xsize - (size.to_string().len() as u16
                                + unit.to_string().len() as u16
                                + link_indicator_len);
        let padding = sized_string.len() - sized_string.width_cjk();
        let padding = xsize - padding as u16;
        let padding = padding - tag_len;

        format!(
            "{}{}{}{}{}{}{}{}",
            termion::cursor::Save,
            match &file.color {
                Some(color) => format!("{}{}{}{:padding$}{}",
                                       tag,
                                       term::from_lscolor(color),
                                       selection_color,
                                       &sized_string,
                                       term::normal_color(),
                                       padding = padding as usize),
                _ => format!("{}{}{}{:padding$}{}",
                             tag,
                             term::normal_color(),
                             selection_color,
                             &sized_string,
                             term::normal_color(),
                             padding = padding as usize),
            } ,
            termion::cursor::Restore,
            termion::cursor::Right(size_pos),
            link_indicator,
            term::highlight_color(),
            size,
            unit
        )
    }

    fn render(&self) -> Vec<String> {
        match self.content.get_filter() {
            Some(filter) => self.content
                .files
                .iter()
                .filter(|f| f.name.contains(&filter))
                .map(|file| self.render_line(&file))
                .collect(),
            None => self.content
                .files
                .iter()
                .map(|file| self.render_line(&file))
                .collect()
        }
    }
}


impl<T> Widget for ListView<T> where ListView<T>: Listable {
    fn get_core(&self) -> HResult<&WidgetCore> {
        Ok(&self.core)
    }
    fn get_core_mut(&mut self) -> HResult<&mut WidgetCore> {
        Ok(&mut self.core)
    }
    fn refresh(&mut self) -> HResult<()> {
        self.on_refresh().log();
        self.lines = self.len();

        if self.selection >= self.lines && self.selection != 0 {
            self.selection -= 1;
        }

        if self.core.is_dirty() || self.buffer.len() != self.len() {
            self.buffer = self.render();
            self.core.set_clean();
        }
        Ok(())
    }

    fn render_header(&self) -> HResult<String> {
        Listable::render_header(self)
    }

    fn render_footer(&self) -> HResult<String> {
        Listable::render_footer(self)
    }

    fn get_drawlist(&self) -> HResult<String> {
        let mut output = term::reset();
        let (xpos, ypos) = self.get_coordinates().unwrap().position().position();
        let ysize = self.get_coordinates().unwrap().ysize() as usize;

        output += &self
            .buffer
            .iter()
            .skip(self.offset)
            .take(ysize)
            .enumerate()
            .map(|(i, item)| {
                let mut output = term::normal_color();

                if i == (self.selection - self.offset) {
                    output += &term::invert();
                }
                output += &format!(
                    "{}{}{}",
                    term::goto_xy(xpos, i as u16 + ypos),
                    item,
                    term::reset()
                );
                String::from(output)
            })
            .collect::<String>();

        output += &self.get_redraw_empty_list(self.buffer.len())?;

        Ok(output)
    }

    fn on_key(&mut self, key: Key) -> HResult<()> {
        Listable::on_key(self, key)
    }
}
