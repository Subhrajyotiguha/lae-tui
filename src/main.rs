mod ffi;
mod playlist;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{poll, read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType, DisableLineWrap, EnableLineWrap},
};
use ffi::*;
use playlist::Playlist;
use std::ffi::CString;
use std::io::{stdout, Write};
use std::time::Duration;
use base64::{engine::general_purpose, Engine as _};
use rand::Rng; 
use std::fs;
use std::path::{Path, PathBuf};
use serde_json::Value;
use std::cmp::Reverse;

struct LyricLine {
    time: f64,
    text: String,
}

fn populate_dir_menu(path: &Path) -> Vec<String> {
    let mut items = vec!["[LOAD ALL FILES HERE]".to_string(), ".. (Go Up)".to_string()];
    if let Ok(entries) = fs::read_dir(path) {
        let mut folders = Vec::new();
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    folders.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        folders.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        items.extend(folders);
    }
    items
}

fn load_or_fetch_lyrics(flac_path: &str, title: &str, artist: &str, album: &str, duration_sec: f64) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    let lrc_path = flac_path.replace(".flac", ".lrc").replace(".FLAC", ".lrc");
    
    if Path::new(&lrc_path).exists() {
        if let Ok(content) = fs::read_to_string(&lrc_path) { return parse_lrc_content(&content); }
    }

    if title.is_empty() || artist.is_empty() { return lines; }

    let safe_title = urlencoding::encode(title);
    let safe_artist = urlencoding::encode(artist);
    let safe_album = urlencoding::encode(album);
    
    let url = format!("https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
        safe_title, safe_artist, safe_album, duration_sec.round()
    );

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(3))
        .user_agent("NullVector-Linux-Player/0.1.0") 
        .build();

    if let Ok(response) = agent.get(&url).call() {
        if let Ok(json) = response.into_json::<Value>() {
            if let Some(synced_lyrics) = json["syncedLyrics"].as_str() {
                if let Ok(mut file) = fs::File::create(&lrc_path) {
                    let _ = file.write_all(synced_lyrics.as_bytes());
                }
                return parse_lrc_content(synced_lyrics);
            }
        }
    }
    lines
}

fn parse_lrc_content(content: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in content.lines() {
        if line.starts_with('[') {
            let parts: Vec<&str> = line.split(']').collect();
            if parts.len() >= 2 {
                let time_str = &parts[0][1..];
                let time_parts: Vec<&str> = time_str.split(':').collect();
                if time_parts.len() == 2 {
                    if let (Ok(m), Ok(s)) = (time_parts[0].parse::<f64>(), time_parts[1].parse::<f64>()) {
                        lines.push(LyricLine { time: (m * 60.0) + s, text: parts[1].trim().to_string() });
                    }
                }
            }
        }
    }
    lines
}

fn upload_art_gpu(picture_data: *mut u8, picture_length: i32) {
    if picture_data.is_null() || picture_length == 0 { return; }
    let slice = unsafe { std::slice::from_raw_parts(picture_data, picture_length as usize) };
    let b64 = general_purpose::STANDARD.encode(slice);
    print!("\x1b_Ga=d,d=A\x1b\\"); 
    print!("\x1b_Ga=t,t=d,f=100,i=1,q=2;{}\x1b\\", b64);
    let _ = stdout().flush();
}

fn place_art_gpu(sy: u16, sx: u16, fit_w: u16, fit_h: u16) {
    print!("\x1b_Ga=d,d=p,i=1,p=1\x1b\\"); 
    print!("\x1b[{};{}H", sy + 1, sx + 1);
    print!("\x1b_Ga=p,i=1,p=1,c={},r={},z=10\x1b\\", fit_w, fit_h);
    let _ = stdout().flush();
}

fn safe_truncate(prefix: &str, val: &str, max_w: usize) -> String {
    let clean_val = val.replace('\n', " ").replace('\r', "");
    let full = format!("{}{}", prefix, clean_val);
    if full.chars().count() > max_w {
        format!("{}...", full.chars().take(max_w.saturating_sub(3)).collect::<String>())
    } else {
        full
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut pl = Playlist::new();
    
    if args.len() >= 2 { pl.add_dir(&args[1]); }

    unsafe { engine_boot() };
    
    let mut dac_buf = [0u8; 256];
    unsafe { boot_dac_menu(dac_buf.as_mut_ptr() as *mut std::ffi::c_char) };
    let dac_c = unsafe { std::ffi::CStr::from_ptr(dac_buf.as_ptr() as *const std::ffi::c_char).to_owned() };
    
    let dac_str_rust = dac_c.to_string_lossy().into_owned();
    let is_pipewire = dac_str_rust == "default" || dac_str_rust == "pipewire";

    enable_raw_mode()?;
    execute!(stdout(), Hide, DisableLineWrap, Clear(ClearType::All), EnableMouseCapture)?;

    let mut ui_sel = 0;
    let mut scroll = 0;
    
    let mut search_mode = false;
    let mut search_query = String::new();
    let mut filtered_sel = 0;
    let mut filtered_scroll = 0;

    let mut folder_mode = false;
    let mut folder_query = String::new();
    
    let mut dir_menu_mode = false;
    let mut dir_menu_path = PathBuf::new();
    let mut dir_menu_items: Vec<String> = Vec::new();
    let mut dir_menu_sel = 0;
    let mut dir_menu_scroll = 0;

    let mut sort_mode = false;
    let mut sort_sel = 0;
    let sort_options = [
        " Date Modified (ASC)",
        " Date Modified (DESC)",
        " Track Number/Name (ASC)",
        " Track Number/Name (DESC)",
    ];
    
    let mut shuf = false;
    let mut rep = false;

    let mut last_cols = 0;
    let mut last_rows = 0;
    
    let mut current_loaded_index = usize::MAX;
    let mut art_loaded = false;
    let mut lyrics: Vec<LyricLine> = Vec::new();
    let mut lrc_scroll = 0;
    let mut sync_lyrics = true;
    let mut force_redraw = true;
    let mut quit_prompt = false;

    loop {
        let state = unsafe { &*engine_get_state() };
        
        if !pl.paths.is_empty() {
            if state.track_finished && current_loaded_index != usize::MAX {
                if rep {
                } else if shuf {
                    pl.current_index = rand::thread_rng().gen_range(0..pl.paths.len());
                } else {
                    pl.current_index = (pl.current_index + 1) % pl.paths.len();
                }
            }

            if current_loaded_index != pl.current_index || (state.track_finished && current_loaded_index != usize::MAX) {
                let current_path = pl.paths[pl.current_index].to_str().unwrap();
                let c_path = CString::new(current_path)?;
                unsafe { engine_load_track(c_path.as_ptr(), dac_c.as_ptr()) };
                
                std::thread::sleep(std::time::Duration::from_millis(50));
                let state_new = unsafe { &*engine_get_state() };
                let c_title = unsafe { std::ffi::CStr::from_ptr(state_new.title.as_ptr()).to_string_lossy().into_owned() };
                let c_art = unsafe { std::ffi::CStr::from_ptr(state_new.artist.as_ptr()).to_string_lossy().into_owned() };
                let c_alb = unsafe { std::ffi::CStr::from_ptr(state_new.album.as_ptr()).to_string_lossy().into_owned() };
                let duration_sec = if state_new.sample_rate > 0 { state_new.total_samples as f64 / state_new.sample_rate as f64 } else { 0.0 };

                lyrics = load_or_fetch_lyrics(current_path, &c_title, &c_art, &c_alb, duration_sec);
                lrc_scroll = 0;
                sync_lyrics = true;
                art_loaded = false;
                current_loaded_index = pl.current_index;
                ui_sel = pl.current_index;
                force_redraw = true;
            }

            let current_state = unsafe { &*engine_get_state() };
            if !art_loaded && !current_state.picture_data.is_null() {
                upload_art_gpu(current_state.picture_data, current_state.picture_length);
                art_loaded = true;
            }
        }

        let (cols, rows) = size()?;
        if cols != last_cols || rows != last_rows {
            force_redraw = true;
            last_cols = cols;
            last_rows = rows;
        }

        if cols < 60 || rows < 12 {
            execute!(stdout(), Clear(ClearType::All), MoveTo(0,0), Print("Terminal too small! Expand window."))?;
            force_redraw = true; 
            if poll(Duration::from_millis(16))? { 
                if let Event::Key(key) = read()? {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') { break; }
                }
            }
            continue;
        }

        let lw = (cols as f32 * 0.70) as u16;
        let mw = cols.saturating_sub(lw);

        let mut match_indices = Vec::new();
        let query_lower = search_query.to_lowercase();
        for (i, path) in pl.paths.iter().enumerate() {
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            if !search_mode || search_query.is_empty() || filename.to_lowercase().contains(&query_lower) {
                match_indices.push(i);
            }
        }
        let match_count = match_indices.len();

        if poll(Duration::from_millis(16))? { 
            match read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if quit_prompt {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => break,
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => { quit_prompt = false; force_redraw = true; }
                                _ => {}
                            }
                        } 
                        else if dir_menu_mode {
                            match key.code {
                                KeyCode::Esc => { dir_menu_mode = false; force_redraw = true; }
                                KeyCode::Up => { if dir_menu_sel > 0 { dir_menu_sel -= 1; force_redraw = true; } }
                                KeyCode::Down => { if dir_menu_sel < dir_menu_items.len().saturating_sub(1) { dir_menu_sel += 1; force_redraw = true; } }
                                KeyCode::Enter => {
                                    if dir_menu_sel == 0 {
                                        let old_len = pl.paths.len();
                                        pl.add_dir(dir_menu_path.to_str().unwrap());
                                        if old_len == 0 && !pl.paths.is_empty() {
                                            pl.current_index = 0;
                                            ui_sel = 0;
                                        }
                                        dir_menu_mode = false;
                                    } else if dir_menu_sel == 1 {
                                        if dir_menu_path.pop() {
                                            dir_menu_items = populate_dir_menu(&dir_menu_path);
                                            dir_menu_sel = 0;
                                            dir_menu_scroll = 0;
                                        }
                                    } else {
                                        dir_menu_path.push(&dir_menu_items[dir_menu_sel]);
                                        dir_menu_items = populate_dir_menu(&dir_menu_path);
                                        dir_menu_sel = 0;
                                        dir_menu_scroll = 0;
                                    }
                                    force_redraw = true;
                                }
                                _ => {}
                            }
                        }
                        else if sort_mode {
                            match key.code {
                                KeyCode::Esc => { sort_mode = false; force_redraw = true; }
                                KeyCode::Up => { if sort_sel > 0 { sort_sel -= 1; force_redraw = true; } }
                                KeyCode::Down => { if sort_sel < 3 { sort_sel += 1; force_redraw = true; } }
                                KeyCode::Enter => {
                                    if !pl.paths.is_empty() {
                                        let playing_path = if current_loaded_index < pl.paths.len() {
                                            Some(pl.paths[current_loaded_index].clone())
                                        } else { None };

                                        match sort_sel {
                                            0 => pl.paths.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH)),
                                            1 => pl.paths.sort_by_key(|p| Reverse(p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH))),
                                            2 => pl.paths.sort_by(|a, b| a.file_name().cmp(&b.file_name())),
                                            3 => pl.paths.sort_by(|a, b| b.file_name().cmp(&a.file_name())),
                                            _ => {}
                                        }

                                        if let Some(cp) = playing_path {
                                            if let Some(new_idx) = pl.paths.iter().position(|p| p == &cp) {
                                                pl.current_index = new_idx;
                                                current_loaded_index = new_idx;
                                                ui_sel = new_idx;
                                            }
                                        }
                                    }
                                    sort_mode = false;
                                    force_redraw = true;
                                }
                                _ => {}
                            }
                        }
                        else if folder_mode {
                            match key.code {
                                KeyCode::Enter => {
                                    folder_mode = false;
                                    let path_to_add = if folder_query.starts_with('~') {
                                        folder_query.replacen('~', &std::env::var("HOME").unwrap_or_default(), 1)
                                    } else {
                                        folder_query.clone()
                                    };
                                    
                                    let p = Path::new(&path_to_add);
                                    if p.is_dir() {
                                        dir_menu_path = p.to_path_buf();
                                        dir_menu_items = populate_dir_menu(&dir_menu_path);
                                        dir_menu_sel = 0;
                                        dir_menu_scroll = 0;
                                        dir_menu_mode = true;
                                    } else {
                                        let old_len = pl.paths.len();
                                        pl.add_dir(&path_to_add);
                                        if old_len == 0 && !pl.paths.is_empty() {
                                            pl.current_index = 0;
                                            ui_sel = 0;
                                        }
                                    }
                                    folder_query.clear();
                                    force_redraw = true;
                                }
                                KeyCode::Esc => { folder_mode = false; force_redraw = true; }
                                KeyCode::Backspace => { folder_query.pop(); force_redraw = true; }
                                KeyCode::Char(c) => { folder_query.push(c); force_redraw = true; }
                                _ => {}
                            }
                        }
                        else if search_mode {
                            match key.code {
                                KeyCode::Enter => {
                                    search_mode = false;
                                    if match_count > 0 {
                                        ui_sel = match_indices[filtered_sel.min(match_count.saturating_sub(1))];
                                        scroll = ui_sel.saturating_sub((rows / 2) as usize);
                                        if !pl.paths.is_empty() { pl.current_index = ui_sel; }
                                    }
                                    force_redraw = true;
                                }
                                KeyCode::Esc => { search_mode = false; force_redraw = true; }
                                KeyCode::Backspace => { search_query.pop(); filtered_sel = 0; force_redraw = true; }
                                KeyCode::Up => { if filtered_sel > 0 { filtered_sel -= 1; force_redraw = true; } }
                                KeyCode::Down => { if filtered_sel < match_count.saturating_sub(1) { filtered_sel += 1; force_redraw = true; } }
                                KeyCode::Char(c) => { search_query.push(c); filtered_sel = 0; force_redraw = true; }
                                _ => {}
                            }
                        } 
                        else {
                            match key.code {
                                KeyCode::Esc => { sort_mode = true; sort_sel = 0; force_redraw = true; }
                                KeyCode::Char('f') => { folder_mode = true; folder_query.clear(); force_redraw = true; }
                                KeyCode::Char('S') => { search_mode = true; search_query.clear(); filtered_sel = 0; force_redraw = true; }
                                KeyCode::Char('q') => { quit_prompt = true; force_redraw = true; }
                                KeyCode::Char(' ') => { if !pl.paths.is_empty() { unsafe { engine_play_pause() } } },
                                KeyCode::Char('n') => {
                                    if !pl.paths.is_empty() {
                                        if shuf { pl.current_index = rand::thread_rng().gen_range(0..pl.paths.len()); } 
                                        else { pl.current_index = (pl.current_index + 1) % pl.paths.len(); }
                                        ui_sel = pl.current_index;
                                        force_redraw = true;
                                    }
                                }
                                KeyCode::Char('p') => {
                                    if !pl.paths.is_empty() {
                                        if pl.current_index == 0 { pl.current_index = pl.paths.len().saturating_sub(1); }
                                        else { pl.current_index -= 1; }
                                        ui_sel = pl.current_index;
                                        force_redraw = true;
                                    }
                                }
                                KeyCode::Char('s') => { shuf = !shuf; force_redraw = true; }
                                KeyCode::Char('r') => { rep = !rep; force_redraw = true; }
                                KeyCode::Char('i') => { sync_lyrics = !sync_lyrics; force_redraw = true; }
                                KeyCode::Right => { if !pl.paths.is_empty() { unsafe { engine_seek_fwd() } } },
                                KeyCode::Left | KeyCode::Char('b') => { if !pl.paths.is_empty() { unsafe { engine_seek_bwd() } } },
                                KeyCode::Up => {
                                    if !sync_lyrics && !lyrics.is_empty() {
                                        if lrc_scroll > 0 { lrc_scroll -= 1; force_redraw = true; }
                                    } else if ui_sel > 0 { 
                                        ui_sel -= 1; force_redraw = true; 
                                    }
                                }
                                KeyCode::Down => {
                                    if !sync_lyrics && !lyrics.is_empty() {
                                        if lrc_scroll < lyrics.len().saturating_sub(1) { lrc_scroll += 1; force_redraw = true; }
                                    } else if ui_sel < pl.paths.len().saturating_sub(1) { 
                                        ui_sel += 1; force_redraw = true; 
                                    }
                                }
                                KeyCode::Enter => {
                                    if !pl.paths.is_empty() {
                                        pl.current_index = ui_sel;
                                        force_redraw = true;
                                    }
                                }
                                KeyCode::Char('=') | KeyCode::Char('+') => unsafe { engine_set_volume((state.volume + 0.01).min(1.0)) },
                                KeyCode::Char('-') => unsafe { engine_set_volume((state.volume - 0.01).max(0.0)) },
                                _ => {}
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if !quit_prompt {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                if dir_menu_mode {
                                    if dir_menu_sel > 0 { dir_menu_sel -= 1; force_redraw = true; }
                                } else if search_mode {
                                    if filtered_sel > 0 { filtered_sel -= 1; force_redraw = true; }
                                } else if sort_mode {
                                    if sort_sel > 0 { sort_sel -= 1; force_redraw = true; }
                                } else if !sync_lyrics && !lyrics.is_empty() {
                                    if lrc_scroll > 0 { lrc_scroll -= 1; force_redraw = true; }
                                } else {
                                    if ui_sel > 0 { ui_sel -= 1; force_redraw = true; }
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                if dir_menu_mode {
                                    if dir_menu_sel < dir_menu_items.len().saturating_sub(1) { dir_menu_sel += 1; force_redraw = true; }
                                } else if search_mode {
                                    if filtered_sel < match_count.saturating_sub(1) { filtered_sel += 1; force_redraw = true; }
                                } else if sort_mode {
                                    if sort_sel < 3 { sort_sel += 1; force_redraw = true; }
                                } else if !sync_lyrics && !lyrics.is_empty() {
                                    if lrc_scroll < lyrics.len().saturating_sub(1) { lrc_scroll += 1; force_redraw = true; }
                                } else {
                                    if ui_sel < pl.paths.len().saturating_sub(1) { ui_sel += 1; force_redraw = true; }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Resize(_, _) => force_redraw = true,
                _ => {}
            }
        }

        // ==========================================
        // STATIC UI (Only redraws when interaction happens)
        // ==========================================
        if force_redraw {
            execute!(stdout(), Clear(ClearType::All))?;
            
            let top_left = "─".repeat((lw.saturating_sub(1)) as usize);
            let top_right = "─".repeat((cols.saturating_sub(lw).saturating_sub(2)) as usize);
            
            execute!(
                stdout(),
                SetForegroundColor(Color::DarkGrey),
                MoveTo(0, 0), Print(format!("┌{}┬{}┐", top_left, top_right)),
                MoveTo(0, rows - 2), Print(format!("└{}┴{}┘", top_left, top_right))
            )?;

            for r in 1..rows-2 { execute!(stdout(), MoveTo(0, r), Print("│"), MoveTo(lw, r), Print("│"), MoveTo(cols-1, r), Print("│"))?; }
            execute!(stdout(), ResetColor)?;
            
            execute!(stdout(), MoveTo(0, rows - 1))?;
            let help_text = if quit_prompt {
                " ARE YOU SURE YOU WANT TO QUIT? (y/n) "
            } else {
                " [SPC] Pause | [ENTER] Play | [< >] 5s | [f] Folder | [S] Search | [ESC] Sort | [i] Sync | [q] Quit "
            };
            let display_help = safe_truncate("", help_text, cols as usize);

            execute!(
                stdout(),
                SetBackgroundColor(if quit_prompt { Color::Red } else { Color::White }),
                SetForegroundColor(if quit_prompt { Color::White } else { Color::Black }),
                Print(format!("{:<width$}", display_help, width = cols as usize)),
                ResetColor
            )?;

            if folder_mode {
                let folder_display = safe_truncate(" DIRECTORY PATH: ", &format!("{}_ ", folder_query), lw.saturating_sub(4) as usize);
                execute!(stdout(), MoveTo(2, 0), SetForegroundColor(Color::Yellow), Print(folder_display), ResetColor)?;
            } else if search_mode {
                let search_display = safe_truncate(" SEARCH: ", &format!("{}_ ", search_query), lw.saturating_sub(4) as usize);
                execute!(stdout(), MoveTo(2, 0), SetForegroundColor(Color::Yellow), Print(search_display), ResetColor)?;
            } else {
                execute!(stdout(), MoveTo(2, 0), SetForegroundColor(Color::White), Print(format!(" LIBRARY [{:03} Tracks] ", pl.paths.len())), ResetColor)?;
            }
            
            if art_loaded && !pl.paths.is_empty() { 
                let mut art_h = (rows / 2).saturating_sub(3); 
                let mut art_w = art_h * 2; 
                let max_w = mw.saturating_sub(6);
                if art_w > max_w { art_w = max_w; art_h = art_w / 2; }
                let art_x = lw + 3 + (max_w.saturating_sub(art_w) / 2);
                place_art_gpu(2, art_x, art_w, art_h); 
            }

            let list_h = (rows.saturating_sub(3)) as usize; 
            
            if search_mode {
                if filtered_sel >= match_count && match_count > 0 { filtered_sel = match_count - 1; }
                if filtered_sel < filtered_scroll { filtered_scroll = filtered_sel; }
                if filtered_sel >= filtered_scroll + list_h { filtered_scroll = filtered_sel.saturating_sub(list_h).saturating_add(1); }
            } else {
                if ui_sel < scroll { scroll = ui_sel; }
                if ui_sel >= scroll + list_h { scroll = ui_sel.saturating_sub(list_h).saturating_add(1); }
            }

            let pl_max_w = lw.saturating_sub(4) as usize;

            if pl.paths.is_empty() {
                execute!(stdout(), MoveTo(2, 2), SetForegroundColor(Color::DarkGrey), Print("Playlist empty. Press 'f' to append directory."), ResetColor)?;
            } else {
                for i in 0..list_h {
                    let current_scroll = if search_mode { filtered_scroll } else { scroll };
                    let idx = current_scroll + i;
                    if idx >= match_count { break; }

                    let real_idx = match_indices[idx];
                    let filename = pl.paths[real_idx].file_name().unwrap_or_default().to_string_lossy();
                    
                    let is_selected = if search_mode { idx == filtered_sel } else { real_idx == ui_sel };
                    let is_playing = real_idx == pl.current_index;

                    execute!(stdout(), MoveTo(2, i as u16 + 1))?; 
                    if is_selected {
                        execute!(stdout(), SetBackgroundColor(Color::White), SetForegroundColor(Color::Black))?;
                    } else if is_playing {
                        execute!(stdout(), SetForegroundColor(Color::Green))?;
                    }

                    let display_str = safe_truncate("", &format!("[{:03}] {}", real_idx + 1, filename), pl_max_w);
                    print!("{:<width$}", display_str, width = pl_max_w);
                    execute!(stdout(), ResetColor)?;
                }
            }
        }

        // ==========================================
        // DYNAMIC UI (Redraws 60 times a second without input)
        // ==========================================
        let ah = (rows / 2).saturating_sub(1);
        let mut ty = ah + 2;
        let mw_max = mw.saturating_sub(4) as usize; 
        
        let state = unsafe { &*engine_get_state() };
        let status = if state.is_playing { ">> PLAYING" } else { "|| PAUSED" };
        let mode_str = format!("[{}{}]", if shuf { "S" } else { "-" }, if rep { "R" } else { "-" });
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("", &format!("{} {}", status, mode_str), mw_max), width=mw_max)))?; ty += 1;
        
        let c_title = unsafe { std::ffi::CStr::from_ptr(state.title.as_ptr()).to_string_lossy() };
        let c_art = unsafe { std::ffi::CStr::from_ptr(state.artist.as_ptr()).to_string_lossy() };
        let c_alb = unsafe { std::ffi::CStr::from_ptr(state.album.as_ptr()).to_string_lossy() };
        let c_yr = unsafe { std::ffi::CStr::from_ptr(state.date.as_ptr()).to_string_lossy() };
        let c_trk = unsafe { std::ffi::CStr::from_ptr(state.track_num.as_ptr()).to_string_lossy() };

        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("Title:  ", &c_title, mw_max), width=mw_max)))?; ty += 1;
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("Artist: ", &c_art, mw_max), width=mw_max)))?; ty += 1;
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("Album:  ", &c_alb, mw_max), width=mw_max)))?; ty += 1;
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("Year:   ", &c_yr, mw_max), width=mw_max)))?; ty += 1;
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("Track:  ", &c_trk, mw_max), width=mw_max)))?; ty += 1;

        let rate_str = format!("RATE: {} kbps | VOL: {}%", state.kbps, (state.volume * 100.0) as i32);
        execute!(stdout(), MoveTo(lw + 2, ty), Print(format!("{:<width$}", safe_truncate("", &rate_str, mw_max), width=mw_max)))?; ty += 1;
        
        let flac_str = format!("FLAC: {}-bit / {:.1} kHz", state.bps, state.sample_rate as f32 / 1000.0);
        execute!(stdout(), MoveTo(lw + 2, ty), SetForegroundColor(Color::Cyan), Print(format!("{:<width$}", safe_truncate("", &flac_str, mw_max), width=mw_max)), ResetColor)?; ty += 1;

        if is_pipewire {
            let dac_str = format!("DAC : PipeWire [{}-bit SHARED]", state.hw_format_state);
            execute!(stdout(), MoveTo(lw + 2, ty), SetForegroundColor(Color::Yellow), Print(format!("{:<width$}", safe_truncate("", &dac_str, mw_max), width=mw_max)), ResetColor)?; ty += 1;
        } else if state.bps == state.hw_format_state as u32 && !pl.paths.is_empty() {
            let dac_str = format!("DAC : {}-bit [BIT-PERFECT]", state.hw_format_state);
            execute!(stdout(), MoveTo(lw + 2, ty), SetForegroundColor(Color::Green), Print(format!("{:<width$}", safe_truncate("", &dac_str, mw_max), width=mw_max)), ResetColor)?; ty += 1;
        } else {
            let dac_str = format!("DAC : {}-bit [ALSA PADDED]", state.hw_format_state);
            execute!(stdout(), MoveTo(lw + 2, ty), SetForegroundColor(Color::Yellow), Print(format!("{:<width$}", safe_truncate("", &dac_str, mw_max), width=mw_max)), ResetColor)?; ty += 1;
        }

        let viz_y = ty; 
        let viz_max_h = rows.saturating_sub(4).saturating_sub(viz_y + 1) as usize; 
        
        let dash_count = mw.saturating_sub(12) as usize;
        let l_border = "─".repeat(dash_count);
        execute!(
            stdout(),
            MoveTo(lw, viz_y),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("├─ LYRICS {}┤", l_border)),
            ResetColor
        )?;

        if lyrics.is_empty() || pl.paths.is_empty() {
            let msg = if pl.paths.is_empty() { "WAITING FOR TRACKS..." } else { "NO .LRC FOUND" };
            for i in 0..viz_max_h {
                execute!(stdout(), MoveTo(lw + 2, viz_y + 1 + i as u16))?;
                if i == viz_max_h / 2 {
                    let padding = mw_max.saturating_sub(msg.len()) / 2;
                    let line = format!("{}{}{}", " ".repeat(padding), msg, " ".repeat(mw_max.saturating_sub(padding + msg.len())));
                    execute!(stdout(), SetForegroundColor(Color::DarkGrey), Print(format!("{:<width$}", line, width=mw_max)), ResetColor)?;
                } else { execute!(stdout(), Print(" ".repeat(mw_max)))?; }
            }
        } else {
            let current_time = state.current_sample as f64 / state.sample_rate as f64;
            let mut active_line = 0;
            for (i, line) in lyrics.iter().enumerate() {
                if current_time >= line.time { active_line = i; } else { break; }
            }

            if sync_lyrics { lrc_scroll = active_line.saturating_sub(viz_max_h / 2); }
            let max_scroll = lyrics.len().saturating_sub(viz_max_h);
            if lrc_scroll > max_scroll { lrc_scroll = max_scroll; }

            for i in 0..viz_max_h {
                let idx = lrc_scroll + i;
                execute!(stdout(), MoveTo(lw + 2, viz_y + 1 + i as u16))?;
                
                if idx < lyrics.len() {
                    if sync_lyrics && idx == active_line {
                        let line_text = safe_truncate("", &format!(">> {}", lyrics[idx].text), mw_max);
                        execute!(stdout(), SetForegroundColor(Color::Cyan), Print(format!("{:<width$}", line_text, width=mw_max)), ResetColor)?;
                    } else {
                        let line_text = safe_truncate("", &format!("   {}", lyrics[idx].text), mw_max);
                        execute!(stdout(), Print(format!("{:<width$}", line_text, width=mw_max)))?;
                    }
                } else { execute!(stdout(), Print(" ".repeat(mw_max)))?; }
            }
        }

        let cur_s = state.current_sample as f64 / state.sample_rate as f64;
        let tot_s = if state.sample_rate > 0 { state.total_samples as f64 / state.sample_rate as f64 } else { 0.0 };
        
        let time_str = format!("{:02}:{:02} / {:02}:{:02}", 
            (cur_s / 60.0) as i32, (cur_s % 60.0) as i32, 
            (tot_s / 60.0) as i32, (tot_s % 60.0) as i32
        );
        execute!(stdout(), MoveTo(lw + 2, rows - 4), Print(format!("{:<width$}", safe_truncate("", &time_str, mw_max), width=mw_max)))?;

        let bar_w = mw.saturating_sub(6) as usize;
        if bar_w > 0 {
            execute!(stdout(), MoveTo(lw + 2, rows - 3), SetForegroundColor(Color::Cyan), Print("["))?;
            let prg = if state.total_samples > 0 { state.current_sample as f64 / state.total_samples as f64 } else { 0.0 };
            let fill = (prg * bar_w as f64) as usize;
            for i in 0..bar_w { if i < fill { print!("█"); } else { print!("░"); } }
            execute!(stdout(), Print("]"), ResetColor)?;
        }

        // ==========================================
        // FLOATING OVERLAYS (Drawn last so they stay on top)
        // ==========================================
        if dir_menu_mode || sort_mode {
            if dir_menu_mode {
                let menu_w = 46;
                let menu_h = 16;
                let start_x = (cols.saturating_sub(menu_w)) / 2;
                let start_y = (rows.saturating_sub(menu_h)) / 2;

                let top = "─".repeat((menu_w - 2) as usize);
                
                execute!(
                    stdout(),
                    SetForegroundColor(Color::White),
                    SetBackgroundColor(Color::Black),
                    MoveTo(start_x, start_y), Print(format!("┌{}┐", top))
                )?;
                
                for i in 1..(menu_h-1) { execute!(stdout(), MoveTo(start_x, start_y + i), Print(format!("│{:width$}│", " ", width=(menu_w-2) as usize)))?; }
                execute!(stdout(), MoveTo(start_x, start_y + menu_h - 1), Print(format!("└{}┘", top)))?;

                execute!(stdout(), MoveTo(start_x + 2, start_y), SetForegroundColor(Color::Yellow), Print(" DIRECTORY BROWSER "), ResetColor)?;

                let path_str = dir_menu_path.to_string_lossy();
                let trunc_path = safe_truncate(" ", &path_str, (menu_w - 4) as usize);
                execute!(stdout(), MoveTo(start_x + 2, start_y + menu_h - 1), SetForegroundColor(Color::DarkGrey), Print(trunc_path), ResetColor)?;

                let list_h = (menu_h - 2) as usize;
                if dir_menu_sel < dir_menu_scroll { dir_menu_scroll = dir_menu_sel; }
                if dir_menu_sel >= dir_menu_scroll + list_h { dir_menu_scroll = dir_menu_sel.saturating_sub(list_h).saturating_add(1); }

                for i in 0..list_h {
                    let idx = dir_menu_scroll + i;
                    if idx >= dir_menu_items.len() { break; }
                    
                    execute!(stdout(), MoveTo(start_x + 2, start_y + 1 + i as u16))?;
                    if idx == dir_menu_sel {
                        execute!(stdout(), SetBackgroundColor(Color::White), SetForegroundColor(Color::Black))?;
                    } else if idx == 0 {
                        execute!(stdout(), SetForegroundColor(Color::Green))?; 
                    }
                    
                    let opt = &dir_menu_items[idx];
                    let prefix = if idx > 1 { "📁 " } else { "" };
                    let display = safe_truncate(prefix, opt, (menu_w - 4) as usize);
                    execute!(stdout(), Print(format!("{:<width$}", display, width=(menu_w-4) as usize)), ResetColor)?;
                }
            } 
            else if sort_mode {
                let menu_w = 34;
                let menu_h = 6;
                let start_x = (cols.saturating_sub(menu_w)) / 2;
                let start_y = (rows.saturating_sub(menu_h)) / 2;

                let top = "─".repeat((menu_w - 2) as usize);
                
                execute!(
                    stdout(),
                    SetForegroundColor(Color::White),
                    SetBackgroundColor(Color::Black),
                    MoveTo(start_x, start_y), Print(format!("┌{}┐", top))
                )?;
                
                for i in 1..(menu_h-1) { execute!(stdout(), MoveTo(start_x, start_y + i), Print(format!("│{:width$}│", " ", width=(menu_w-2) as usize)))?; }
                execute!(stdout(), MoveTo(start_x, start_y + menu_h - 1), Print(format!("└{}┘", top)))?;

                execute!(stdout(), MoveTo(start_x + 2, start_y), SetForegroundColor(Color::Yellow), Print(" SORT PLAYLIST "), ResetColor)?;

                for (i, opt) in sort_options.iter().enumerate() {
                    execute!(stdout(), MoveTo(start_x + 2, start_y + 1 + i as u16))?;
                    if i == sort_sel { execute!(stdout(), SetBackgroundColor(Color::White), SetForegroundColor(Color::Black))?; }
                    execute!(stdout(), Print(format!("{:<width$}", opt, width=(menu_w-4) as usize)), ResetColor)?;
                }
            }
        }
        force_redraw = false;
    }

    unsafe { engine_shutdown() };
    disable_raw_mode()?;
    execute!(stdout(), DisableMouseCapture, EnableLineWrap, Show, Clear(ClearType::All))?;
    print!("\x1b_Ga=d,d=A\x1b\\"); 
    Ok(())
}