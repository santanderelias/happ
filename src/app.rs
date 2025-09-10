use crate::config::{self, AppConfig};
use crate::file_system::{self, FileSystemEvent, FileSystemItem};
use chrono::{DateTime, Local};
use eframe::egui::{self, Align, Key, Layout, Margin, Sense, TextEdit};
use egui_extras::{Column, TableBuilder};
use human_bytes::human_bytes;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Clone, Copy, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
    Modified,
}

#[derive(PartialEq)]
enum ClipboardAction {
    Copy,
    Cut,
}

struct ClipboardItem {
    action: ClipboardAction,
    path: PathBuf,
}

pub struct FileManager {
    items: Vec<FileSystemItem>,
    current_path: PathBuf,
    history: Vec<PathBuf>,
    history_index: usize,
    favorites: Vec<PathBuf>,
    status_message: String,
    rx: Receiver<Vec<FileSystemItem>>,
    event_tx: Sender<FileSystemEvent>,
    selected_items: HashSet<PathBuf>,
    show_hidden_files: bool,
    config: AppConfig,
    search_query: String,
    sort_by: SortBy,
    sort_ascending: bool,
    show_new_file_dialog: bool,
    new_file_name: String,
    show_new_folder_dialog: bool,
    new_folder_name: String,
    show_delete_confirmation: bool,
    item_to_delete: Option<PathBuf>,
    renaming_item: Option<PathBuf>,
    renaming_text: String,
    show_go_to_dialog: bool,
    go_to_path: String,
    show_properties_dialog: bool,
    properties_item: Option<FileSystemItem>,
    clipboard: Option<ClipboardItem>,
    context_menu_pos: Option<egui::Pos2>,
    context_menu_item: Option<FileSystemItem>,
    file_op_progress: f32,
    show_settings_dialog: bool,
    show_about_dialog: bool,
    drag_start_pos: Option<egui::Pos2>,
    drag_rect: Option<egui::Rect>,
    context_menu_rect: Option<egui::Rect>,
}

impl FileManager {
    pub fn new(rx: Receiver<Vec<FileSystemItem>>, event_tx: Sender<FileSystemEvent>) -> Self {
        let config = config::load_config().unwrap_or_default();
        let current_path =
            config.history.last().cloned().unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));

        let mut fm = Self {
            items: Vec::new(),
            current_path: PathBuf::new(),
            history: config.history.clone(),
            history_index: if config.history.is_empty() { 0 } else { config.history.len() - 1 },
            favorites: config.favorites.clone(),
            status_message: String::new(),
            rx,
            event_tx,
            selected_items: HashSet::new(),
            show_hidden_files: config.show_hidden_files,
            config,
            search_query: String::new(),
            sort_by: SortBy::Name,
            sort_ascending: true,
            show_new_file_dialog: false,
            new_file_name: String::new(),
            show_new_folder_dialog: false,
            new_folder_name: String::new(),
            show_delete_confirmation: false,
            item_to_delete: None,
            renaming_item: None,
            renaming_text: String::new(),
            show_go_to_dialog: false,
            go_to_path: String::new(),
            show_properties_dialog: false,
            properties_item: None,
            clipboard: None,
            context_menu_pos: None,
            context_menu_item: None,
            file_op_progress: 0.0,
            show_settings_dialog: false,
            show_about_dialog: false,
            drag_start_pos: None,
            drag_rect: None,
            context_menu_rect: None,
        };

        fm.navigate_to(&current_path.clone());
        fm
    }

    fn navigate_to(&mut self, path: &Path) {
        if path.is_dir() {
            self.current_path = path.to_path_buf();
            self.event_tx.send(FileSystemEvent::ListDirectory(self.current_path.clone())).unwrap();
            self.status_message = format!("Navigated to {}", self.current_path.display());
            self.selected_items.clear();
            self.search_query.clear();

            if self.history.last() != Some(&self.current_path) {
                if self.history_index < self.history.len() - 1 {
                    self.history.truncate(self.history_index + 1);
                }
                self.history.push(self.current_path.clone());
                self.history_index = self.history.len() - 1;
            }

            self.config.history = self.history.clone();
            config::save_config(&self.config).unwrap();
        }
    }

    fn go_back(&mut self) {
        if self.history_index > 0 {
            self.history_index -= 1;
            let path = self.history[self.history_index].clone();
            self.navigate_to(&path);
        } else if let Some(parent) = self.current_path.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(&parent);
        }
    }

    fn go_forward(&mut self) {
        if self.history_index < self.history.len() - 1 {
            self.history_index += 1;
            let path = self.history[self.history_index].clone();
            self.navigate_to(&path);
        }
    }

    fn refresh(&mut self) {
        self.event_tx.send(FileSystemEvent::ListDirectory(self.current_path.clone())).unwrap();
        self.status_message = "Refreshed".to_string();
    }

    fn create_file(&mut self) {
        if !self.new_file_name.is_empty() {
            let path = self.current_path.join(&self.new_file_name);
            self.event_tx.send(FileSystemEvent::CreateFile(path)).unwrap();
            self.show_new_file_dialog = false;
            self.new_file_name.clear();
        }
    }

    fn create_folder(&mut self) {
        if !self.new_folder_name.is_empty() {
            let path = self.current_path.join(&self.new_folder_name);
            self.event_tx.send(FileSystemEvent::CreateFolder(path)).unwrap();
            self.show_new_folder_dialog = false;
            self.new_folder_name.clear();
        }
    }

    fn delete_item(&mut self) {
        if let Some(path) = self.item_to_delete.take() {
            self.event_tx.send(FileSystemEvent::DeleteItem(path)).unwrap();
        }
        self.show_delete_confirmation = false;
    }

    fn rename_item(&mut self) {
        if let Some(path) = self.renaming_item.take() {
            let new_path = path.with_file_name(&self.renaming_text);
            self.event_tx.send(FileSystemEvent::RenameItem(path, new_path)).unwrap();
            self.renaming_text.clear();
        }
    }

    fn copy_selection(&mut self) {
        if let Some(item) = self.selected_items.iter().next() {
            self.clipboard = Some(ClipboardItem {
                action: ClipboardAction::Copy,
                path: item.clone(),
            });
            self.status_message = "Copied to clipboard".to_string();
        }
    }

    fn cut_selection(&mut self) {
        if let Some(item) = self.selected_items.iter().next() {
            self.clipboard = Some(ClipboardItem {
                action: ClipboardAction::Cut,
                path: item.clone(),
            });
            self.status_message = "Cut to clipboard".to_string();
        }
    }

    fn paste(&mut self) {
        if let Some(clipboard_item) = self.clipboard.take() {
            let dest_path = self.current_path.join(clipboard_item.path.file_name().unwrap());
            match clipboard_item.action {
                ClipboardAction::Copy => {
                    self.event_tx.send(FileSystemEvent::CopyItem(clipboard_item.path, dest_path)).unwrap();
                }
                ClipboardAction::Cut => {
                    self.event_tx.send(FileSystemEvent::MoveItem(clipboard_item.path, dest_path)).unwrap();
                }
            }
        }
    }

    fn open_item(&mut self, path: &Path) {
        if path.is_dir() {
            self.navigate_to(path);
        } else {
            self.event_tx.send(FileSystemEvent::OpenFile(path.to_path_buf())).unwrap();
        }
    }

    fn open_in_terminal(&mut self, path: &Path) {
        let terminal_path = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        self.event_tx.send(FileSystemEvent::OpenTerminal(terminal_path.to_path_buf())).unwrap();
    }

    fn is_dialog_open(&self) -> bool {
        self.show_new_file_dialog
            || self.show_new_folder_dialog
            || self.show_delete_confirmation
            || self.show_go_to_dialog
            || self.show_properties_dialog
            || self.show_settings_dialog
            || self.show_about_dialog
            || self.renaming_item.is_some()
    }

    fn handle_key_shortcuts(&mut self, ctx: &egui::Context) {
        if self.is_dialog_open() {
            return;
        }
        ctx.input(|i| {
            if i.key_pressed(Key::Backspace) {
                self.go_back();
            }
            if i.key_pressed(Key::Home) {
                if let Some(home_dir) = dirs::home_dir() {
                    self.navigate_to(&home_dir);
                }
            }
            if i.key_pressed(Key::F5) {
                self.refresh();
            }
            if i.key_pressed(Key::Delete) && !self.selected_items.is_empty() {
                self.item_to_delete = self.selected_items.iter().next().cloned();
                self.show_delete_confirmation = true;
            }
            if i.key_pressed(Key::F2) && self.selected_items.len() == 1 {
                if let Some(item) = self.selected_items.iter().next().cloned() {
                    self.renaming_item = Some(item.clone());
                    self.renaming_text = item.file_name().unwrap().to_str().unwrap().to_string();
                }
            }
            if i.key_pressed(Key::Enter) && self.selected_items.len() == 1 {
                if let Some(item) = self.selected_items.iter().next().cloned() {
                    self.open_item(&item);
                }
            }

            let ctrl = i.modifiers.ctrl;
            if ctrl && i.key_pressed(Key::H) {
                self.show_hidden_files = !self.show_hidden_files;
                self.config.show_hidden_files = self.show_hidden_files;
                config::save_config(&self.config).unwrap();
                self.refresh();
            }
            if ctrl && i.key_pressed(Key::N) {
                self.show_new_file_dialog = true;
            }
            if ctrl && i.modifiers.shift && i.key_pressed(Key::N) {
                self.show_new_folder_dialog = true;
            }
            if ctrl && i.key_pressed(Key::A) {
                self.selected_items = self.items.iter().map(|item| item.path.clone()).collect();
            }
            if ctrl && i.key_pressed(Key::G) {
                self.show_go_to_dialog = true;
                self.go_to_path = self.current_path.to_str().unwrap().to_string();
            }
            if ctrl && i.key_pressed(Key::C) {
                self.copy_selection();
            }
            if ctrl && i.key_pressed(Key::X) {
                self.cut_selection();
            }
            if ctrl && i.key_pressed(Key::V) {
                self.paste();
            }
        });
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Window").clicked() {
                        self.event_tx.send(FileSystemEvent::NewWindow).unwrap();
                        ui.close_menu();
                    }
                    ui.menu_button("New", |ui| {
                        if ui.button("File").clicked() {
                            self.show_new_file_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Folder").clicked() {
                            self.show_new_folder_dialog = true;
                            ui.close_menu();
                        }
                    });
                    if ui.button("Go To...").clicked() {
                        self.show_go_to_dialog = true;
                        self.go_to_path = self.current_path.to_str().unwrap().to_string();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        frame.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Copy").clicked() {
                        self.copy_selection();
                        ui.close_menu();
                    }
                    if ui.button("Cut").clicked() {
                        self.cut_selection();
                        ui.close_menu();
                    }
                    if ui.button("Paste").clicked() {
                        self.paste();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Select All").clicked() {
                        self.selected_items = self.items.iter().map(|item| item.path.clone()).collect();
                        ui.close_menu();
                    }
                    if ui.button("Select None").clicked() {
                        self.selected_items.clear();
                        ui.close_menu();
                    }
                    if ui.button("Invert Selection").clicked() {
                        let all_items: HashSet<_> = self.items.iter().map(|item| item.path.clone()).collect();
                        self.selected_items = all_items.difference(&self.selected_items).cloned().collect();
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.checkbox(&mut self.show_hidden_files, "Show Hidden Files").clicked() {
                        self.config.show_hidden_files = self.show_hidden_files;
                        config::save_config(&self.config).unwrap();
                        self.refresh();
                        ui.close_menu();
                    }
                    ui.menu_button("Sort By", |ui| {
                        if ui.radio_value(&mut self.sort_by, SortBy::Name, "Name").clicked() {
                            self.config.sort_by = self.sort_by;
                            config::save_config(&self.config).unwrap();
                            self.refresh();
                            ui.close_menu();
                        }
                        if ui.radio_value(&mut self.sort_by, SortBy::Size, "Size").clicked() {
                            self.config.sort_by = self.sort_by;
                            config::save_config(&self.config).unwrap();
                            self.refresh();
                            ui.close_menu();
                        }
                        if ui.radio_value(&mut self.sort_by, SortBy::Modified, "Modified").clicked() {
                            self.config.sort_by = self.sort_by;
                            config::save_config(&self.config).unwrap();
                            self.refresh();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Sort Order", |ui| {
                        if ui.radio_value(&mut self.sort_ascending, true, "Ascending").clicked() {
                            self.config.sort_ascending = self.sort_ascending;
                            config::save_config(&self.config).unwrap();
                            self.refresh();
                            ui.close_menu();
                        }
                        if ui.radio_value(&mut self.sort_ascending, false, "Descending").clicked() {
                            self.config.sort_ascending = self.sort_ascending;
                            config::save_config(&self.config).unwrap();
                            self.refresh();
                            ui.close_menu();
                        }
                    });
                });
                ui.menu_button("Go", |ui| {
                    if ui.button("Back").clicked() {
                        self.go_back();
                        ui.close_menu();
                    }
                    if ui.button("Forward").clicked() {
                        self.go_forward();
                        ui.close_menu();
                    }
                    if ui.button("Up").clicked() {
                        if let Some(parent) = self.current_path.parent().map(|p| p.to_path_buf()) {
                            self.navigate_to(&parent);
                        }
                        ui.close_menu();
                    }
                    if ui.button("Home").clicked() {
                        if let Some(home_dir) = dirs::home_dir() {
                            self.navigate_to(&home_dir);
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Refresh").clicked() {
                        self.refresh();
                        ui.close_menu();
                    }
                });
                ui.menu_button("History", |ui| {
                    let history = self.history.clone();
                    for path in history.iter().rev().take(10) {
                        if ui.button(path.display().to_string()).clicked() {
                            self.navigate_to(path);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Favorites", |ui| {
                    if ui.button("Add to Favorites").clicked() {
                        if !self.favorites.contains(&self.current_path) {
                            self.favorites.push(self.current_path.clone());
                            self.config.favorites = self.favorites.clone();
                            config::save_config(&self.config).unwrap();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    for fav in self.favorites.clone() {
                        let fav_name = fav.file_name().unwrap_or_default().to_str().unwrap_or_default();
                        if ui.button(fav_name).clicked() {
                            self.navigate_to(&fav);
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.show_about_dialog = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn draw_address_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⬅").clicked() {
                self.go_back();
            }
            if ui.button("➡").clicked() {
                self.go_forward();
            }
            if ui.button("⬆").clicked() {
                if let Some(parent) = self.current_path.parent().map(|p| p.to_path_buf()) {
                    self.navigate_to(&parent);
                }
            }

            let mut path_str = self.current_path.to_str().unwrap_or("").to_string();
            let response = ui.add(TextEdit::singleline(&mut path_str).desired_width(f32::INFINITY));
            if response.lost_focus() {
                ui.input(|i| {
                    if i.key_pressed(Key::Enter) {
                        self.navigate_to(&PathBuf::from(path_str));
                    }
                });
            }


            ui.add_space(10.0);
            let mut search_query = self.search_query.clone();
            if ui.add(TextEdit::singleline(&mut search_query).hint_text("Search...")).changed() {
                self.search_query = search_query;
            }
        });
    }

    fn draw_file_list(&mut self, ui: &mut egui::Ui) {
        let mut filtered_items = self.items.clone();
        if !self.search_query.is_empty() {
            filtered_items.retain(|item| {
                item.path
                    .file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&self.search_query.to_lowercase())
            });
        }
        if !self.show_hidden_files {
            filtered_items.retain(|item| !item.is_hidden);
        }

        match self.sort_by {
            SortBy::Name => filtered_items.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name())),
            SortBy::Size => filtered_items.sort_by_key(|a| a.size),
            SortBy::Modified => filtered_items.sort_by_key(|a| a.modified),
        }
        if !self.sort_ascending {
            filtered_items.reverse();
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let available_rect = ui.available_rect_before_wrap();
            let response = ui.interact(
                available_rect,
                egui::Id::new("file_list_background"),
                Sense::click_and_drag(),
            );

            if response.drag_started() {
                if !ui.ctx().input(|i| i.modifiers.ctrl) {
                    self.selected_items.clear();
                }
                self.drag_start_pos = response.hover_pos();
            }
            if response.dragged() {
                if let Some(start_pos) = self.drag_start_pos {
                    let current_pos = response.hover_pos().unwrap_or(start_pos);
                    self.drag_rect = Some(egui::Rect::from_two_pos(start_pos, current_pos));
                }
            }
            if response.drag_released() {
                self.drag_start_pos = None;
                self.drag_rect = None;
            }

            if response.clicked() {
                self.selected_items.clear();
            }
            if response.secondary_clicked() {
                self.context_menu_pos = Some(response.hover_pos().unwrap());
                self.context_menu_item = None;
            }

            if let Some(rect) = self.drag_rect {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::none(),
                    ui.style().visuals.selection.bg_fill.gamma_multiply(0.5),
                );
            }

            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(Column::initial(250.0).at_least(100.0))
                .column(Column::initial(80.0).at_least(40.0))
                .column(Column::initial(150.0).at_least(80.0))
                .min_scrolled_height(0.0);

            table
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Name");
                    });
                    header.col(|ui| {
                        ui.strong("Size");
                    });
                    header.col(|ui| {
                        ui.strong("Last Modified");
                    });
                })
                .body(|body| {
                    body.rows(18.0, filtered_items.len(), |row_index, mut row| {
                        let item = &filtered_items[row_index];
                        let is_selected = self.selected_items.contains(&item.path);

                        row.col(|ui| {
                            let icon = if item.is_dir { "📁" } else { "📄" };
                            let label = format!("{} {}", icon, item.path.file_name().unwrap().to_str().unwrap());
                            let response =
                                ui.add(egui::SelectableLabel::new(is_selected, label));

                            if let Some(drag_rect) = self.drag_rect {
                                if drag_rect.intersects(response.rect) {
                                    self.selected_items.insert(item.path.clone());
                                }
                            } else if response.clicked() {
                                if !ui.input(|i| i.modifiers.ctrl) {
                                    self.selected_items.clear();
                                }
                                if is_selected {
                                    self.selected_items.remove(&item.path);
                                } else {
                                    self.selected_items.insert(item.path.clone());
                                }
                            }
                            if response.double_clicked() {
                                self.open_item(&item.path.clone());
                            }
                            if response.secondary_clicked() {
                                self.context_menu_pos = Some(response.hover_pos().unwrap());
                                self.context_menu_item = Some(item.clone());
                            }

                            if let Some(renaming_path) = &self.renaming_item {
                                if renaming_path == &item.path {
                                    if ui.add(TextEdit::singleline(&mut self.renaming_text)).lost_focus() {
                                        self.rename_item();
                                    }
                                }
                            }
                        });

                        row.col(|ui| {
                            ui.label(if item.is_dir {
                                "".to_string()
                            } else {
                                human_bytes(item.size as f64)
                            });
                        });

                        row.col(|ui| {
                            let modified_time =
                                DateTime::<Local>::from(item.modified).format("%Y-%m-%d %H:%M:%S").to_string();
                            ui.label(modified_time);
                        });
                    });
                });
        });
    }

    fn draw_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(&self.status_message);
            if self.file_op_progress > 0.0 && self.file_op_progress < 1.0 {
                ui.add(egui::ProgressBar::new(self.file_op_progress).show_percentage());
            }
        });
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        if self.show_new_file_dialog {
            egui::Window::new("Create New File").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("File Name:");
                    ui.text_edit_singleline(&mut self.new_file_name);
                });
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.create_file();
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.show_new_file_dialog = false;
                        self.new_file_name.clear();
                    }
                });
            });
        }

        if self.show_new_folder_dialog {
            egui::Window::new("Create New Folder").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Folder Name:");
                    ui.text_edit_singleline(&mut self.new_folder_name);
                });
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.create_folder();
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.show_new_folder_dialog = false;
                        self.new_folder_name.clear();
                    }
                });
            });
        }

        if self.show_delete_confirmation {
            egui::Window::new("Confirm Deletion").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("Are you sure you want to delete the selected item(s)?");
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        self.delete_item();
                    }
                    if ui.button("No").clicked() {
                        self.show_delete_confirmation = false;
                        self.item_to_delete = None;
                    }
                });
            });
        }

        if self.show_go_to_dialog {
            egui::Window::new("Go To Path").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.text_edit_singleline(&mut self.go_to_path);
                ui.horizontal(|ui| {
                    if ui.button("Go").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        self.navigate_to(&PathBuf::from(&self.go_to_path));
                        self.show_go_to_dialog = false;
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        self.show_go_to_dialog = false;
                    }
                });
            });
        }

        if self.show_properties_dialog {
            if let Some(item) = &self.properties_item.clone() {
                egui::Window::new("Properties").collapsible(false).resizable(false).show(ctx, |ui| {
                    egui::Grid::new("properties_grid").show(ui, |ui| {
                        ui.label("Name:");
                        ui.label(item.path.file_name().unwrap().to_str().unwrap());
                        ui.end_row();
                        ui.label("Path:");
                        ui.label(item.path.to_str().unwrap());
                        ui.end_row();
                        ui.label("Type:");
                        ui.label(if item.is_dir { "Folder" } else { "File" });
                        ui.end_row();
                        if !item.is_dir {
                            ui.label("Size:");
                            ui.label(human_bytes(item.size as f64));
                            ui.end_row();
                        }
                        ui.label("Modified:");
                        let modified_time = DateTime::<Local>::from(item.modified).format("%Y-%m-%d %H:%M:%S").to_string();
                        ui.label(modified_time);
                        ui.end_row();
                    });
                    if ui.button("Close").clicked() {
                        self.show_properties_dialog = false;
                        self.properties_item = None;
                    }
                });
            }
        }

        if self.show_about_dialog {
            egui::Window::new("About").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("File Manager v0.1.0");
                ui.label("A simple file manager built with Rust and egui.");
                if ui.button("Close").clicked() {
                    self.show_about_dialog = false;
                }
            });
        }

        if self.show_settings_dialog {
            egui::Window::new("Settings").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.checkbox(&mut self.show_hidden_files, "Show Hidden Files");
                if ui.button("Reset Configuration").clicked() {
                    self.config = AppConfig::default();
                    config::save_config(&self.config).unwrap();
                }
                if ui.button("Close").clicked() {
                    self.show_settings_dialog = false;
                }
            });
        }
    }

    fn draw_context_menu(&mut self, ctx: &egui::Context) {
        if let Some(pos) = self.context_menu_pos {
            let area = egui::Area::new("context_menu").fixed_pos(pos);
            let area_response = area.show(ctx, |ui| {
                let frame = egui::Frame::menu(ui.style());
                frame.show(ui, |ui| {
                    if let Some(item) = &self.context_menu_item.clone() {
                        ui.label(item.path.file_name().unwrap().to_str().unwrap());
                        ui.separator();
                        if ui.button("Open").clicked() {
                            self.open_item(&item.path);
                            self.context_menu_pos = None;
                        }
                        if ui.button("Rename").clicked() {
                            self.renaming_item = Some(item.path.clone());
                            self.renaming_text =
                                item.path.file_name().unwrap().to_str().unwrap().to_string();
                            self.context_menu_pos = None;
                        }
                        if ui.button("Delete").clicked() {
                            self.item_to_delete = Some(item.path.clone());
                            self.show_delete_confirmation = true;
                            self.context_menu_pos = None;
                        }
                        if ui.button("Properties").clicked() {
                            self.properties_item = Some(item.clone());
                            self.show_properties_dialog = true;
                            self.context_menu_pos = None;
                        }
                        ui.separator();
                        if ui.button("Copy Path").clicked() {
                            ctx.output_mut(|o| o.copied_text = item.path.to_str().unwrap().to_string());
                            self.context_menu_pos = None;
                        }
                        if ui.button("Open in Terminal").clicked() {
                            self.open_in_terminal(&item.path);
                            self.context_menu_pos = None;
                        }
                    } else {
                        if ui.button("New File").clicked() {
                            self.show_new_file_dialog = true;
                            self.context_menu_pos = None;
                        }
                        if ui.button("New Folder").clicked() {
                            self.show_new_folder_dialog = true;
                            self.context_menu_pos = None;
                        }
                        ui.separator();
                        if ui.button("Paste").clicked() {
                            self.paste();
                            self.context_menu_pos = None;
                        }
                        ui.separator();
                        let current_path = self.current_path.clone();
                        if ui.button("Open in Terminal").clicked() {
                            self.open_in_terminal(&current_path);
                            self.context_menu_pos = None;
                        }
                    }
                });
            });
            self.context_menu_rect = Some(area_response.response.rect);
        } else {
            self.context_menu_rect = None;
        }
    }
}

impl eframe::App for FileManager {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Ok(items) = self.rx.try_recv() {
            self.items = items;
            self.status_message = format!("Listed {} items", self.items.len());
        }

        self.handle_key_shortcuts(ctx);
        self.draw_menu_bar(ctx, frame);

        egui::CentralPanel::default()
            .frame(egui::Frame {
                inner_margin: Margin::same(0.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                self.draw_address_bar(ui);
                ui.separator();
                self.draw_file_list(ui);
            });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            self.draw_status_bar(ui);
        });

        self.draw_dialogs(ctx);
        self.draw_context_menu(ctx);

        ctx.input(|i| {
            if i.pointer.any_click() {
                if let Some(menu_rect) = self.context_menu_rect {
                    if let Some(pos) = i.pointer.hover_pos() {
                        if !menu_rect.contains(pos) {
                            self.context_menu_pos = None;
                        }
                    }
                }
            }
        });

        // Request a repaint if there are ongoing operations
        if self.file_op_progress > 0.0 && self.file_op_progress < 1.0 {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        config::save_config(&self.config).unwrap();
    }
}
