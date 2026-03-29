#include "audio_engine.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h> 
#include <pthread.h>
#include <unistd.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

snd_pcm_t *pcm_handle = NULL;
EngineState global_state;
FLAC__StreamDecoder *dec = NULL;
pthread_t audio_thread;

float current_fade = 0.0f; 
int fading_in = 1; 
FLAC__uint64 lst_p = 0;
FLAC__uint64 lst_s = 0;

void engine_silent_alsa() { snd_lib_error_set_handler(NULL); }

int engine_init_alsa(const char *device, unsigned int bps, unsigned int rate, unsigned int channels) {
    if (pcm_handle) { snd_pcm_drop(pcm_handle); snd_pcm_close(pcm_handle); pcm_handle = NULL; }
    
    int err = snd_pcm_open(&pcm_handle, device, SND_PCM_STREAM_PLAYBACK, 0);
    if (err < 0) return err;

    if (bps > 16) {
        err = snd_pcm_set_params(pcm_handle, SND_PCM_FORMAT_S24_LE, SND_PCM_ACCESS_RW_INTERLEAVED, channels, rate, 1, 500000);
        if (err >= 0) { global_state.hw_format_state = 24; return 0; }

        snd_pcm_close(pcm_handle); snd_pcm_open(&pcm_handle, device, SND_PCM_STREAM_PLAYBACK, 0);
        err = snd_pcm_set_params(pcm_handle, SND_PCM_FORMAT_S32_LE, SND_PCM_ACCESS_RW_INTERLEAVED, channels, rate, 1, 500000);
        if (err >= 0) { global_state.hw_format_state = 32; return 0; }
    }

    snd_pcm_close(pcm_handle); snd_pcm_open(&pcm_handle, device, SND_PCM_STREAM_PLAYBACK, 0);
    err = snd_pcm_set_params(pcm_handle, SND_PCM_FORMAT_S16_LE, SND_PCM_ACCESS_RW_INTERLEAVED, channels, rate, 1, 500000);
    if (err >= 0) { global_state.hw_format_state = 16; return 0; }

    return err;
}

void engine_close_alsa() { if (pcm_handle) { snd_pcm_drop(pcm_handle); snd_pcm_close(pcm_handle); pcm_handle = NULL; } }

void boot_dac_menu(char *selected_device) {
    void **hints, **n; char dac_list[20][256]; int count = 1; 
    
    // Hardcode PipeWire (default ALSA route) as Option 0
    strncpy(dac_list[0], "default", 255);

    printf("\n Lossless Audio Engine | HARDWARE SELECTOR \n---------------------------\n");
    printf(" [0] PipeWire (Standard Desktop Audio - Shared/Not Bit-Perfect)\n");

    if (snd_device_name_hint(-1, "pcm", &hints) >= 0) {
        for (n = hints; *n != NULL && count < 20; n++) {
            char *name = snd_device_name_get_hint(*n, "NAME");
            // Only show direct hardware devices to avoid ALSA plugin clutter
            if (name && (strncmp(name, "hw:", 3) == 0 || strncmp(name, "plughw:", 7) == 0)) {
                strncpy(dac_list[count], name, 255); 
                printf(" [%d] %s\n", count, name); 
                count++;
            }
            if (name) free(name);
        }
        snd_device_name_free_hint(hints);
    }
    
    printf("Select Output (0-%d): ", count - 1);
    char in[10]; 
    if (fgets(in, 10, stdin)) {
        int sel = atoi(in);
        if (sel < 0 || sel >= count) sel = 0;
        strcpy(selected_device, dac_list[sel]);
    } else {
        exit(0);
    }
}

void metadata_callback(const FLAC__StreamDecoder *decoder, const FLAC__StreamMetadata *metadata, void *client_data) {
    EngineState *m = (EngineState *)client_data;
    if (metadata->type == FLAC__METADATA_TYPE_STREAMINFO) {
        m->total_samples = metadata->data.stream_info.total_samples;
        m->bps = metadata->data.stream_info.bits_per_sample;
        m->sample_rate = metadata->data.stream_info.sample_rate;
        m->channels = metadata->data.stream_info.channels;
    } else if (metadata->type == FLAC__METADATA_TYPE_VORBIS_COMMENT) {
        for (int i = 0; i < (int)metadata->data.vorbis_comment.num_comments; i++) {
            char *entry = (char *)metadata->data.vorbis_comment.comments[i].entry;
            if (strncasecmp(entry, "TITLE=", 6) == 0) strncpy(m->title, entry + 6, 255);
            else if (strncasecmp(entry, "ARTIST=", 7) == 0) strncpy(m->artist, entry + 7, 255);
            else if (strncasecmp(entry, "ALBUM=", 6) == 0) strncpy(m->album, entry + 6, 255);
            else if (strncasecmp(entry, "DATE=", 5) == 0) strncpy(m->date, entry + 5, 31);
            else if (strncasecmp(entry, "TRACKNUMBER=", 12) == 0) strncpy(m->track_num, entry + 12, 15);
        }
    } else if (metadata->type == FLAC__METADATA_TYPE_PICTURE) {
        if (m->picture_data == NULL) {
            m->picture_length = metadata->data.picture.data_length;
            m->picture_data = malloc(m->picture_length);
            if (m->picture_data) memcpy(m->picture_data, metadata->data.picture.data, m->picture_length);
        }
    }
}

void error_callback(const FLAC__StreamDecoder *decoder, FLAC__StreamDecoderErrorStatus status, void *client_data) {}

void compute_fft(float* real, float* imag, int n) {
    int i, j, k, n1, n2, a;
    float c, s, t1, t2;
    int steps = 0, temp = n;
    while (temp > 1) { steps++; temp >>= 1; }
    j = 0;
    for (i = 0; i < n - 1; i++) {
        if (i < j) {
            t1 = real[i]; real[i] = real[j]; real[j] = t1;
            t1 = imag[i]; imag[i] = imag[j]; imag[j] = t1;
        }
        k = n / 2;
        while (k <= j) { j -= k; k /= 2; }
        j += k;
    }
    n1 = 0; n2 = 1;
    for (i = 0; i < steps; i++) {
        n1 = n2; n2 = n2 + n2;
        a = 0;
        for (j = 0; j < n1; j++) {
            c = cosf(-2.0f * M_PI * a / n);
            s = sinf(-2.0f * M_PI * a / n);
            a += 1 << (steps - i - 1);
            for (k = j; k < n; k += n2) {
                t1 = c * real[k + n1] - s * imag[k + n1];
                t2 = s * real[k + n1] + c * imag[k + n1];
                real[k + n1] = real[k] - t1;
                imag[k + n1] = imag[k] - t2;
                real[k] += t1;
                imag[k] += t2;
            }
        }
    }
}

FLAC__StreamDecoderWriteStatus write_callback(const FLAC__StreamDecoder *decoder, const FLAC__Frame *frame, const FLAC__int32 * const buffer[], void *client_data) {
    EngineState *m = (EngineState *)client_data;
    int blocksize = frame->header.blocksize;
    int channels = frame->header.channels;
    m->current_sample += blocksize;
    
    float fade_step = 1.0f / (0.5f * m->sample_rate);
    float base_gain = m->volume * m->volume * m->volume;
    int err; 

    #define FFT_SIZE 512
    if (blocksize >= FFT_SIZE) {
        float real[FFT_SIZE];
        float imag[FFT_SIZE];
        for (int i = 0; i < FFT_SIZE; i++) {
            float val = (float)buffer[0][i] / (float)(1LL << (m->bps - 1));
            float hann = 0.5f * (1.0f - cosf(2.0f * M_PI * i / (FFT_SIZE - 1)));
            real[i] = val * hann;
            imag[i] = 0.0f;
        }
        compute_fft(real, imag, FFT_SIZE);
        int band_limits[17] = {1, 2, 3, 5, 7, 10, 14, 20, 28, 40, 56, 80, 114, 160, 200, 256}; 
        for (int b = 0; b < 16; b++) {
            float sum = 0;
            int start_bin = (b == 0) ? 1 : band_limits[b - 1];
            int end_bin = band_limits[b];
            int count = end_bin - start_bin;
            if (count <= 0) count = 1;
            for (int i = start_bin; i < end_bin; i++) {
                float mag = sqrtf(real[i]*real[i] + imag[i]*imag[i]);
                sum += mag;
            }
            float avg_mag = (sum / count) * 4.0f; 
            float eq_val = avg_mag * (1.0f + (b * 0.25f)); 
            if (eq_val > m->spectrum[b]) m->spectrum[b] = eq_val;
        }
    }

    if (m->hw_format_state == 16) {
        short out[blocksize * channels];
        for (int i = 0; i < blocksize; i++) {
            if (fading_in && current_fade < 1.0f) current_fade += fade_step;
            else if (!fading_in && current_fade > 0.0f) current_fade -= fade_step;
            float sample_gain = base_gain * current_fade;
            for (int c = 0; c < channels; c++) {
                int s = buffer[c][i];
                if (m->bps == 24) s >>= 8; 
                out[i * channels + c] = (short)((float)s * sample_gain);
            }
        }
        err = snd_pcm_writei(pcm_handle, out, blocksize);
    } else {
        int out[blocksize * channels];
        for (int i = 0; i < blocksize; i++) {
            if (fading_in && current_fade < 1.0f) current_fade += fade_step;
            else if (!fading_in && current_fade > 0.0f) current_fade -= fade_step;
            float sample_gain = base_gain * current_fade;
            for (int c = 0; c < channels; c++) {
                int s = buffer[c][i];
                s = (int)((float)s * sample_gain);
                if (m->hw_format_state == 32 && m->bps == 24) s <<= 8;
                if (m->hw_format_state == 32 && m->bps == 16) s <<= 16;
                if (m->hw_format_state == 24 && m->bps == 16) s <<= 8;
                out[i * channels + c] = s;
            }
        }
        err = snd_pcm_writei(pcm_handle, out, blocksize);
    }

    if (err == -EPIPE) snd_pcm_prepare(pcm_handle);
    else if (err < 0) snd_pcm_recover(pcm_handle, err, 0);

    return FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE;
}

// --- THE BACKGROUND DECODING THREAD ---
void* audio_worker_thread(void* arg) {
    FLAC__uint64 cur_p = 0;
    while (!global_state.track_finished) {
        if (!global_state.is_playing) {
            fading_in = 0;
            usleep(10000); 
            continue;
        } else {
            fading_in = 1;
        }

        if (!FLAC__stream_decoder_process_single(dec)) {
            global_state.track_finished = true;
            break;
        }
        
        if (FLAC__stream_decoder_get_state(dec) == FLAC__STREAM_DECODER_END_OF_STREAM) {
            global_state.track_finished = true;
            break;
        }
        
        FLAC__stream_decoder_get_decode_position(dec, &cur_p);
        if (global_state.current_sample - lst_s >= (global_state.sample_rate / 2)) {
            double dt = (double)(global_state.current_sample - lst_s) / global_state.sample_rate;
            if(dt > 0) global_state.kbps = (int)(((cur_p - lst_p) * 8.0) / dt / 1000.0);
            lst_p = cur_p; 
            lst_s = global_state.current_sample;
        }
    }
    return NULL;
}

// --- FFI EXPORTS FOR RUST ---

void engine_boot(void) {
    memset(&global_state, 0, sizeof(EngineState));
    global_state.volume = 0.30f;
    engine_silent_alsa();
}

int engine_load_track(const char *filepath, const char *dac_device) {
    if (dec) {
        global_state.track_finished = true;
        pthread_join(audio_thread, NULL);
        FLAC__stream_decoder_finish(dec);
        FLAC__stream_decoder_delete(dec);
    }
    
    if (global_state.picture_data) {
        free(global_state.picture_data);
    }

    float saved_vol = global_state.volume;
    memset(&global_state, 0, sizeof(EngineState));
    global_state.volume = saved_vol;
    global_state.is_playing = true;
    global_state.track_finished = false;
    
    // --- THIS IS THE MISSING PIPEWIRE FLAG LOGIC ---
    if (strcmp(dac_device, "default") == 0 || strcmp(dac_device, "pipewire") == 0) {
        global_state.is_pipewire = true;
    } else {
        global_state.is_pipewire = false;
    }
    // -----------------------------------------------

    current_fade = 0.0f;
    lst_p = 0;
    lst_s = 0;

    dec = FLAC__stream_decoder_new();
    FLAC__stream_decoder_set_metadata_respond(dec, FLAC__METADATA_TYPE_VORBIS_COMMENT);
    FLAC__stream_decoder_set_metadata_respond(dec, FLAC__METADATA_TYPE_PICTURE);
    FLAC__stream_decoder_init_file(dec, filepath, write_callback, metadata_callback, error_callback, &global_state);
    FLAC__stream_decoder_process_until_end_of_metadata(dec);
    
    engine_init_alsa(dac_device, global_state.bps, global_state.sample_rate, global_state.channels);
    
    pthread_create(&audio_thread, NULL, audio_worker_thread, NULL);
    return 0;
}

void engine_play_pause(void) {
    global_state.is_playing = !global_state.is_playing;
}

void engine_seek_fwd(void) {
    FLAC__uint64 target = global_state.current_sample + (5 * global_state.sample_rate);
    if (target < global_state.total_samples) { 
        FLAC__stream_decoder_seek_absolute(dec, target); 
        global_state.current_sample = target; 
    }
}

void engine_seek_bwd(void) {
    FLAC__int64 target = (FLAC__int64)global_state.current_sample - (5 * global_state.sample_rate);
    if (target < 0) target = 0;
    FLAC__stream_decoder_seek_absolute(dec, (FLAC__uint64)target); 
    global_state.current_sample = (FLAC__uint64)target;
}

void engine_set_volume(float vol) {
    if (vol > 1.0f) vol = 1.0f;
    if (vol < 0.0f) vol = 0.0f;
    global_state.volume = vol;
}

EngineState* engine_get_state(void) {
    return &global_state;
}

void engine_shutdown(void) {
    if (dec) {
        global_state.track_finished = true;
        pthread_join(audio_thread, NULL);
        FLAC__stream_decoder_finish(dec);
        FLAC__stream_decoder_delete(dec);
    }
    if (global_state.picture_data) {
        free(global_state.picture_data);
    }
    engine_close_alsa();
}