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
            "509ab9d3773650329f76a56f761e96ff1f4233c23eda20930caae1b578799903",
            "63acb2660ded88af16891ce5b7eeb85a2595dba93ebd21bc2fe1429ab580ea39",
            "f660f2948fc1f2bdb36177021abb0bf4a9a9835ceac61de2fb3682f21f0fe1fb",
            "946f9d7369fe75982ead17926af0fd3ad0fdf56d791bd4abf5ccd8ff663df62c",
            "006edccb7e469d3add6315390acc981179570b5c49fc62e5e8e6f94996b3106c",
            "3be6af9edf61a0a8a1e19d6de5f96560feae134d490586cda1cf7769b9817478",
            "3de639e09634af31bbf0c5346810af52ac405c3e7878ebe0ffa310010cda73b9",
            "d7e7c8fbb808981e61ee80596dda5d551832ab6a56d2f80001f449881398c8db",
            "bfa8c444bdf2e4bae6f4168c9295bd2dac3d43fce1a5265de7ea808d0f1faf42",
            "63621758683ecad58b12abaf157415074615ee0e56cbc69c03b16a57efc25650",
            "605906ee0e3602f466535da4a8666e54cb52689fc96c8751eb05d0ea39abc203",
            "f5578e4c47f9781e336ea3155f8d60cd9213c54b522e0d9e7ccc80701e5188c0",
            "4b81860531136a400ab7696c5f0b7e77e5f9b71abeb1662ecc47031d7f1573ae",
            "1edc232853a140ffd2334c4657d30ac19a70bdb52c20d050cba87eb66a2e12a4",
            "7bc72da366bd599ae3f02a17e3aabafbb3f40e2087a4ffc99527d402a00af4f2",
            "0cafebe9d58c64932d70315a4f76dfe78d65603fe5be8ebf40e042e9d8b275da",
            "0beeecbea95161c6e937c34ee03728e70673fbe7ecb79d6f7a791dcdeabd6072",
            "b47b29ba6c2e22d7b544c67e4ed3b13131c499cafe19c02150c574f873f95c36",
            "22702d491994ad31d9b6c7706b42db0a832c1117bb537a1e75ee7bb94deae76c",
            "53e4a48573f7e6d1887a56b102cbafd0bda25a387de19188cd51109f13a31f65",
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
