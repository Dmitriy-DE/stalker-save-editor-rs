//! Pixel-tolerant golden screenshot comparison.

use sse_core::{Error, Result};

/// Golden comparison tolerances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Maximum absolute difference of any RGBA channel.
    pub channel: u8,
    /// Maximum fraction of pixels allowed to exceed channel tolerance (0..=1).
    pub pixel_fraction: f32,
}

/// Comparison statistics and an RGBA difference image.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// Compared width.
    pub width: u32,
    /// Compared height.
    pub height: u32,
    /// Pixels exceeding the per-channel tolerance.
    pub differing_pixels: usize,
    /// Total compared pixels.
    pub total_pixels: usize,
    /// Largest absolute channel delta observed.
    pub max_channel_delta: u8,
    /// Whether both tolerance gates passed.
    pub accepted: bool,
    /// RGBA diagnostic difference image.
    pub diff_rgba: Vec<u8>,
}

/// Compare rendered RGBA8 pixels with a reference PNG.
pub fn compare(
    rendered: &[u8],
    width: u32,
    height: u32,
    reference_png: &[u8],
    tolerance: Tolerance,
) -> Result<Comparison> {
    if !(0.0..=1.0).contains(&tolerance.pixel_fraction) {
        return Err(Error::Refused("golden pixel fraction must be 0..=1".to_owned()));
    }
    let reference = sse_codecs::png::decode(reference_png)?;
    if reference.width != width || reference.height != height {
        return Err(Error::Refused("golden dimensions differ".to_owned()));
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| Error::damaged("golden size overflow"))?;
    if rendered.len() != expected {
        return Err(Error::Refused("rendered RGBA buffer size differs".to_owned()));
    }
    let mut diff = vec![0u8; expected];
    let (mut bad, mut max) = (0usize, 0u8);
    for ((a, b), out) in rendered
        .chunks_exact(4)
        .zip(reference.pixels.chunks_exact(4))
        .zip(diff.chunks_exact_mut(4))
    {
        let mut pixel_bad = false;
        let mut pd = 0u8;
        for (x, y) in a.iter().zip(b) {
            let d = x.abs_diff(*y);
            max = max.max(d);
            pd = pd.max(d);
            pixel_bad |= d > tolerance.channel;
        }
        if pixel_bad {
            bad = bad.saturating_add(1);
        }
        out.copy_from_slice(&[pd, if pixel_bad { 0 } else { pd }, 0, 255]);
    }
    let pixels = expected / 4;
    let bad_u32 = u32::try_from(bad).map_err(|_| Error::damaged("golden bad-pixel count"))?;
    let pixels_u32 = u32::try_from(pixels).map_err(|_| Error::damaged("golden pixel count"))?;
    let accepted = f64::from(bad_u32) <= f64::from(pixels_u32) * f64::from(tolerance.pixel_fraction);
    Ok(Comparison {
        width,
        height,
        differing_pixels: bad,
        total_pixels: pixels,
        max_channel_delta: max,
        accepted,
        diff_rgba: diff,
    })
}

/// Encode a comparison's visual difference as PNG.
pub fn diff_png(c: &Comparison) -> Result<Vec<u8>> {
    sse_codecs::png_encode::encode_rgba8(c.width, c.height, &c.diff_rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_and_diff() {
        let r = [10, 20, 30, 255, 40, 50, 60, 255];
        let png = sse_codecs::png_encode::encode_rgba8(2, 1, &r).unwrap_or_else(|error| panic!("{error:?}"));
        let a = [12, 20, 30, 255, 50, 50, 60, 255];
        let c = compare(
            &a,
            2,
            1,
            &png,
            Tolerance {
                channel: 2,
                pixel_fraction: 0.5,
            },
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(c.accepted);
        assert_eq!(c.differing_pixels, 1);
        assert!(sse_codecs::png::decode(&diff_png(&c).unwrap_or_else(|error| panic!("{error:?}"))).is_ok());
    }

    #[test]
    #[cfg(feature = "native-ui")]
    fn all_shell_screens_match_golden_fingerprints() -> sse_core::Result<()> {
        use crate::glyphs::Fonts;
        use crate::screens::shell::Shell;
        use crate::screens::ScreenId;
        use crate::theme::BG_BASE;
        use crate::widget::Tree;

        struct EnvGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (key, value) in self.0.drain(..) {
                    if let Some(value) = value {
                        std::env::set_var(key, value);
                    } else {
                        std::env::remove_var(key);
                    }
                }
            }
        }
        let isolated = std::env::temp_dir().join(format!("sse-golden-home-{}", std::process::id()));
        std::fs::create_dir_all(&isolated)?;
        let keys = [
            "HOME",
            "XDG_DATA_HOME",
            "STALKER_SAVE_EDITOR_DATA",
            "USERPROFILE",
            "LOCALAPPDATA",
        ];
        let previous = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect();
        let _guard = EnvGuard(previous);
        for key in keys {
            std::env::set_var(key, &isolated);
        }

        const EXPECTED: [&str; 20] = [
            "35366b97e20175cff3c8657c808fefd52ef3ab91f763a372c519249dce2f6959",
            "14e1f762ea5a97a091fdbe55dff3d7ab19762a34a6d13c093235e185681de2ee",
            "c6e882ae5022b9db14d2a0ad0bf8e4c786fe304de104649e6508b27f2437fada",
            "6dcffa74a2b8cd2ef902a7fd9a7f61ca1dfc632ddc6e7d74b14568faa3d5acaa",
            "1458bdf07a956601bfe0a83d611d325d13276cdc407ae6ba2dfc08a555e82525",
            "c2f95b3bc458bce6a78fac3a21fbfd0e67093c319ced2a4461d37c21129b48a2",
            "68563be1dab8d34bd0ffd59f68885c5e41d4437724c95a1aa294612e9bc1734a",
            "dc8aa4c76f7d2542638726baa393375efa6dd298ac7ed456f6fd0e6cbc4f6fb7",
            "7842cf735705698177f7ce4638dce1d664b6205e9b176b9a33fdc0cf997d08c1",
            "c56fd93de47edce576779d253647bc34132aaa4b67d512ae5ee1057d755b9d97",
            "c89ad0539bce73796406ca29bea7d5c38933642c83832c77296ee8bd00fd8cf9",
            "3f24246f36f0725cafe6a0110f5ac3848ae1dd32b79cdb5d95fc7110afac8e8d",
            "4bc4faaa3e1f5540bacd93fa70a15b3f7410876c7d4ae40fa182f49d7a6a71b5",
            "43423d104af933682b2d43eeb48122dbd2d56da2dc69dd7a3c55e3fc62acd81e",
            "b7107e5dbb153139edbff968c9b10b3b5d975855867459c280d37d4958cbd448",
            "a1a49356939fb306ccdd709623570106138cb9d88347de6faf213bedab13b97a",
            "743f873565cac67d71a09852da7a661cbd46ce72bf2de7f67e17fe17d7e7ff2b",
            "66c43a7588ebcc5a2a7790c0bfd93ec723c60c357db05c2cd12eeba1b28f5b42",
            "726b8e8cf07c481e6ddf981727cf4eca67a4d89d6c011ce95f74756111b043ff",
            "93210157ec52eb407e8cd3fc306fcb98ded6342d874e2936a556feaae6735e59",
        ];
        assert_eq!(
            ScreenId::ALL.len(),
            EXPECTED.len(),
            "golden set must cover every menu screen"
        );
        let mut actual = Vec::with_capacity(ScreenId::ALL.len());
        let mut tree = Tree::new(Fonts::bundled()?, crate::screens::style::rgb(BG_BASE));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        tree.resize(1280, 800);
        for id in &ScreenId::ALL {
            shell.open(&mut tree, *id)?;
            let mut frame = vec![0_u32; 1280 * 800];
            tree.paint(&mut frame, 1280)?;
            let mut bytes = Vec::with_capacity(frame.len() * 4);
            for pixel in frame {
                bytes.extend_from_slice(&pixel.to_le_bytes());
            }
            let digest = sse_codecs::sha256::sha256_hex(&bytes);
            actual.push(digest);
        }
        for (index, (id, digest)) in ScreenId::ALL.iter().zip(actual.iter()).enumerate() {
            assert_eq!(
                digest,
                EXPECTED.get(index).unwrap_or(&""),
                "golden mismatch for {id:?}; actual set: {actual:?}"
            );
        }
        Ok(())
    }
}
