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
            "51eea55aec69aa30435ad2884f070a1fae8648c38fc2525e2148cc3f61c47e10",
            "6423d3028e5ffe9f1f386e114e2b5a74513e75916cf99b434a6158db49ded186",
            "9f164ad21ac1eefdb40b12302650dc8468c47c132f899e1a0a325ad98e2760b2",
            "c4a863ce6505e602c93609ad4e165c327c6efed0137b8db4686b3feca8ab4b98",
            "39ff57d7116b6d84930acb218b687428a56d64e5d574bace83e481b918d3300f",
            "d6a7e6ff832f2c88bae9623fe388ce839621c1ef146ca17c5b5f742229bc7600",
            "14c6ea0c8b18b34625038e3730cd829ac75dfac41403c58fd9440cd886ae9a03",
            "28f33e257bba7993d4726c2e33219c64100fca2b6ef4c349ddd7b591731670d5",
            "6869022ed88c442435deca394972074d4fb1abf5078f905bfd09031f8b77bc10",
            "605906ee0e3602f466535da4a8666e54cb52689fc96c8751eb05d0ea39abc203",
            "43160bb9bf4e6d1f5eb861993aa69a84ddeddeeeb39b3ccaceb31190e3c51774",
            "3edb3ef86ef764d9891c955b37b765339f0531107cde3d16f9c90b8a7e488a0f",
            "acad22ef62c7a0bcabb758ca1735f49f3e540b0c8f59e9dd4d5536fff40ac6c3",
            "c2a48f3c9ebf1e7f732cbed48c7a2599d86f922375f72e6efd1b890cd7061f88",
            "2629304e40c86ac26be233c42ea5f20dc5ef61ac7b5f27d31d8c9a9eebde8800",
            "9599d5dd96f0c6998d4c0f3d717bca1c2bfe8a7fbb648959985ed1a3072a61b9",
            "ccc8e028e93fbbc051d31a2022f93e19f9debcc567789648d2b7f7ceddecd4c7",
            "37d96b0489c486bbd4c747cc6ab758cc60b7e6d69dec831290a966095f6cf827",
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
