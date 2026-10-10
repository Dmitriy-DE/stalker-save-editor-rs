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
            "d7719843ecc4cdc2aa42ed20ed99d6c1626b1e59f1e2875b16b5181d933ad045",
            "4dc15133ee1bab57c0cdabc2c50fe26482315a9915ba6bafa392028a6744c601",
            "4347173f5fe8a5283fd5cf6dbbee352548ba5e048336890d49e389d5c9414553",
            "cf24aef5f0582bc7238c6de172ca343ac6b774417881a06dd6e12db03c82907f",
            "21cc76db240d7c630dd75639e2ecb2ca3d5cd6ce1297df4fe10ba78874f8c3bb",
            "947030eb6000a4ee2a22a241a770d54a5a2f4b68881a80e5e257d2af1153fe9f",
            "d8650c0b17083d61c8bc9ad825cffa70278b94661ea2d27ea071f0d1ed052028",
            "c9ef10a3d8a7eec55978ac256741aaa373ba0bebc6e1df92887998a97c81a389",
            "9614fdffa33bd3ffcfaea1078049b94737c4a0e3afcf4e3d05edcce4598e5f05",
            "499d1f7f8ae1b9d4466ead6018fcd61b60942f327717a659d990fef6284f093a",
            "02c00b4d07dca03ee7ae0b1a8fe80f99120c91eefb08232f75bbf907b6613791",
            "f06eceb60381868f434adb346b1fccd1a3d5f455cad23e97993328e4e1faf06a",
            "3cbd25892a4cd702712791370b755b39730f2689310146b3ff3c9e0368ab016a",
            "616243b1acb0f067c8bbc3b2d4693adb242d4b81be35d084ca185d43b074eebd",
            "e37626b4efac613753d28424d555b6fcd4b5fdc00c0ddf7fa42f7e1d467bc8e3",
            "762acf171a5f08a6706fac7577e11d4c6cb64e73cc6035c1c544d5cf24709cc8",
            "50f7a1b4cea5d33224f0d090f5ce63ee726da26c3b22f9a450e41b2150d9932e",
            "bfe8543c8c6f39686b3cc1ca5b18872d97760c4e6b1d401db3c50bf6066eebac",
            "6224c23d09d59ba30084e9a7d6409995a205a0114e609e72abc5cf8179561fd0",
            "796cdb8a06f9d9372752900dc6ce2d6af2a87de48e6310c5901a45011e9e3ed7",
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
