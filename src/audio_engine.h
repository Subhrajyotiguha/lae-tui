#ifndef AUDIO_ENGINE_H
#define AUDIO_ENGINE_H

#include <alsa/asoundlib.h>
#include <FLAC/stream_decoder.h>
#include <stdbool.h>

// This is the shared memory bridge. Rust will read this to draw the TUI.
typedef struct {
    char title[256];
    char artist[256];
    char album[256];
    char date[32];
    char track_num[16];
    
    unsigned int sample_rate;
    unsigned int channels;
    unsigned int bps;
    
    FLAC__uint64 total_samples;
    FLAC__uint64 current_sample;
    
    unsigned char *picture_data;
    int picture_length;
    float spectrum[16]; 
    
    int kbps;
    float volume;
    int hw_format_state;
    
    bool is_playing;
    bool track_finished;
    bool is_pipewire;
} EngineState;

// --- FFI API FOR RUST ---
void engine_boot(void);
int engine_load_track(const char *filepath, const char *dac_device);
void engine_play_pause(void);
void engine_seek_fwd(void);
void engine_seek_bwd(void);
void engine_set_volume(float vol);
EngineState* engine_get_state(void);
void engine_shutdown(void);

// --- INTERNAL C FUNCTIONS ---
void engine_silent_alsa(void);
int engine_init_alsa(const char *device, unsigned int bps, unsigned int rate, unsigned int channels);
void engine_close_alsa(void);
void boot_dac_menu(char *selected_device);

FLAC__StreamDecoderWriteStatus write_callback(const FLAC__StreamDecoder *decoder, const FLAC__Frame *frame, const FLAC__int32 * const buffer[], void *client_data);
void metadata_callback(const FLAC__StreamDecoder *decoder, const FLAC__StreamMetadata *metadata, void *client_data);
void error_callback(const FLAC__StreamDecoder *decoder, FLAC__StreamDecoderErrorStatus status, void *client_data);

#endif