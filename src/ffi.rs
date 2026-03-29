use std::ffi::{c_char, c_int, c_uint};

#[repr(C)]
pub struct EngineState {
    pub title: [c_char; 256],
    pub artist: [c_char; 256],
    pub album: [c_char; 256],
    pub date: [c_char; 32],
    pub track_num: [c_char; 16],

    pub sample_rate: c_uint,
    pub channels: c_uint,
    pub bps: c_uint,

    pub total_samples: u64,
    pub current_sample: u64,

    pub picture_data: *mut u8,
    pub picture_length: c_int,
    pub spectrum: [f32; 16],

    pub kbps: c_int,
    pub volume: f32,
    pub hw_format_state: c_int,

    pub is_playing: bool,
    pub track_finished: bool,
}

unsafe extern "C" {
    pub fn engine_boot();
    pub fn engine_load_track(filepath: *const c_char, dac_device: *const c_char) -> c_int;
    pub fn engine_play_pause();
    pub fn engine_seek_fwd();
    pub fn engine_seek_bwd();
    pub fn engine_set_volume(vol: f32);
    pub fn engine_get_state() -> *mut EngineState;
    pub fn engine_shutdown();
    pub fn boot_dac_menu(selected_device: *mut c_char);
}