use super::scaled_sample;
use std::{ffi::c_void, mem, ptr, thread, time::Duration};

type Hwaveout = *mut c_void;
const WAVE_MAPPER: u32 = u32::MAX;
const WAVE_FORMAT_PCM: u16 = 1;
const WAVERR_STILLPLAYING: u32 = 33;

#[repr(C)]
struct WaveFormatEx {
    format_tag: u16,
    channels: u16,
    samples_per_sec: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
    extra_size: u16,
}

#[repr(C)]
struct WaveHdr {
    data: *mut i8,
    buffer_length: u32,
    bytes_recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHdr,
    reserved: usize,
}

#[link(name = "winmm")]
unsafe extern "system" {
    fn waveOutOpen(
        out: *mut Hwaveout,
        device: u32,
        format: *const WaveFormatEx,
        callback: usize,
        instance: usize,
        flags: u32,
    ) -> u32;
    fn waveOutPrepareHeader(out: Hwaveout, header: *mut WaveHdr, size: u32) -> u32;
    fn waveOutWrite(out: Hwaveout, header: *mut WaveHdr, size: u32) -> u32;
    fn waveOutUnprepareHeader(out: Hwaveout, header: *mut WaveHdr, size: u32) -> u32;
    fn waveOutClose(out: Hwaveout) -> u32;
}

fn run(mut pcm: Vec<i16>, channels: u8, rate: u32, volume: f32) {
    for sample in &mut pcm {
        *sample = scaled_sample(*sample, volume);
    }
    let channels16 = u16::from(channels);
    let block_align = channels16.saturating_mul(2);
    let format = WaveFormatEx {
        format_tag: WAVE_FORMAT_PCM,
        channels: channels16,
        samples_per_sec: rate,
        avg_bytes_per_sec: rate.saturating_mul(u32::from(block_align)),
        block_align,
        bits_per_sample: 16,
        extra_size: 0,
    };
    let Ok(byte_len) = u32::try_from(pcm.len().saturating_mul(2)) else {
        return;
    };
    let Ok(header_size) = u32::try_from(mem::size_of::<WaveHdr>()) else {
        return;
    };
    let mut handle: Hwaveout = ptr::null_mut();
    // SAFETY: pointers reference live stack values and WinMM copies the format during open.
    if unsafe { waveOutOpen(&mut handle, WAVE_MAPPER, &format, 0, 0, 0) } != 0 || handle.is_null() {
        return;
    }
    let mut header = WaveHdr {
        data: pcm.as_mut_ptr().cast(),
        buffer_length: byte_len,
        bytes_recorded: 0,
        user: 0,
        flags: 0,
        loops: 0,
        next: ptr::null_mut(),
        reserved: 0,
    };
    // SAFETY: the PCM vector and header stay alive until unprepare succeeds below.
    if unsafe { waveOutPrepareHeader(handle, &mut header, header_size) } == 0
        && unsafe { waveOutWrite(handle, &mut header, header_size) } == 0
    {
        for _ in 0..500 {
            // SAFETY: handle/header are the same live objects passed to prepare/write.
            let result = unsafe { waveOutUnprepareHeader(handle, &mut header, header_size) };
            if result == 0 {
                break;
            }
            if result != WAVERR_STILLPLAYING {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    // SAFETY: handle was returned by waveOutOpen and is no longer used afterwards.
    let _ = unsafe { waveOutClose(handle) };
}

pub(super) fn play(pcm: Vec<i16>, channels: u8, rate: u32, volume: f32) {
    run(pcm, channels, rate, volume);
}
