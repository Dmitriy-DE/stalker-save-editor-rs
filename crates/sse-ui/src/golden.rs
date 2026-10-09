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
            "8e2242a157ab01a631bd6051ae675a1fe8cceb77902a1446c4acebca6678c782",
            "06ef7213700f65c4e647f182147e12d03c7b090d26d549b9f15a983a03092240",
            "6423d3028e5ffe9f1f386e114e2b5a74513e75916cf99b434a6158db49ded186",
            "9f164ad21ac1eefdb40b12302650dc8468c47c132f899e1a0a325ad98e2760b2",
            "c4a863ce6505e602c93609ad4e165c327c6efed0137b8db4686b3feca8ab4b98",
            "39ff57d7116b6d84930acb218b687428a56d64e5d574bace83e481b918d3300f",
            "a3d2822a4e88389a3b80a60ac94de6bf7d6da07987f6b1a8c96f2061fbd5c15c",
            "dd3f3dc2f1b55cf3e3bbf58cce577be8921d53917b5f6822a3fa860c1eaab40d",
            "97314ddfefb9cfbf9993cc069cf90c1cd1e420d5ecd33e2bac2f87847e4f2020",
            "6538819484efe48793307f0639bb8d3793c98266d9f72a9cb114f29a43378565",
            "5853be0f45bf2c2a746090f07525dcee5ef0718876265c620782850786bfea2b",
            "0020a828c636082d03ec98bd3625a183676e405cbf4183acfdca85b756ee8afa",
            "e7fde6a370847bb434d7a9a1add50142d3d44e664f7c9fb4f2ccae521413edf1",
            "9f7395a072a99e1189a5ae792d9d99728b5737f16d08181e99a04fdafcb22405",
            "b6262863212864cc8370aaf0b8054a01accc374493fc7b45c5070be3141210f5",
            "fdf3aa2daf2e613680e65b92dfbabdeefe835fa88bba745449650ff02ef49f62",
            "ea81f95966705dd08f44cf8b3462cf2beea9665690145786ac73b441842df875",
            "0bd8b8661835fcb67e14fc23487e6c9598a965b8e4a6c08489d9e132c9e54c21",
            "2f6f43d6ded3e60bd687a7ccfb66eb4e9e523c012d20471c669fe4514f91a7e0",
            "77bd8af606adf7f57c52dfbbaf8fd44b7318a1ab11ef1cd22214bec3b0a97235",
        ];
        assert_eq!(
            ScreenId::ALL.len(),
            EXPECTED.len(),
            "golden set must cover every menu screen"
        );
        let mut actual = Vec::with_capacity(ScreenId::ALL.len());
        let mut tree = Tree::new(Fonts::bundled()?, crate::screens::style::rgb(BG_BASE));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.resize_window(&mut tree, 1280, 800)?;
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
