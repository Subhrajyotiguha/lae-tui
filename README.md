# LAE (Lossless Audio Engine) - TUI

A highly optimized, bit-perfect Linux audio player built with a custom C audio engine and a Rust-based Terminal User Interface (TUI). 

LAE is designed for audiophiles and system administrators who want maximum hardware control and lossless audio fidelity without leaving the terminal.

## Architecture

This project utilizes a hybrid Single-Loop Architecture to maximize low-level hardware performance while maintaining a responsive, memory-safe user interface.

* **The Backend (C):** A custom-written audio engine that directly interfaces with the Linux ALSA subsystem. It handles the heavy lifting: decoding FLAC files, parsing Vorbis comments/metadata, extracting embedded album art, and managing the ring buffer.
* **The Frontend (Rust):** A 60-FPS synchronous terminal interface built with `crossterm`. It communicates with the C backend via a Foreign Function Interface (FFI) to read the playback state, track time, and binary image data.

## The Audio Chain

LAE prioritizes audio fidelity by establishing a direct hardware connection whenever possible, dynamically analyzing the target DAC's capabilities upon boot.

1. **Bit-Perfect ALSA Mode:** If the hardware DAC supports the native bit-depth and sample rate of the source FLAC file (e.g., sending 16-bit / 44.1kHz FLAC data to a 16-bit hardware format), the engine locks into a direct, unaltered bit-perfect stream, entirely bypassing software mixing and resampling.
2. **ALSA Padded Mode:** If the hardware strictly requires a different format (e.g., a 32-bit target for a 16-bit file), the C engine automatically pads the data losslessly to prevent sample distortion.
3. **PipeWire Shared Mode:** For standard desktop usage, the engine seamlessly falls back to the default PipeWire/PulseAudio endpoints.

## Features

* **Bit-Perfect Hardware Playback:** True lossless streaming directly to ALSA devices.
* **Dynamic LRCLIB Integration:** Automatically fetches, caches, and live-syncs LRC lyrics directly to the terminal track timer without requiring asynchronous runtimes.
* **Floating UI Overlays:** Features interactive, layered menus for directory browsing and sorting that do not interrupt the main render loop.
* **In-Terminal Album Art:** Extracts base64 picture data from FLAC headers and renders it directly in GPU-accelerated terminal emulators.
* **Zero-Dependency Playback:** Operates entirely locally once lyrics are cached.

## Keybindings

### Global Controls
| Key | Action |
| :--- | :--- |
| `Space` | Play / Pause |
| `Enter` | Play selected track |
| `Right / f` | Seek forward 5 seconds |
| `Left / b` | Seek backward 5 seconds |
| `Up / Down` | Navigate tracks / Scroll lyrics manually |
| `+ / =` | Increase Volume |
| `-` | Decrease Volume |
| `q` | Quit application |

### Modes & Menus
| Key | Action |
| :--- | :--- |
| `f` | Open Directory Browser (Appends folders dynamically) |
| `S` | Open Search Overlay |
| `Esc` | Open Sort Menu / Close current overlay |
| `s` | Toggle Shuffle Mode |
| `r` | Toggle Repeat Mode |
| `i` | Toggle Auto-Sync Lyrics |

## Building from Source

### Dependencies
Ensure you have standard Linux build tools, the Rust toolchain, and ALSA development headers installed.
* Ubuntu/Debian: `sudo apt install build-essential libasound2-dev`
* Arch Linux: `sudo pacman -S base-devel alsa-lib`

### Compilation
Clone the repository and build the release binary:

```bash
git clone [https://github.com/Subhrajyotiguha/lae-tui.git](https://github.com/Subhrajyotiguha/lae-tui.git)
cd lae-tui
cargo build --release