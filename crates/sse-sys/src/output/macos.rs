use super::scaled_sample;
use std::{ffi::c_void, ptr, thread, time::Duration};

type AudioQueueRef = *mut c_void;
type AudioQueueBufferRef = *mut AudioQueueBuffer;

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioQueueBuffer {
    audio_data_bytes_capacity: u32,
    audio_data: *mut c_void,
    audio_data_byte_size: u32,
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

type OutputCallback = unsafe extern "C" fn(*mut c_void, AudioQueueRef, AudioQueueBufferRef);

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: OutputCallback,
        user: *mut c_void,
        run_loop: *mut c_void,
        mode: *mut c_void,
        flags: u32,
        out: *mut AudioQueueRef,
    ) -> i32;
    fn AudioQueueAllocateBuffer(queue: AudioQueueRef, capacity: u32, out: *mut AudioQueueBufferRef) -> i32;
    fn AudioQueueEnqueueBuffer(
        queue: AudioQueueRef,
        buffer: AudioQueueBufferRef,
        packet_count: u32,
        packets: *const c_void,
    ) -> i32;
    fn AudioQueueStart(queue: AudioQueueRef, start_time: *const c_void) -> i32;
    fn AudioQueueStop(queue: AudioQueueRef, immediate: u8) -> i32;
    fn AudioQueueDispose(queue: AudioQueueRef, immediate: u8) -> i32;
}

unsafe extern "C" fn finished(_user: *mut c_void, _queue: AudioQueueRef, _buffer: AudioQueueBufferRef) {}

fn fourcc(bytes: [u8; 4]) -> u32 {
    u32::from_be_bytes(bytes)
}

fn run(mut pcm: Vec<i16>, channels: u8, rate: u32, volume: f32) {
    for sample in &mut pcm {
        *sample = scaled_sample(*sample, volume);
    }
    let bytes_per_frame = u32::from(channels).saturating_mul(2);
    let format = AudioStreamBasicDescription {
        sample_rate: f64::from(rate),
        format_id: fourcc(*b"lpcm"),
        format_flags: 0x4 | 0x8,
        bytes_per_packet: bytes_per_frame,
        frames_per_packet: 1,
        bytes_per_frame,
        channels_per_frame: u32::from(channels),
        bits_per_channel: 16,
        reserved: 0,
    };
    let Ok(byte_len) = u32::try_from(pcm.len().saturating_mul(2)) else {
        return;
    };
    let mut queue: AudioQueueRef = ptr::null_mut();
    // SAFETY: all pointers are valid for the duration of the call; callback has the documented ABI.
    if unsafe {
        AudioQueueNewOutput(
            &format,
            finished,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            &mut queue,
        )
    } != 0
        || queue.is_null()
    {
        return;
    }
    let mut buffer: AudioQueueBufferRef = ptr::null_mut();
    // SAFETY: queue is a live AudioQueue returned above and out points to writable storage.
    if unsafe { AudioQueueAllocateBuffer(queue, byte_len, &mut buffer) } != 0 || buffer.is_null() {
        // SAFETY: queue is live and owned by this function.
        let _ = unsafe { AudioQueueDispose(queue, 1) };
        return;
    }
    // SAFETY: AudioQueue allocated at least byte_len bytes; source PCM owns exactly byte_len initialized bytes.
    unsafe {
        ptr::copy_nonoverlapping(
            pcm.as_ptr().cast::<u8>(),
            (*buffer).audio_data.cast::<u8>(),
            usize::try_from(byte_len).unwrap_or_default(),
        );
        (*buffer).audio_data_byte_size = byte_len;
    }
    // SAFETY: queue and buffer are live; linear PCM is CBR so packet descriptions are null.
    if unsafe { AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null()) } == 0
        && unsafe { AudioQueueStart(queue, ptr::null()) } == 0
    {
        let frames = pcm.len().checked_div(usize::from(channels)).unwrap_or(0);
        let millis = u64::try_from(frames)
            .unwrap_or(u64::MAX)
            .saturating_mul(1000)
            .checked_div(u64::from(rate))
            .unwrap_or(0);
        thread::sleep(Duration::from_millis(millis.saturating_add(50)));
        // SAFETY: queue is live; immediate stop is appropriate for one-shot teardown after duration elapsed.
        let _ = unsafe { AudioQueueStop(queue, 1) };
    }
    // SAFETY: queue is live and no further access occurs after disposal.
    let _ = unsafe { AudioQueueDispose(queue, 1) };
}

pub(super) fn play(pcm: Vec<i16>, channels: u8, rate: u32, volume: f32) {
    run(pcm, channels, rate, volume);
}
