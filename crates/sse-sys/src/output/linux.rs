use super::scaled_sample;
use super::stream::{chunk_duration, ChunkSink};
use sse_core::{Error, Result};
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

const COMMAND_REPLY: u32 = 2;
const COMMAND_CREATE_PLAYBACK_STREAM: u32 = 3;
const COMMAND_DELETE_PLAYBACK_STREAM: u32 = 4;
const COMMAND_SET_CLIENT_NAME: u32 = 9;
const COMMAND_AUTH: u32 = 8;
const COMMAND_DRAIN_PLAYBACK_STREAM: u32 = 12;
const PROTOCOL_VERSION: u32 = 12;
const INVALID_INDEX: u32 = u32::MAX;
const VOLUME_NORM: u32 = 0x1_0000;
const MAX_PACKET: usize = 64 * 1024;

/// Byte transport used by the PulseAudio native protocol codec.
pub trait PulseTransport {
    /// Sends all bytes.
    fn send(&mut self, bytes: &[u8]) -> Result<()>;
    /// Receives exactly the requested number of bytes.
    fn receive_exact(&mut self, bytes: &mut [u8]) -> Result<()>;
}

pub(super) struct SocketTransport(UnixStream);

impl PulseTransport for SocketTransport {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.0.write_all(bytes).map_err(Error::from)
    }

    fn receive_exact(&mut self, bytes: &mut [u8]) -> Result<()> {
        self.0.read_exact(bytes).map_err(Error::from)
    }
}

#[derive(Default)]
struct Tags {
    bytes: Vec<u8>,
}

impl Tags {
    fn u32(&mut self, value: u32) {
        self.bytes.push(b'L');
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.bytes.push(b't');
                self.bytes.extend_from_slice(value.as_bytes());
                self.bytes.push(0);
            }
            None => self.bytes.push(b'N'),
        }
    }

    fn arbitrary(&mut self, value: &[u8]) {
        // Tag, then the length as a bare big-endian u32 (no tag of its own), then the data.
        self.bytes.push(b'x');
        let length = u32::try_from(value.len()).unwrap_or(u32::MAX);
        self.bytes.extend_from_slice(&length.to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn boolean(&mut self, value: bool) {
        self.bytes.push(if value { b'1' } else { b'0' });
    }

    fn sample_spec(&mut self, channels: u8, rate: u32) {
        // Tag, format (3 = signed 16-bit little-endian), channel count, then the rate as a bare big-endian u32.
        self.bytes.push(b'a');
        self.bytes.push(3);
        self.bytes.push(channels);
        self.bytes.extend_from_slice(&rate.to_be_bytes());
    }

    fn channel_map(&mut self, channels: u8) {
        self.bytes.push(b'm');
        self.bytes.push(channels);
        for channel in 0..channels {
            let position = match (channels, channel) {
                (1, 0) => 0,
                (_, 0) => 1,
                (_, 1) => 2,
                _ => 0,
            };
            self.bytes.push(position);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn cvolume(&mut self, channels: u8, volume: f32) {
        self.bytes.push(b'v');
        self.bytes.push(channels);
        let scaled = (VOLUME_NORM as f32 * volume.clamp(0.0, 1.0)).round() as u32;
        for _ in 0..channels {
            self.bytes.extend_from_slice(&scaled.to_be_bytes());
        }
    }
}

fn packet(payload: &[u8]) -> Result<Vec<u8>> {
    let length = u32::try_from(payload.len()).map_err(|_| Error::Refused("PulseAudio packet too large".to_owned()))?;
    let mut out = Vec::with_capacity(20usize.saturating_add(payload.len()));
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&u32::MAX.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

fn memblock(channel: u32, payload: &[u8]) -> Result<Vec<u8>> {
    let length =
        u32::try_from(payload.len()).map_err(|_| Error::Refused("PulseAudio audio block too large".to_owned()))?;
    let mut out = Vec::with_capacity(20usize.saturating_add(payload.len()));
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&channel.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

fn read_frame<T: PulseTransport>(transport: &mut T) -> Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 20];
    transport.receive_exact(&mut header)?;
    let length = u32::from_be_bytes(
        header
            .get(0..4)
            .and_then(|value| <[u8; 4]>::try_from(value).ok())
            .ok_or_else(|| Error::damaged("PulseAudio frame length"))?,
    );
    let channel = u32::from_be_bytes(
        header
            .get(4..8)
            .and_then(|value| <[u8; 4]>::try_from(value).ok())
            .ok_or_else(|| Error::damaged("PulseAudio frame channel"))?,
    );
    let length = usize::try_from(length).map_err(|_| Error::damaged("PulseAudio frame size"))?;
    if length > MAX_PACKET {
        return Err(Error::Refused("PulseAudio control packet too large".to_owned()));
    }
    let mut body = vec![0u8; length];
    transport.receive_exact(&mut body)?;
    Ok((channel, body))
}

fn tagged_u32(data: &[u8], offset: &mut usize) -> Result<u32> {
    if data.get(*offset) != Some(&b'L') {
        return Err(Error::damaged("PulseAudio u32 tag"));
    }
    *offset = offset
        .checked_add(1)
        .ok_or_else(|| Error::damaged("PulseAudio offset"))?;
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("PulseAudio offset"))?;
    let bytes = data
        .get(*offset..end)
        .ok_or_else(|| Error::damaged("short PulseAudio u32"))?;
    *offset = end;
    Ok(u32::from_be_bytes(
        <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("PulseAudio u32"))?,
    ))
}

fn reply_for<T: PulseTransport>(transport: &mut T, wanted_tag: u32) -> Result<Vec<u8>> {
    loop {
        let (channel, body) = read_frame(transport)?;
        if channel != u32::MAX {
            continue;
        }
        let mut offset = 0usize;
        let command = tagged_u32(&body, &mut offset)?;
        let tag = tagged_u32(&body, &mut offset)?;
        if tag != wanted_tag {
            continue;
        }
        if command != COMMAND_REPLY {
            let code = tagged_u32(&body, &mut offset).unwrap_or(u32::MAX);
            return Err(Error::System(format!(
                "PulseAudio rejected command {wanted_tag} with error {code}"
            )));
        }
        return Ok(body.get(offset..).unwrap_or_default().to_vec());
    }
}

fn send_command<T: PulseTransport>(transport: &mut T, command: u32, tag: u32, tail: Tags) -> Result<Vec<u8>> {
    let mut tags = Tags::default();
    tags.u32(command);
    tags.u32(tag);
    tags.bytes.extend_from_slice(&tail.bytes);
    transport.send(&packet(&tags.bytes)?)?;
    reply_for(transport, tag)
}

fn auth<T: PulseTransport>(transport: &mut T, cookie: &[u8; 256]) -> Result<()> {
    let mut tail = Tags::default();
    tail.u32(PROTOCOL_VERSION);
    tail.arbitrary(cookie);
    let reply = send_command(transport, COMMAND_AUTH, 1, tail)?;
    let mut offset = 0usize;
    let server_version = tagged_u32(&reply, &mut offset)? & 0x3fff_ffff;
    if server_version < 8 {
        return Err(Error::Refused("PulseAudio protocol older than v8".to_owned()));
    }
    Ok(())
}

/// Names this connection so the server can show and manage it; clients send this before creating streams.
fn set_client_name<T: PulseTransport>(transport: &mut T, name: &str) -> Result<()> {
    let mut tail = Tags::default();
    // Property list: key, length of the value, then the value as arbitrary bytes; the list ends with a null.
    tail.string(Some("application.name"));
    tail.u32(u32::try_from(name.len()).unwrap_or(u32::MAX));
    tail.arbitrary(name.as_bytes());
    tail.string(None);
    let _ = send_command(transport, COMMAND_SET_CLIENT_NAME, 3, tail)?;
    Ok(())
}

fn create_stream<T: PulseTransport>(transport: &mut T, channels: u8, rate: u32, volume: f32) -> Result<(u32, u32)> {
    let mut tail = Tags::default();
    tail.string(Some("S.T.A.L.K.E.R. Save Editor UI"));
    tail.sample_spec(channels, rate);
    tail.channel_map(channels);
    tail.u32(INVALID_INDEX);
    tail.string(None);
    tail.u32(u32::MAX);
    tail.boolean(false);
    tail.u32(u32::MAX);
    tail.u32(u32::MAX);
    tail.u32(u32::MAX);
    tail.u32(0);
    tail.cvolume(channels, volume);
    for _ in 0..7 {
        tail.boolean(false);
    }
    let reply = send_command(transport, COMMAND_CREATE_PLAYBACK_STREAM, 2, tail)?;
    let mut offset = 0usize;
    let stream_index = tagged_u32(&reply, &mut offset)?;
    let channel = tagged_u32(&reply, &mut offset)?;
    Ok((stream_index, channel))
}

fn drain<T: PulseTransport>(transport: &mut T, stream_index: u32) -> Result<()> {
    let mut tail = Tags::default();
    tail.u32(stream_index);
    let _ = send_command(transport, COMMAND_DRAIN_PLAYBACK_STREAM, 3, tail)?;
    Ok(())
}

fn cookie() -> [u8; 256] {
    let mut cookie = [0u8; 256];
    let mut paths = Vec::new();
    if let Some(path) = env::var_os("PULSE_COOKIE") {
        paths.push(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(path).join("pulse/cookie"));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.push(home.join(".config/pulse/cookie"));
        paths.push(home.join(".pulse-cookie"));
    }
    for path in paths {
        if let Ok(bytes) = fs::read(path) {
            if let Some(source) = bytes.get(..256) {
                cookie.copy_from_slice(source);
                break;
            }
        }
    }
    cookie
}

fn socket_path() -> Option<PathBuf> {
    if let Ok(server) = env::var("PULSE_SERVER") {
        if let Some(path) = server.strip_prefix("unix:") {
            return Some(PathBuf::from(path));
        }
        if server.starts_with('/') {
            return Some(PathBuf::from(server));
        }
    }
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("pulse/native"))
}

fn run(pcm: &[i16], channels: u8, rate: u32, volume: f32) -> Result<()> {
    let path = socket_path().ok_or_else(|| Error::System("PulseAudio runtime socket is unavailable".to_owned()))?;
    let stream = UnixStream::connect(&path)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut transport = SocketTransport(stream);
    auth(&mut transport, &cookie())?;
    let (stream_index, channel) = create_stream(&mut transport, channels, rate, volume)?;
    let mut bytes = Vec::with_capacity(pcm.len().saturating_mul(2));
    for sample in pcm {
        bytes.extend_from_slice(&scaled_sample(*sample, 1.0).to_le_bytes());
    }
    for chunk in bytes.chunks(32 * 1024) {
        transport.send(&memblock(channel, chunk)?)?;
    }
    drain(&mut transport, stream_index)
}

pub(super) fn play(pcm: Vec<i16>, channels: u8, rate: u32, volume: f32) {
    let _ = run(&pcm, channels, rate, volume);
}

/// One PulseAudio playback stream on its own connection, fed chunk by chunk for looping music.
pub(super) struct MusicStream<T: PulseTransport = SocketTransport> {
    transport: T,
    stream_index: u32,
    channel: u32,
    channels: u8,
    rate: u32,
}

impl MusicStream {
    /// Connects, authenticates and creates the stream. Failure means no music, never a panic.
    pub(super) fn open(channels: u8, rate: u32, volume: f32) -> Result<Self> {
        let path = socket_path().ok_or_else(|| Error::System("PulseAudio runtime socket is unavailable".to_owned()))?;
        let stream = UnixStream::connect(&path)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        establish(SocketTransport(stream), &cookie(), channels, rate, volume)
    }
}

/// Authenticates over `transport` and creates a playback stream on it.
fn establish<T: PulseTransport>(
    mut transport: T,
    cookie: &[u8; 256],
    channels: u8,
    rate: u32,
    volume: f32,
) -> Result<MusicStream<T>> {
    auth(&mut transport, cookie)?;
    set_client_name(&mut transport, "S.T.A.L.K.E.R. Save Editor UI")?;
    let (stream_index, channel) = create_stream(&mut transport, channels, rate, volume)?;
    Ok(MusicStream {
        transport,
        stream_index,
        channel,
        channels,
        rate,
    })
}

impl<T: PulseTransport + Send + 'static> ChunkSink for MusicStream<T> {
    fn write(&mut self, samples: &[i16]) -> bool {
        let mut bytes = Vec::with_capacity(samples.len().saturating_mul(2));
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        for chunk in bytes.chunks(32 * 1024) {
            let Ok(frame) = memblock(self.channel, chunk) else {
                return false;
            };
            if self.transport.send(&frame).is_err() {
                return false;
            }
        }
        let frames = samples.len().checked_div(usize::from(self.channels)).unwrap_or(0);
        std::thread::sleep(chunk_duration(frames, self.rate));
        true
    }
}

impl<T: PulseTransport> Drop for MusicStream<T> {
    fn drop(&mut self) {
        // The server removes the stream from the mixer when it is deleted; the connection then closes.
        let _ = delete_stream(&mut self.transport, self.stream_index);
    }
}

/// Removes a playback stream by index, so that it leaves the mixer.
fn delete_stream<T: PulseTransport>(transport: &mut T, stream_index: u32) -> Result<()> {
    let mut tail = Tags::default();
    tail.u32(stream_index);
    let _ = send_command(transport, COMMAND_DELETE_PLAYBACK_STREAM, 4, tail)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        sent: Vec<Vec<u8>>,
        recv: VecDeque<u8>,
    }

    impl PulseTransport for Fake {
        fn send(&mut self, bytes: &[u8]) -> Result<()> {
            self.sent.push(bytes.to_vec());
            Ok(())
        }

        fn receive_exact(&mut self, bytes: &mut [u8]) -> Result<()> {
            for slot in bytes {
                *slot = self
                    .recv
                    .pop_front()
                    .ok_or_else(|| Error::damaged("fake transport exhausted"))?;
            }
            Ok(())
        }
    }

    fn control_frame(command: u32, tag: u32, tail: &[u8]) -> Vec<u8> {
        let mut tags = Tags::default();
        tags.u32(command);
        tags.u32(tag);
        tags.bytes.extend_from_slice(tail);
        packet(&tags.bytes).unwrap_or_default()
    }

    #[test]
    fn tagstruct_encoding_matches_native_wire_shapes() {
        let mut tags = Tags::default();
        tags.u32(0x0102_0304);
        tags.string(Some("x"));
        tags.boolean(true);
        tags.sample_spec(2, 48_000);
        assert_eq!(tags.bytes.get(0..5), Some(&[b'L', 1, 2, 3, 4][..]));
        assert_eq!(tags.bytes.get(5..8), Some(&[b't', b'x', 0][..]));
        assert_eq!(tags.bytes.get(8), Some(&b'1'));
        assert_eq!(tags.bytes.get(9..16), Some(&[b'a', 3, 2, 0, 0, 0xbb, 0x80][..]));
    }

    #[test]
    fn packet_descriptor_is_twenty_big_endian_bytes() {
        let framed = packet(b"abc").unwrap_or_default();
        assert_eq!(framed.len(), 23);
        assert_eq!(framed.get(0..4), Some(&3u32.to_be_bytes()[..]));
        assert_eq!(framed.get(4..8), Some(&u32::MAX.to_be_bytes()[..]));
        assert_eq!(framed.get(20..), Some(&b"abc"[..]));
    }

    #[test]
    fn auth_accepts_hand_built_reply() {
        let mut tail = Tags::default();
        tail.u32(PROTOCOL_VERSION);
        let mut fake = Fake::default();
        fake.recv.extend(control_frame(COMMAND_REPLY, 1, &tail.bytes));
        assert!(auth(&mut fake, &[7u8; 256]).is_ok());
        let first = fake.sent.first().cloned().unwrap_or_default();
        assert!(first
            .windows(5)
            .any(|window| { window == [b'L', 0, 0, 0, u8::try_from(COMMAND_AUTH).unwrap_or_default(),] }));
        assert!(first.windows(261).any(|window| {
            window.get(..5) == Some(&[b'x', 0, 0, 1, 0][..])
                && window.get(5..).is_some_and(|value| value.iter().all(|byte| *byte == 7))
        }));
    }

    #[test]
    fn create_stream_reads_stream_and_memblock_channels() {
        let mut tail = Tags::default();
        tail.u32(41);
        tail.u32(7);
        let mut fake = Fake::default();
        fake.recv.extend(control_frame(COMMAND_REPLY, 2, &tail.bytes));
        assert_eq!(create_stream(&mut fake, 2, 48_000, 0.5), Ok((41, 7)));
    }

    #[test]
    fn wrong_reply_command_is_rejected() {
        let mut fake = Fake::default();
        fake.recv.extend(control_frame(0, 9, &[]));
        assert!(matches!(reply_for(&mut fake, 9), Err(Error::System(_))));
    }

    #[test]
    fn oversized_control_packet_is_refused_before_allocation() {
        let mut fake = Fake::default();
        let mut header = [0u8; 20];
        header
            .get_mut(0..4)
            .unwrap_or_default()
            .copy_from_slice(&(u32::try_from(MAX_PACKET).unwrap_or_default().saturating_add(1)).to_be_bytes());
        header
            .get_mut(4..8)
            .unwrap_or_default()
            .copy_from_slice(&u32::MAX.to_be_bytes());
        fake.recv.extend(header);
        assert!(matches!(read_frame(&mut fake), Err(Error::Refused(_))));
    }

    #[test]
    fn music_chunks_are_written_as_memblocks_on_the_created_channel() {
        let mut tail = Tags::default();
        tail.u32(41);
        tail.u32(7);
        let mut auth_reply = Tags::default();
        auth_reply.u32(PROTOCOL_VERSION);
        let mut fake = Fake::default();
        fake.recv.extend(control_frame(COMMAND_REPLY, 1, &auth_reply.bytes));
        fake.recv.extend(control_frame(COMMAND_REPLY, 3, &[]));
        fake.recv.extend(control_frame(COMMAND_REPLY, 2, &tail.bytes));
        let Ok(mut stream) = establish(fake, &[0u8; 256], 1, 48_000, 0.5) else {
            panic!("stream is created on a valid reply");
        };
        assert!(stream.write(&[1, 2, 3]));
        let frame = stream.transport.sent.last().cloned().unwrap_or_default();
        assert_eq!(
            frame.get(4..8),
            Some(&7u32.to_be_bytes()[..]),
            "memblock goes to the created channel"
        );
        assert_eq!(
            frame.get(20..),
            Some(&[1, 0, 2, 0, 3, 0][..]),
            "samples are little-endian"
        );
    }

    #[test]
    fn deleting_a_stream_sends_its_index_with_the_delete_command() {
        let mut fake = Fake::default();
        fake.recv.extend(control_frame(COMMAND_REPLY, 4, &[]));
        assert!(delete_stream(&mut fake, 41).is_ok());
        let frame = fake.sent.first().cloned().unwrap_or_default();
        // Payload after the 20-byte header: tagged command, tagged request tag, then tagged stream index.
        assert_eq!(
            frame.get(21..25),
            Some(&COMMAND_DELETE_PLAYBACK_STREAM.to_be_bytes()[..])
        );
        assert_eq!(frame.get(30..35), Some(&[b'L', 0, 0, 0, 41][..]));
    }
}
