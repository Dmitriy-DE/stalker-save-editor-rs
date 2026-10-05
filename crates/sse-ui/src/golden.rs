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
    fn all_shell_screens_match_golden_fingerprints() -> sse_core::Result<()> {
        use crate::glyphs::Fonts;
        use crate::screens::shell::Shell;
        use crate::screens::ScreenId;
        use crate::theme::BG_BASE;
        use crate::widget::Tree;

        const EXPECTED: [&str; 20] = [
            "77ee3947e1d0eca5607700d59fa60f8d35916d267c6976545d3ac0ad7a1dac16",
            "aab58483cf1ee9b68b178e4ef301ba32e7460a9a00d414dacbd1148895da15c4",
            "8051d4cf91309b6969a244e611f2d3bef8931c18d397762300a44f9cbbbee518",
            "0a1501a839c6153fb8972bec0f03e3ebca17df91155c7c4d8be20956782a8ac1",
            "8e1936796dea6e0b101e1d6b1b1bd4569aff5555b557bf2ed99d8ddc9cc89bce",
            "82f27c4d4eebd7446f4998f2c129cf2ce0ea7861b0bcc3361c8889c25d5bec10",
            "bcf698903569d0ba574cbf0817d31411ebc78bd113ec539c68bbe3b479121a58",
            "5f3b964cb60149665c6e03334b05e06a8e2bb473ddb6b4c4d865950886d23d47",
            "52e86d80de846191617ff01823d5b4c1734619c6bae55e34c079ac15ff92a4f4",
            "31d088b72494e5ffe58ce3447be7bc085def4537f973ea7c8a4943b6a25e331f",
            "8cee39ba54c8796b936b482189f458f318618a73c0e5f27bbabb0e360ee7ce81",
            "39da010d0598005538838624eddb92932260999e39a232936b5999374aa29917",
            "efd8077d42cf4cbae88b1492de990bd7bb3303287d71abb430e4135231e75016",
            "6628eed4d1908f2b77170c2f65c9cdbbc4628055e1298a427c6aafbf0909758b",
            "2bfa2ab78778e634fe17867105460c05af22910f2a7e5efa179e70be393dd3d4",
            "95bc47d49f0a2774e5bbbc08ce0c7f09add1ec7e54ac51d93ad125c216b1f20d",
            "2633f90b1e3233a469d009419d8ae42555400da433a77d9ddff4103ecd838fe5",
            "e6d23f92c7e880ac3dc6e20c9bc901791c2fc3c8755bf309f7de8113af7e2e40",
            "edd1d1c2e2fc56282b6658afe3e6318ed918347612c11718edac47f226162c29",
            "589780f6d3d0f4ce53ed3c9d97a3a63ae21ea822c2e71b90bd1147e773155bab",
        ];
        assert_eq!(
            ScreenId::ALL.len(),
            EXPECTED.len(),
            "golden set must cover every menu screen"
        );
        let mut actual = Vec::with_capacity(ScreenId::ALL.len());
        let mut tree = Tree::new(Fonts::bundled()?, crate::screens::style::rgb(BG_BASE));
        let mut shell = Shell::build(&mut tree, None)?;
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
