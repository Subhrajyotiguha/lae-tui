use std::path::PathBuf;
use std::fs;

pub struct Playlist {
    pub paths: Vec<PathBuf>,
    pub current_index: usize,
}

impl Playlist {
    pub fn new() -> Self {
        Playlist {
            paths: Vec::new(),
            current_index: 0,
        }
    }

    pub fn add_dir(&mut self, dir_path: &str) {
        let mut entries = Vec::new();
        
        // Scan directory for FLAC files
        if let Ok(read_dir) = fs::read_dir(dir_path) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("flac")) {
                    entries.push(path);
                }
            }
        }

        // Sort by Last Modified Time (Newest to Oldest)
        entries.sort_by_key(|a| {
            std::cmp::Reverse(
                fs::metadata(a).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            )
        });

        self.paths.extend(entries);
    }
}