// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — PCM5102A I2S audio DAC driver (Rust)
//
// Rust port of components/drv_audio_pcm5102a/src/drv_audio_pcm5102a.c.
//
// Drives the PCM5102A DAC over I2S in standard (Philips) mode.
// Initialises the I2S TX channel at 44100 Hz / 16-bit / stereo and provides
// software volume scaling before each write.
//
// Hardware path: ESP-IDF i2s_std API (ESP-IDF ≥ 5.0).
// On non-ESP32 targets (host tests, SDL2 simulator) all I2S calls are stubbed.

use std::os::raw::{c_char, c_void};

use crate::hal_registry::{HalAudioConfig, HalAudioDriver};

// ── ESP error codes ──────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── I2S constants (driver/i2s_types.h, driver/i2s_std.h) ────────────────────

/// I2S_GPIO_UNUSED — GPIO that is not connected.
const I2S_GPIO_UNUSED: i32 = -1;

/// i2s_role_t: I2S_ROLE_MASTER = 0
#[allow(dead_code)]
const I2S_ROLE_MASTER: u32 = 0;

/// i2s_data_bit_width_t values
const I2S_DATA_BIT_WIDTH_8BIT: u32 = 8;
const I2S_DATA_BIT_WIDTH_16BIT: u32 = 16;
const I2S_DATA_BIT_WIDTH_24BIT: u32 = 24;
const I2S_DATA_BIT_WIDTH_32BIT: u32 = 32;

/// i2s_slot_mode_t values
const I2S_SLOT_MODE_MONO: u32 = 1;
const I2S_SLOT_MODE_STEREO: u32 = 2;

/// portMAX_DELAY — maximum ticks to wait in i2s_channel_write
const PORT_MAX_DELAY: u32 = u32::MAX;

// ── I2S config struct layouts ────────────────────────────────────────────────
//
// These reproduce the relevant fields of the ESP-IDF macro-generated structs
// in the order they appear in memory on Xtensa (ESP32-S3).
//
// i2s_chan_config_t (driver/i2s_common.h)
//   id          : i2s_port_t  (i32)
//   role        : i2s_role_t  (u32)
//   dma_desc_num: u32
//   dma_frame_num: u32
//   auto_clear  : bool (+ 3 bytes padding)
//
// I2S_CHANNEL_DEFAULT_CONFIG(id, role) expands to:
//   { .id = id, .role = role, .dma_desc_num = 6, .dma_frame_num = 240,
//     .auto_clear = false }

#[cfg(target_os = "espidf")]
#[repr(C)]
struct I2sChanConfig {
    id: i32,
    role: u32,
    dma_desc_num: u32,
    dma_frame_num: u32,
    auto_clear: bool,
}

// i2s_std_clk_config_t (driver/i2s_std.h)
//   sample_rate_hz: u32
//   clk_src       : i2s_clock_src_t (u32, I2S_CLK_SRC_DEFAULT = 0)
//   ext_clk_freq_hz: u32   (only when clk_src == I2S_CLK_SRC_EXTERNAL)
//   mclk_multiple : i2s_mclk_multiple_t (u32, I2S_MCLK_MULTIPLE_256 = 256)
//
// I2S_STD_CLK_DEFAULT_CONFIG(rate) expands to:
//   { .sample_rate_hz = rate, .clk_src = I2S_CLK_SRC_DEFAULT,
//     .mclk_multiple = I2S_MCLK_MULTIPLE_256 }

#[cfg(target_os = "espidf")]
#[repr(C)]
struct I2sStdClkConfig {
    sample_rate_hz: u32,
    clk_src: u32,        // I2S_CLK_SRC_DEFAULT = 0
    ext_clk_freq_hz: u32,
    mclk_multiple: u32,  // I2S_MCLK_MULTIPLE_256 = 256
}

// i2s_std_slot_config_t (driver/i2s_std.h) — Philips format
//   data_bit_width    : i2s_data_bit_width_t (u32)
//   slot_bit_width    : i2s_slot_bit_width_t (u32, I2S_SLOT_BIT_WIDTH_AUTO = 0)
//   slot_mode         : i2s_slot_mode_t      (u32)
//   slot_mask         : i2s_std_slot_mask_t  (u32, I2S_STD_SLOT_BOTH = 3)
//   ws_width          : u32                  (= data_bit_width)
//   ws_pol            : bool (+ 3 pad)
//   bit_shift         : bool (+ 3 pad)
//   left_align        : bool (+ 3 pad)
//   big_endian        : bool (+ 3 pad)
//   bit_order_lsb     : bool (+ 3 pad)
//
// I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(bits, mode) expands to:
//   { .data_bit_width = bits, .slot_bit_width = I2S_SLOT_BIT_WIDTH_AUTO,
//     .slot_mode = mode, .slot_mask = I2S_STD_SLOT_BOTH,
//     .ws_width = bits, .ws_pol = false, .bit_shift = true,
//     .left_align = false, .big_endian = false, .bit_order_lsb = false }

#[cfg(target_os = "espidf")]
#[repr(C)]
struct I2sStdSlotConfig {
    data_bit_width: u32,
    slot_bit_width: u32,  // I2S_SLOT_BIT_WIDTH_AUTO = 0
    slot_mode: u32,
    slot_mask: u32,       // I2S_STD_SLOT_BOTH = 3
    ws_width: u32,
    ws_pol: bool,
    _pad0: [u8; 3],
    bit_shift: bool,
    _pad1: [u8; 3],
    left_align: bool,
    _pad2: [u8; 3],
    big_endian: bool,
    _pad3: [u8; 3],
    bit_order_lsb: bool,
    _pad4: [u8; 3],
}

// i2s_std_gpio_config_t (driver/i2s_std.h)
//   mclk : gpio_num_t (i32)
//   bclk : gpio_num_t (i32)
//   ws   : gpio_num_t (i32)
//   dout : gpio_num_t (i32)
//   din  : gpio_num_t (i32)
//   invert_flags struct (u32 bitmask — all zero for us)

#[cfg(target_os = "espidf")]
#[repr(C)]
struct I2sStdGpioConfig {
    mclk: i32,
    bclk: i32,
    ws: i32,
    dout: i32,
    din: i32,
    invert_flags: u32,
}

// i2s_std_config_t (driver/i2s_std.h)
//   { clk_cfg, slot_cfg, gpio_cfg }

#[cfg(target_os = "espidf")]
#[repr(C)]
struct I2sStdConfig {
    clk_cfg: I2sStdClkConfig,
    slot_cfg: I2sStdSlotConfig,
    gpio_cfg: I2sStdGpioConfig,
}

// ── ESP-IDF FFI — only compiled on-target ───────────────────────────────────

#[cfg(target_os = "espidf")]
extern "C" {
    /// Allocate a new I2S TX channel.
    fn i2s_new_channel(cfg: *const I2sChanConfig, tx_handle: *mut *mut c_void, rx_handle: *mut *mut c_void) -> i32;

    /// Free a previously allocated I2S channel.
    fn i2s_del_channel(handle: *mut c_void) -> i32;

    /// Configure the channel in standard (Philips) I2S mode.
    fn i2s_channel_init_std_mode(handle: *mut c_void, cfg: *const I2sStdConfig) -> i32;

    /// Enable (start) the channel.
    fn i2s_channel_enable(handle: *mut c_void) -> i32;

    /// Disable (stop) the channel.
    fn i2s_channel_disable(handle: *mut c_void) -> i32;

    /// Write PCM samples to the TX channel.
    fn i2s_channel_write(
        handle: *mut c_void,
        src: *const c_void,
        size: usize,
        bytes_written: *mut usize,
        timeout_ms: u32,
    ) -> i32;

    /// Reconfigure the clock without recreating the channel.
    fn i2s_channel_reconfig_std_clock(handle: *mut c_void, clk_cfg: *const I2sStdClkConfig) -> i32;

    /// Reconfigure the slot format without recreating the channel.
    fn i2s_channel_reconfig_std_slot(handle: *mut c_void, slot_cfg: *const I2sStdSlotConfig) -> i32;
}

// ── Simulator / host stubs ───────────────────────────────────────────────────

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_new_channel(
    _cfg: *const c_void,
    tx_handle: *mut *mut c_void,
    _rx_handle: *mut *mut c_void,
) -> i32 {
    *tx_handle = 1usize as *mut c_void; // non-null sentinel
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_del_channel(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_init_std_mode(_handle: *mut c_void, _cfg: *const c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_enable(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_disable(_handle: *mut c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_write(
    _handle: *mut c_void,
    _src: *const c_void,
    size: usize,
    bytes_written: *mut usize,
    _timeout_ms: u32,
) -> i32 {
    *bytes_written = size; // pretend all bytes were written
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_reconfig_std_clock(_handle: *mut c_void, _clk_cfg: *const c_void) -> i32 {
    ESP_OK
}

#[cfg(not(target_os = "espidf"))]
unsafe fn i2s_channel_reconfig_std_slot(_handle: *mut c_void, _slot_cfg: *const c_void) -> i32 {
    ESP_OK
}

// ── Configuration struct ─────────────────────────────────────────────────────

/// C-compatible configuration for the PCM5102A I2S audio driver.
///
/// Must match `audio_pcm5102a_config_t` in the C header.
#[repr(C)]
pub struct AudioPcm5102aConfig {
    /// I2S port number (i2s_port_t).
    pub i2s_num: i32,
    /// Bit clock GPIO (gpio_num_t).
    pub pin_bck: i32,
    /// Word select GPIO (gpio_num_t).
    pub pin_ws: i32,
    /// Data out GPIO (gpio_num_t).
    pub pin_data: i32,
}

// SAFETY: Config holds only primitive integers.
unsafe impl Send for AudioPcm5102aConfig {}
unsafe impl Sync for AudioPcm5102aConfig {}

// ── Driver state ─────────────────────────────────────────────────────────────

struct AudioState {
    cfg: AudioPcm5102aConfig,
    tx_handle: *mut c_void,
    initialized: bool,
    playing: bool,
    /// Software volume: 0–100.  Applied as a linear scale on a heap copy before
    /// each `i2s_channel_write` call.
    volume: u8,
}

// SAFETY: Mutated only during single-threaded board-init and from the single
// audio playback context, mirroring the C static-state model.
unsafe impl Send for AudioState {}
unsafe impl Sync for AudioState {}

impl AudioState {
    const fn new() -> Self {
        AudioState {
            cfg: AudioPcm5102aConfig {
                i2s_num: 0,
                pin_bck: I2S_GPIO_UNUSED,
                pin_ws: I2S_GPIO_UNUSED,
                pin_data: I2S_GPIO_UNUSED,
            },
            tx_handle: std::ptr::null_mut(),
            initialized: false,
            playing: false,
            volume: 100,
        }
    }
}

static mut S_AUDIO: AudioState = AudioState::new();

// ── Software volume helper ───────────────────────────────────────────────────

/// Scale `data` (16-bit interleaved PCM) by `volume` percent.
///
/// Returns the input unchanged (as an owned copy) when `volume >= 100`.
/// Each sample is multiplied by `volume / 100`, matching the C driver's
/// integer arithmetic exactly.
///
/// This is pure Rust with no hardware dependency — safe to test on any host.
pub fn apply_volume(data: &[u8], volume: u8) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    if volume >= 100 {
        return data.to_vec();
    }

    // Safety: we reinterpret the byte slice as i16 samples.  The caller
    // supplies PCM data that is inherently 16-bit; any trailing odd byte
    // (which would indicate malformed input) is simply truncated here,
    // matching the C behaviour of `len / sizeof(int16_t)`.
    let sample_count = data.len() / 2;
    let samples: &[i16] = unsafe {
        std::slice::from_raw_parts(data.as_ptr() as *const i16, sample_count)
    };

    let scaled: Vec<i16> = samples
        .iter()
        .map(|&s| (s as i32 * volume as i32 / 100) as i16)
        .collect();

    // Convert the scaled i16 slice back to bytes.
    let byte_len = scaled.len() * 2;
    unsafe {
        std::slice::from_raw_parts(scaled.as_ptr() as *const u8, byte_len)
    }
    .to_vec()
}

// ── vtable implementations ───────────────────────────────────────────────────

/// Initialise the PCM5102A I2S channel.
///
/// Creates the I2S TX channel and configures it for 44100 Hz / 16-bit / stereo
/// Philips mode.  The channel is not enabled until the first call to `play`.
///
/// # Safety
/// `config` must point to a valid `AudioPcm5102aConfig`.
unsafe extern "C" fn pcm5102a_init(config: *const c_void) -> i32 {
    if config.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let audio = &mut *(&raw mut S_AUDIO);

    if audio.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let src = &*(config as *const AudioPcm5102aConfig);
    audio.cfg.i2s_num  = src.i2s_num;
    audio.cfg.pin_bck  = src.pin_bck;
    audio.cfg.pin_ws   = src.pin_ws;
    audio.cfg.pin_data = src.pin_data;
    audio.volume  = 100;
    audio.playing = false;

    // Allocate the I2S TX channel.
    #[cfg(target_os = "espidf")]
    {
        let chan_cfg = I2sChanConfig {
            id:            audio.cfg.i2s_num,
            role:          I2S_ROLE_MASTER,
            dma_desc_num:  6,
            dma_frame_num: 240,
            auto_clear:    false,
        };
        let ret = i2s_new_channel(
            &chan_cfg as *const I2sChanConfig,
            &mut audio.tx_handle,
            std::ptr::null_mut(),
        );
        if ret != ESP_OK {
            return ret;
        }
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let ret = i2s_new_channel(
            std::ptr::null(),
            &mut audio.tx_handle,
            std::ptr::null_mut(),
        );
        if ret != ESP_OK {
            return ret;
        }
    }

    // Configure standard Philips mode: 44100 Hz, 16-bit, stereo.
    #[cfg(target_os = "espidf")]
    {
        let std_cfg = I2sStdConfig {
            clk_cfg: I2sStdClkConfig {
                sample_rate_hz: 44100,
                clk_src:        0,   // I2S_CLK_SRC_DEFAULT
                ext_clk_freq_hz: 0,
                mclk_multiple:  256, // I2S_MCLK_MULTIPLE_256
            },
            slot_cfg: I2sStdSlotConfig {
                data_bit_width: I2S_DATA_BIT_WIDTH_16BIT,
                slot_bit_width: 0,                  // I2S_SLOT_BIT_WIDTH_AUTO
                slot_mode:      I2S_SLOT_MODE_STEREO,
                slot_mask:      3,                  // I2S_STD_SLOT_BOTH
                ws_width:       I2S_DATA_BIT_WIDTH_16BIT,
                ws_pol:         false,
                _pad0:          [0u8; 3],
                bit_shift:      true,               // Philips: 1-bit shift
                _pad1:          [0u8; 3],
                left_align:     false,
                _pad2:          [0u8; 3],
                big_endian:     false,
                _pad3:          [0u8; 3],
                bit_order_lsb:  false,
                _pad4:          [0u8; 3],
            },
            gpio_cfg: I2sStdGpioConfig {
                mclk:         I2S_GPIO_UNUSED, // PCM5102A derives MCLK from BCK
                bclk:         audio.cfg.pin_bck,
                ws:           audio.cfg.pin_ws,
                dout:         audio.cfg.pin_data,
                din:          I2S_GPIO_UNUSED,
                invert_flags: 0,
            },
        };
        let ret = i2s_channel_init_std_mode(
            audio.tx_handle,
            &std_cfg as *const I2sStdConfig,
        );
        if ret != ESP_OK {
            i2s_del_channel(audio.tx_handle);
            audio.tx_handle = std::ptr::null_mut();
            return ret;
        }
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let ret = i2s_channel_init_std_mode(audio.tx_handle, std::ptr::null());
        if ret != ESP_OK {
            i2s_del_channel(audio.tx_handle);
            audio.tx_handle = std::ptr::null_mut();
            return ret;
        }
    }

    audio.initialized = true;
    ESP_OK
}

/// De-initialise the PCM5102A driver and release the I2S channel.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn pcm5102a_deinit() {
    let audio = &mut *(&raw mut S_AUDIO);

    if !audio.initialized {
        return;
    }

    if audio.playing {
        i2s_channel_disable(audio.tx_handle);
        audio.playing = false;
    }

    i2s_del_channel(audio.tx_handle);
    audio.tx_handle  = std::ptr::null_mut();
    audio.initialized = false;
}

/// Write PCM audio data to the DAC.
///
/// On the first call after init (or after `stop`) the channel is enabled.
/// A software volume copy is made when `volume < 100`; the caller's buffer
/// is never modified.
///
/// # Safety
/// `data` must be a valid pointer to `len` bytes of 16-bit PCM samples.
unsafe extern "C" fn pcm5102a_play(data: *const u8, len: usize) -> i32 {
    let audio = &mut *(&raw mut S_AUDIO);

    if !audio.initialized {
        return ESP_ERR_INVALID_STATE;
    }
    if data.is_null() || len == 0 {
        return ESP_ERR_INVALID_ARG;
    }

    // Enable the channel on the first play call.
    if !audio.playing {
        let ret = i2s_channel_enable(audio.tx_handle);
        if ret != ESP_OK {
            return ret;
        }
        audio.playing = true;
    }

    // Apply software volume on a heap copy so the caller's buffer is safe.
    let raw_slice = std::slice::from_raw_parts(data, len);
    let scaled = apply_volume(raw_slice, audio.volume);

    let mut bytes_written: usize = 0;
    let ret = i2s_channel_write(
        audio.tx_handle,
        scaled.as_ptr() as *const c_void,
        scaled.len(),
        &mut bytes_written,
        PORT_MAX_DELAY,
    );

    if ret != ESP_OK {
        return ret;
    }

    // A short write is non-fatal (matches C behaviour — logs a warning but
    // returns ESP_OK).
    ESP_OK
}

/// Stop audio playback by disabling the I2S channel.
///
/// The channel remains allocated; the next call to `play` will re-enable it.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn pcm5102a_stop() -> i32 {
    let audio = &mut *(&raw mut S_AUDIO);

    if !audio.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    if audio.playing {
        let ret = i2s_channel_disable(audio.tx_handle);
        if ret != ESP_OK {
            return ret;
        }
        audio.playing = false;
    }

    ESP_OK
}

/// Set the software volume (0–100 %).
///
/// Values > 100 are clamped to 100.  The new volume takes effect on the next
/// call to `play`.
///
/// # Safety
/// Called from C via the HAL vtable.
unsafe extern "C" fn pcm5102a_set_volume(percent: u8) -> i32 {
    let audio = &mut *(&raw mut S_AUDIO);
    audio.volume = if percent > 100 { 100 } else { percent };
    ESP_OK
}

/// Reconfigure the I2S channel for a different sample rate, bit depth, or
/// channel count at runtime.
///
/// The channel is disabled before reconfiguration and must be re-enabled by
/// calling `play` again.
///
/// # Safety
/// `cfg` must point to a valid `HalAudioConfig`.
unsafe extern "C" fn pcm5102a_configure(cfg: *const HalAudioConfig) -> i32 {
    if cfg.is_null() {
        return ESP_ERR_INVALID_ARG;
    }

    let audio = &mut *(&raw mut S_AUDIO);

    if !audio.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Disable before reconfiguring (required by ESP-IDF).
    if audio.playing {
        i2s_channel_disable(audio.tx_handle);
        audio.playing = false;
    }

    let src = &*cfg;

    let bit_width: u32 = match src.bits_per_sample {
        8  => I2S_DATA_BIT_WIDTH_8BIT,
        16 => I2S_DATA_BIT_WIDTH_16BIT,
        24 => I2S_DATA_BIT_WIDTH_24BIT,
        32 => I2S_DATA_BIT_WIDTH_32BIT,
        _  => return ESP_ERR_INVALID_ARG,
    };

    let slot_mode: u32 = if src.channels == 1 {
        I2S_SLOT_MODE_MONO
    } else {
        I2S_SLOT_MODE_STEREO
    };

    #[cfg(target_os = "espidf")]
    {
        let clk_cfg = I2sStdClkConfig {
            sample_rate_hz:  src.sample_rate,
            clk_src:         0,   // I2S_CLK_SRC_DEFAULT
            ext_clk_freq_hz: 0,
            mclk_multiple:   256, // I2S_MCLK_MULTIPLE_256
        };
        let ret = i2s_channel_reconfig_std_clock(
            audio.tx_handle,
            &clk_cfg as *const I2sStdClkConfig,
        );
        if ret != ESP_OK {
            return ret;
        }

        let slot_cfg = I2sStdSlotConfig {
            data_bit_width: bit_width,
            slot_bit_width: 0,          // I2S_SLOT_BIT_WIDTH_AUTO
            slot_mode,
            slot_mask:      3,          // I2S_STD_SLOT_BOTH
            ws_width:       bit_width,
            ws_pol:         false,
            _pad0:          [0u8; 3],
            bit_shift:      true,       // Philips format
            _pad1:          [0u8; 3],
            left_align:     false,
            _pad2:          [0u8; 3],
            big_endian:     false,
            _pad3:          [0u8; 3],
            bit_order_lsb:  false,
            _pad4:          [0u8; 3],
        };
        let ret = i2s_channel_reconfig_std_slot(
            audio.tx_handle,
            &slot_cfg as *const I2sStdSlotConfig,
        );
        if ret != ESP_OK {
            return ret;
        }
    }
    #[cfg(not(target_os = "espidf"))]
    {
        let _ = (bit_width, slot_mode);
        let ret = i2s_channel_reconfig_std_clock(audio.tx_handle, std::ptr::null());
        if ret != ESP_OK {
            return ret;
        }
        let ret = i2s_channel_reconfig_std_slot(audio.tx_handle, std::ptr::null());
        if ret != ESP_OK {
            return ret;
        }
    }

    ESP_OK
}

// ── HAL vtable ───────────────────────────────────────────────────────────────

/// Static HAL audio driver vtable for the PCM5102A.
///
/// Pass to `hal_audio_register()`.  Returned by `drv_audio_pcm5102a_get()`.
static AUDIO_DRIVER: HalAudioDriver = HalAudioDriver {
    init:       Some(pcm5102a_init),
    deinit:     Some(pcm5102a_deinit),
    play:       Some(pcm5102a_play),
    stop:       Some(pcm5102a_stop),
    set_volume: Some(pcm5102a_set_volume),
    configure:  Some(pcm5102a_configure),
    name:       b"PCM5102A\0".as_ptr() as *const c_char,
};

/// Return the PCM5102A audio driver vtable.
///
/// Drop-in replacement for the C `drv_audio_pcm5102a_get()`.
///
/// # Safety
/// Returns a pointer to a program-lifetime static — safe to call from C.
#[no_mangle]
pub extern "C" fn drv_audio_pcm5102a_get() -> *const HalAudioDriver {
    &AUDIO_DRIVER
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset global driver state between tests.
    unsafe fn reset_state() {
        *(&raw mut S_AUDIO) = AudioState::new();
    }

    // ── apply_volume ─────────────────────────────────────────────────────────

    #[test]
    fn test_apply_volume_100_is_identity() {
        // At 100 % volume the samples must be returned bit-for-bit identical.
        let input: Vec<i16> = vec![0, 100, -100, i16::MAX, i16::MIN];
        let bytes: Vec<u8> = input
            .iter()
            .flat_map(|&s| s.to_ne_bytes())
            .collect();
        let out = apply_volume(&bytes, 100);
        assert_eq!(out, bytes, "volume 100 must be an identity transform");
    }

    #[test]
    fn test_apply_volume_0_is_silence() {
        // At 0 % every sample must become 0.
        let input: Vec<i16> = vec![1000, -1000, 32000, -32000];
        let bytes: Vec<u8> = input.iter().flat_map(|&s| s.to_ne_bytes()).collect();
        let out = apply_volume(&bytes, 0);
        let result_samples: Vec<i16> = out
            .chunks_exact(2)
            .map(|c| i16::from_ne_bytes([c[0], c[1]]))
            .collect();
        for &s in &result_samples {
            assert_eq!(s, 0, "all samples must be zero at volume 0");
        }
    }

    #[test]
    fn test_apply_volume_50_halves_samples() {
        // At 50 % each sample should be halved (integer division, truncated toward zero).
        let input: Vec<i16> = vec![1000, -1000, 100, -100, 1, -1];
        let bytes: Vec<u8> = input.iter().flat_map(|&s| s.to_ne_bytes()).collect();
        let out = apply_volume(&bytes, 50);
        let result_samples: Vec<i16> = out
            .chunks_exact(2)
            .map(|c| i16::from_ne_bytes([c[0], c[1]]))
            .collect();
        for (&orig, &scaled) in input.iter().zip(result_samples.iter()) {
            let expected = (orig as i32 * 50 / 100) as i16;
            assert_eq!(
                scaled, expected,
                "at volume 50: sample {} should be {}, got {}",
                orig, expected, scaled
            );
        }
    }

    #[test]
    fn test_apply_volume_empty_input() {
        // Empty slice must return an empty Vec without panicking.
        let out = apply_volume(&[], 50);
        assert!(out.is_empty());
    }

    #[test]
    fn test_apply_volume_above_100_acts_as_identity() {
        // Volume > 100 triggers the fast path (same as 100).
        let input: Vec<i16> = vec![500, -500];
        let bytes: Vec<u8> = input.iter().flat_map(|&s| s.to_ne_bytes()).collect();
        let out_101 = apply_volume(&bytes, 101);
        let out_255 = apply_volume(&bytes, 255);
        assert_eq!(out_101, bytes);
        assert_eq!(out_255, bytes);
    }

    #[test]
    fn test_apply_volume_preserves_length() {
        // Output length must always equal input length (rounded down to 2-byte boundary).
        let bytes = vec![0u8; 10];
        for vol in [0u8, 25, 50, 75, 100] {
            let out = apply_volume(&bytes, vol);
            // apply_volume processes 5 samples (10 / 2) and returns 10 bytes.
            assert_eq!(out.len(), 10, "length mismatch at volume {}", vol);
        }
    }

    // ── Vtable ────────────────────────────────────────────────────────────────

    #[test]
    fn test_vtable_pointer_non_null() {
        assert!(!drv_audio_pcm5102a_get().is_null());
    }

    #[test]
    fn test_vtable_fields_populated() {
        let drv = unsafe { &*drv_audio_pcm5102a_get() };
        assert!(drv.init.is_some(),       "init must be Some");
        assert!(drv.deinit.is_some(),     "deinit must be Some");
        assert!(drv.play.is_some(),       "play must be Some");
        assert!(drv.stop.is_some(),       "stop must be Some");
        assert!(drv.set_volume.is_some(), "set_volume must be Some");
        assert!(drv.configure.is_some(),  "configure must be Some");
        assert!(!drv.name.is_null(),      "name must not be null");
    }

    #[test]
    fn test_vtable_name_is_pcm5102a() {
        let drv = unsafe { &*drv_audio_pcm5102a_get() };
        let name = unsafe { std::ffi::CStr::from_ptr(drv.name) };
        assert_eq!(name.to_str().unwrap(), "PCM5102A");
    }

    // ── init / deinit ─────────────────────────────────────────────────────────

    #[test]
    fn test_init_null_config_returns_invalid_arg() {
        unsafe {
            reset_state();
            assert_eq!(pcm5102a_init(std::ptr::null()), ESP_ERR_INVALID_ARG);
            assert!(!(*(&raw const S_AUDIO)).initialized);
        }
    }

    #[test]
    fn test_init_and_deinit_cycle() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig {
                i2s_num: 0,
                pin_bck: 41,
                pin_ws: 45,
                pin_data: 42,
            };
            let ret = pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            assert_eq!(ret, ESP_OK);
            assert!((*(&raw const S_AUDIO)).initialized);
            assert!(!(*(&raw const S_AUDIO)).tx_handle.is_null());

            pcm5102a_deinit();
            assert!(!(*(&raw const S_AUDIO)).initialized);
            assert!((*(&raw const S_AUDIO)).tx_handle.is_null());
        }
    }

    #[test]
    fn test_double_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig {
                i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42,
            };
            let p = &cfg as *const AudioPcm5102aConfig as *const c_void;
            assert_eq!(pcm5102a_init(p), ESP_OK);
            assert_eq!(pcm5102a_init(p), ESP_ERR_INVALID_STATE);
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_deinit_noop_when_not_initialized() {
        unsafe {
            reset_state();
            pcm5102a_deinit(); // must not panic
            assert!(!(*(&raw const S_AUDIO)).initialized);
        }
    }

    // ── play ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_play_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let buf = [0u8; 16];
            assert_eq!(pcm5102a_play(buf.as_ptr(), buf.len()), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_play_null_data_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            assert_eq!(pcm5102a_play(std::ptr::null(), 16), ESP_ERR_INVALID_ARG);
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_play_zero_len_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            let buf = [0u8; 16];
            assert_eq!(pcm5102a_play(buf.as_ptr(), 0), ESP_ERR_INVALID_ARG);
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_play_enables_channel_on_first_call() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            assert!(!(*(&raw const S_AUDIO)).playing);

            let buf = [0u8; 16];
            assert_eq!(pcm5102a_play(buf.as_ptr(), buf.len()), ESP_OK);
            assert!((*(&raw const S_AUDIO)).playing);
            pcm5102a_deinit();
        }
    }

    // ── stop ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_stop_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            assert_eq!(pcm5102a_stop(), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_stop_when_not_playing_is_ok() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            assert_eq!(pcm5102a_stop(), ESP_OK); // not playing — no-op
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_play_then_stop_clears_playing_flag() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            let buf = [0u8; 16];
            pcm5102a_play(buf.as_ptr(), buf.len());
            assert!((*(&raw const S_AUDIO)).playing);
            assert_eq!(pcm5102a_stop(), ESP_OK);
            assert!(!(*(&raw const S_AUDIO)).playing);
            pcm5102a_deinit();
        }
    }

    // ── set_volume ────────────────────────────────────────────────────────────

    #[test]
    fn test_set_volume_stores_value() {
        unsafe {
            reset_state();
            assert_eq!(pcm5102a_set_volume(75), ESP_OK);
            assert_eq!((*(&raw const S_AUDIO)).volume, 75);
        }
    }

    #[test]
    fn test_set_volume_clamps_above_100() {
        unsafe {
            reset_state();
            assert_eq!(pcm5102a_set_volume(200), ESP_OK);
            assert_eq!((*(&raw const S_AUDIO)).volume, 100);
        }
    }

    #[test]
    fn test_set_volume_zero() {
        unsafe {
            reset_state();
            assert_eq!(pcm5102a_set_volume(0), ESP_OK);
            assert_eq!((*(&raw const S_AUDIO)).volume, 0);
        }
    }

    // ── configure ─────────────────────────────────────────────────────────────

    #[test]
    fn test_configure_null_returns_invalid_arg() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            assert_eq!(pcm5102a_configure(std::ptr::null()), ESP_ERR_INVALID_ARG);
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_configure_before_init_returns_invalid_state() {
        unsafe {
            reset_state();
            let hal_cfg = HalAudioConfig { sample_rate: 48000, bits_per_sample: 16, channels: 2 };
            assert_eq!(pcm5102a_configure(&hal_cfg), ESP_ERR_INVALID_STATE);
        }
    }

    #[test]
    fn test_configure_invalid_bits_per_sample() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            let hal_cfg = HalAudioConfig { sample_rate: 44100, bits_per_sample: 7, channels: 2 };
            assert_eq!(pcm5102a_configure(&hal_cfg), ESP_ERR_INVALID_ARG);
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_configure_valid_params_returns_ok() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);
            for &(rate, bits, ch) in &[
                (44100u32, 16u8, 2u8),
                (48000,    16,   2),
                (22050,    16,   1),
                (44100,     8,   2),
                (44100,    24,   2),
                (44100,    32,   2),
            ] {
                let hal_cfg = HalAudioConfig {
                    sample_rate: rate,
                    bits_per_sample: bits,
                    channels: ch,
                };
                assert_eq!(
                    pcm5102a_configure(&hal_cfg),
                    ESP_OK,
                    "configure({rate} Hz, {bits}-bit, {ch}ch) must succeed"
                );
            }
            pcm5102a_deinit();
        }
    }

    #[test]
    fn test_configure_while_playing_disables_channel() {
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void);

            let buf = [0u8; 16];
            pcm5102a_play(buf.as_ptr(), buf.len());
            assert!((*(&raw const S_AUDIO)).playing);

            let hal_cfg = HalAudioConfig { sample_rate: 48000, bits_per_sample: 16, channels: 2 };
            assert_eq!(pcm5102a_configure(&hal_cfg), ESP_OK);
            // configure must have stopped the channel.
            assert!(!(*(&raw const S_AUDIO)).playing);
            pcm5102a_deinit();
        }
    }

    // ── full lifecycle ─────────────────────────────────────────────────────────

    #[test]
    fn test_full_lifecycle() {
        // init → set_volume → play → stop → configure → play → deinit
        unsafe {
            reset_state();
            let cfg = AudioPcm5102aConfig { i2s_num: 0, pin_bck: 41, pin_ws: 45, pin_data: 42 };
            assert_eq!(
                pcm5102a_init(&cfg as *const AudioPcm5102aConfig as *const c_void),
                ESP_OK
            );
            assert_eq!(pcm5102a_set_volume(80), ESP_OK);

            let buf: Vec<u8> = (0u16..8).flat_map(|s| s.to_ne_bytes()).collect();
            assert_eq!(pcm5102a_play(buf.as_ptr(), buf.len()), ESP_OK);
            assert_eq!(pcm5102a_stop(), ESP_OK);

            let hal_cfg = HalAudioConfig { sample_rate: 48000, bits_per_sample: 16, channels: 1 };
            assert_eq!(pcm5102a_configure(&hal_cfg), ESP_OK);

            assert_eq!(pcm5102a_play(buf.as_ptr(), buf.len()), ESP_OK);
            pcm5102a_deinit();
            assert!(!(*(&raw const S_AUDIO)).initialized);
        }
    }
}
