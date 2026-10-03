//! Strict ZIP reader and reproducible writer (stored/deflate, ZIP64 read support).
use crate::{
    crc32::crc32,
    deflate::{compress_raw, Level},
    inflate::inflate_raw,
};
use sse_core::{Error, Result};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}
fn u16le(b: &[u8], p: usize) -> Result<u16> {
    let s = b
        .get(p..p.saturating_add(2))
        .ok_or_else(|| Error::damaged("short ZIP u16"))?;
    Ok(u16::from_le_bytes(
        <[u8; 2]>::try_from(s).map_err(|_| Error::damaged("ZIP u16"))?,
    ))
}
fn u32le(b: &[u8], p: usize) -> Result<u32> {
    let s = b
        .get(p..p.saturating_add(4))
        .ok_or_else(|| Error::damaged("short ZIP u32"))?;
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(s).map_err(|_| Error::damaged("ZIP u32"))?,
    ))
}
fn u64le(b: &[u8], p: usize) -> Result<u64> {
    let s = b
        .get(p..p.saturating_add(8))
        .ok_or_else(|| Error::damaged("short ZIP u64"))?;
    Ok(u64::from_le_bytes(
        <[u8; 8]>::try_from(s).map_err(|_| Error::damaged("ZIP u64"))?,
    ))
}
fn safe_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        return false;
    }
    !name.split(['/', '\\']).any(|x| x == "..")
}
fn cp437(bytes: &[u8]) -> String {
    const HI:&str="ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´≡±‗¾¶§÷¸°¨·¹³²■ ";
    let chars: Vec<char> = HI.chars().collect();
    let mut s = String::new();
    for x in bytes {
        if *x < 128 {
            s.push(char::from(*x));
        } else {
            s.push(chars.get(usize::from(*x) - 128).copied().unwrap_or('�'));
        }
    }
    s
}
fn name(bytes: &[u8], utf8: bool) -> Result<String> {
    let s = if utf8 {
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::damaged("invalid ZIP UTF-8 name"))?
    } else {
        cp437(bytes)
    };
    if !safe_name(&s) {
        return Err(Error::Refused("unsafe ZIP path".to_owned()));
    }
    Ok(s)
}
fn zip64(extra: &[u8], need_u: bool, need_c: bool, need_o: bool) -> Result<(Option<u64>, Option<u64>, Option<u64>)> {
    let mut p = 0usize;
    while p.saturating_add(4) <= extra.len() {
        let id = u16le(extra, p)?;
        let n = usize::from(u16le(extra, p + 2)?);
        let body = extra
            .get(p + 4..p + 4 + n)
            .ok_or_else(|| Error::damaged("short ZIP extra"))?;
        if id == 1 {
            let mut q = 0usize;
            let mut take = |need: bool| -> Result<Option<u64>> {
                if !need {
                    return Ok(None);
                }
                let v = u64le(body, q)?;
                q = q.saturating_add(8);
                Ok(Some(v))
            };
            return Ok((take(need_u)?, take(need_c)?, take(need_o)?));
        }
        p = p.saturating_add(4 + n);
    }
    Ok((None, None, None))
}
/// Reads all regular-file entries with a total decompressed byte ceiling.
pub fn read(input: &[u8], maximum_output: usize) -> Result<Vec<Entry>> {
    let start = input.len().saturating_sub(65_557);
    let eocd = (start..input.len().saturating_sub(3))
        .rev()
        .find(|p| input.get(*p..p.saturating_add(4)) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(|| Error::damaged("ZIP EOCD not found"))?;
    let count = usize::from(u16le(input, eocd + 10)?);
    let mut cd = usize::try_from(u32le(input, eocd + 16)?).map_err(|_| Error::damaged("ZIP central offset"))?;
    let mut out = Vec::new();
    let mut total = 0usize;
    for _ in 0..count {
        if input.get(cd..cd + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(Error::damaged("ZIP central signature"));
        }
        let flags = u16le(input, cd + 8)?;
        let method = u16le(input, cd + 10)?;
        let crc = u32le(input, cd + 16)?;
        let cs32 = u32le(input, cd + 20)?;
        let us32 = u32le(input, cd + 24)?;
        let nl = usize::from(u16le(input, cd + 28)?);
        let xl = usize::from(u16le(input, cd + 30)?);
        let cl = usize::from(u16le(input, cd + 32)?);
        let attrs = u32le(input, cd + 38)?;
        let off32 = u32le(input, cd + 42)?;
        let nb = input
            .get(cd + 46..cd + 46 + nl)
            .ok_or_else(|| Error::damaged("ZIP name"))?;
        let extra = input
            .get(cd + 46 + nl..cd + 46 + nl + xl)
            .ok_or_else(|| Error::damaged("ZIP extra"))?;
        let (zu, zc, zo) = zip64(extra, us32 == u32::MAX, cs32 == u32::MAX, off32 == u32::MAX)?;
        let us = usize::try_from(zu.unwrap_or(u64::from(us32))).map_err(|_| Error::damaged("ZIP size"))?;
        let cs = usize::try_from(zc.unwrap_or(u64::from(cs32))).map_err(|_| Error::damaged("ZIP size"))?;
        let off = usize::try_from(zo.unwrap_or(u64::from(off32))).map_err(|_| Error::damaged("ZIP offset"))?;
        let unix_mode = (attrs >> 16) & 0xffff;
        if unix_mode & 0xf000 == 0xa000 {
            return Err(Error::Refused("ZIP symlink refused".to_owned()));
        }
        let n = name(nb, flags & 0x800 != 0)?;
        if n.ends_with('/') {
            cd = cd.saturating_add(46 + nl + xl + cl);
            continue;
        }
        total = total
            .checked_add(us)
            .ok_or_else(|| Error::damaged("ZIP output size overflow"))?;
        if total > maximum_output {
            return Err(Error::Refused("ZIP output limit exceeded".to_owned()));
        }
        if input.get(off..off + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(Error::damaged("ZIP local signature"));
        }
        let lnl = usize::from(u16le(input, off + 26)?);
        let lxl = usize::from(u16le(input, off + 28)?);
        let ds = off
            .checked_add(30 + lnl + lxl)
            .ok_or_else(|| Error::damaged("ZIP data offset"))?;
        let comp = input
            .get(ds..ds + cs)
            .ok_or_else(|| Error::damaged("truncated ZIP data"))?;
        let data = match method {
            0 => comp.to_vec(),
            8 => inflate_raw(comp, us)?,
            _ => return Err(Error::Refused("unsupported ZIP compression method".to_owned())),
        };
        if data.len() != us || crc32(&data) != crc {
            return Err(Error::damaged("ZIP size/CRC mismatch"));
        }
        out.push(Entry { name: n, data });
        cd = cd
            .checked_add(46 + nl + xl + cl)
            .ok_or_else(|| Error::damaged("ZIP central overflow"))?;
    }
    Ok(out)
}
fn put16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes())
}
fn put32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes())
}
/// Reproducible ZIP writer: entries are sorted by UTF-8 name and use the DOS epoch timestamp.
pub fn write(entries: &[Entry], level: Level) -> Result<Vec<u8>> {
    let mut refs: Vec<&Entry> = entries.iter().collect();
    refs.sort_by(|a, b| a.name.cmp(&b.name));
    let mut o = Vec::new();
    let mut central = Vec::new();
    for e in refs {
        if !safe_name(&e.name) {
            return Err(Error::Refused("unsafe ZIP path".to_owned()));
        }
        let nb = e.name.as_bytes();
        let nl = u16::try_from(nb.len()).map_err(|_| Error::Refused("ZIP name too long".to_owned()))?;
        let comp = compress_raw(&e.data, level)?;
        let (method, payload) = if comp.len() < e.data.len() {
            (8u16, comp)
        } else {
            (0u16, e.data.clone())
        };
        let off = u32::try_from(o.len())
            .map_err(|_| Error::Refused("ZIP64 writer not required for oversized archive".to_owned()))?;
        let cs = u32::try_from(payload.len()).map_err(|_| Error::Refused("ZIP entry too large".to_owned()))?;
        let us = u32::try_from(e.data.len()).map_err(|_| Error::Refused("ZIP entry too large".to_owned()))?;
        let crc = crc32(&e.data);
        put32(&mut o, 0x04034b50);
        put16(&mut o, 20);
        put16(&mut o, 0x800);
        put16(&mut o, method);
        put16(&mut o, 0);
        put16(&mut o, 0x21);
        put32(&mut o, crc);
        put32(&mut o, cs);
        put32(&mut o, us);
        put16(&mut o, nl);
        put16(&mut o, 0);
        o.extend_from_slice(nb);
        o.extend_from_slice(&payload);
        put32(&mut central, 0x02014b50);
        put16(&mut central, 0x0314);
        put16(&mut central, 20);
        put16(&mut central, 0x800);
        put16(&mut central, method);
        put16(&mut central, 0);
        put16(&mut central, 0x21);
        put32(&mut central, crc);
        put32(&mut central, cs);
        put32(&mut central, us);
        put16(&mut central, nl);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put32(&mut central, 0o100644u32 << 16);
        put32(&mut central, off);
        central.extend_from_slice(nb);
    }
    let cd_off = u32::try_from(o.len()).map_err(|_| Error::Refused("ZIP too large".to_owned()))?;
    let cd_size =
        u32::try_from(central.len()).map_err(|_| Error::Refused("ZIP central directory too large".to_owned()))?;
    o.extend_from_slice(&central);
    let count = u16::try_from(entries.len()).map_err(|_| Error::Refused("too many ZIP entries".to_owned()))?;
    put32(&mut o, 0x06054b50);
    put16(&mut o, 0);
    put16(&mut o, 0);
    put16(&mut o, count);
    put16(&mut o, count);
    put32(&mut o, cd_size);
    put32(&mut o, cd_off);
    put16(&mut o, 0);
    Ok(o)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_stable() {
        let e = vec![
            Entry {
                name: "b.txt".to_owned(),
                data: vec![b'x'; 1000],
            },
            Entry {
                name: "a.txt".to_owned(),
                data: b"hello".to_vec(),
            },
        ];
        let a = write(&e, Level::Default).unwrap_or_default();
        let b = write(&e, Level::Default).unwrap_or_default();
        assert_eq!(a, b);
        let r = read(&a, 5000).unwrap_or_default();
        assert_eq!(r.len(), 2);
        assert_eq!(r.first().map(|x| x.name.as_str()), Some("a.txt"));
    }
    #[test]
    fn traversal_refused() {
        let e = [Entry {
            name: "../x".to_owned(),
            data: Vec::new(),
        }];
        assert!(write(&e, Level::Fast).is_err());
    }
}
