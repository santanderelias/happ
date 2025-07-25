use chrono::{DateTime, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::SystemTime;
use tokio::task;

#[derive(Debug, Clone)]
pub struct FileSystemItem {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: SystemTime,
    pub is_hidden: bool,
}

pub enum FileSystemEvent {
    ListDirectory(PathBuf),
    CreateFile(PathBuf),
    CreateFolder(PathBuf),
    DeleteItem(PathBuf),
    RenameItem(PathBuf, PathBuf),
    CopyItem(PathBuf, PathBuf),
    MoveItem(PathBuf, PathBuf),
    OpenFile(PathBuf),
    OpenTerminal(PathBuf),
    NewWindow,
}

pub async fn watch_directory(tx: Sender<Vec<FileSystemItem>>, rx: Receiver<FileSystemEvent>) {
    loop {
        match rx.try_recv() {
            Ok(event) => {
                let tx = tx.clone();
                task::spawn(async move {
                    match event {
                        FileSystemEvent::ListDirectory(path) => {
                            if let Ok(items) = list_directory(&path) {
                                tx.send(items).unwrap();
                            }
                        }
                        FileSystemEvent::CreateFile(path) => {
                            if fs::File::create(&path).is_ok() {
                                if let Some(parent) = path.parent() {
                                    if let Ok(items) = list_directory(parent) {
                                        tx.send(items).unwrap();
                                    }
                                }
                            }
                        }
                        FileSystemEvent::CreateFolder(path) => {
                            if fs::create_dir(&path).is_ok() {
                                if let Some(parent) = path.parent() {
                                    if let Ok(items) = list_directory(parent) {
                                        tx.send(items).unwrap();
                                    }
                                }
                            }
                        }
                        FileSystemEvent::DeleteItem(path) => {
                            let parent = path.parent().map(|p| p.to_path_buf());
                            if path.is_dir() {
                                let _ = fs::remove_dir_all(&path);
                            } else {
                                let _ = fs::remove_file(&path);
                            }
                            if let Some(parent) = parent {
                                if let Ok(items) = list_directory(&parent) {
                                    tx.send(items).unwrap();
                                }
                            }
                        }
                        FileSystemEvent::RenameItem(from, to) => {
                            if fs::rename(&from, &to).is_ok() {
                                if let Some(parent) = to.parent() {
                                    if let Ok(items) = list_directory(parent) {
                                        tx.send(items).unwrap();
                                    }
                                }
                            }
                        }
                        FileSystemEvent::CopyItem(from, to) => {
                            let parent = to.parent().map(|p| p.to_path_buf());
                            if from.is_dir() {
                                let mut options = fs_extra::dir::CopyOptions::new();
                                options.overwrite = true;
                                let _ = fs_extra::dir::copy(&from, &to.parent().unwrap(), &options);
                            } else {
                                let _ = fs::copy(&from, &to);
                            }
                            if let Some(parent) = parent {
                                if let Ok(items) = list_directory(&parent) {
                                    tx.send(items).unwrap();
                                }
                            }
                        }
                        FileSystemEvent::MoveItem(from, to) => {
                            let parent = to.parent().map(|p| p.to_path_buf());
                            if fs::rename(&from, &to).is_ok() {
                                if let Some(parent) = parent {
                                    if let Ok(items) = list_directory(&parent) {
                                        tx.send(items).unwrap();
                                    }
                                }
                            }
                        }
                        FileSystemEvent::OpenFile(path) => {
                            let _ = open::that(&path);
                        }
                        FileSystemEvent::OpenTerminal(path) => {
                            if cfg!(target_os = "windows") {
                                Command::new("cmd")
                                    .args(&["/C", "start"])
                                    .current_dir(&path)
                                    .spawn()
                                    .expect("failed to open terminal");
                            } else {
                                Command::new("gnome-terminal")
                                    .current_dir(&path)
                                    .spawn()
                                    .expect("failed to open terminal");
                            }
                        }
                        FileSystemEvent::NewWindow => {
                            let _ = Command::new(std::env::current_exe().unwrap()).spawn();
                        }
                    }
                });
            }
            Err(TryRecvError::Empty) => {
                // No event, continue
            }
            Err(TryRecvError::Disconnected) => {
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

fn list_directory(path: &Path) -> Result<Vec<FileSystemItem>, std::io::Error> {
    let mut items = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };
        let modified = metadata.modified()?;
        let is_hidden = path.file_name().unwrap().to_str().unwrap().starts_with('.');

        items.push(FileSystemItem {
            path,
            is_dir,
            size,
            modified,
            is_hidden,
        });
    }
    Ok(items)
}
