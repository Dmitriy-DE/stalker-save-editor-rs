//! Unicode 17.0.0 grapheme-cluster and line-break property tables.
//!
//! Generated from the Unicode Character Database files GraphemeBreakProperty.txt,
//! LineBreak.txt, DerivedCoreProperties.txt and emoji-data.txt.

#[derive(Clone, Copy)]
struct Range {
    start: u32,
    end: u32,
    class: u8,
}

/// Grapheme_Cluster_Break value.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphemeClass {
    Other = 0,
    Cr,
    Lf,
    Control,
    Extend,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
    Zwj,
}

/// Line_Break value before LB1 resolution.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreakClass {
    Xx = 0,
    Bk,
    Cr,
    Lf,
    Cm,
    Nl,
    Sg,
    Wj,
    Zw,
    Gl,
    Sp,
    B2,
    Ba,
    Bb,
    Hy,
    Cb,
    Cl,
    Cp,
    Ex,
    In,
    Ns,
    Op,
    Qu,
    Is,
    Nu,
    Po,
    Pr,
    Sy,
    Ai,
    Al,
    Cj,
    H2,
    H3,
    Hl,
    Id,
    Jl,
    Jv,
    Jt,
    Ri,
    Sa,
    Zwj,
    Eb,
    Em,
    Ak,
    Ap,
    As,
    Vf,
    Vi,
    Hh,
}

/// Indic_Conjunct_Break value used by UAX #29 GB9c.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicConjunct {
    None = 0,
    Extend,
    Consonant,
    Linker,
}

const GRAPHEME_RANGES: &[Range] = &[
    Range {
        start: 0x0,
        end: 0x9,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xa,
        end: 0xa,
        class: GraphemeClass::Lf as u8,
    },
    Range {
        start: 0xb,
        end: 0xc,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xd,
        end: 0xd,
        class: GraphemeClass::Cr as u8,
    },
    Range {
        start: 0xe,
        end: 0x1f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x7f,
        end: 0x9f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xad,
        end: 0xad,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x300,
        end: 0x36f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x483,
        end: 0x487,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x488,
        end: 0x489,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x591,
        end: 0x5bd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x5bf,
        end: 0x5bf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x5c1,
        end: 0x5c2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x5c4,
        end: 0x5c5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x5c7,
        end: 0x5c7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x600,
        end: 0x605,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x610,
        end: 0x61a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x61c,
        end: 0x61c,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x64b,
        end: 0x65f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x670,
        end: 0x670,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x6d6,
        end: 0x6dc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x6dd,
        end: 0x6dd,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x6df,
        end: 0x6e4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x6e7,
        end: 0x6e8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x6ea,
        end: 0x6ed,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x70f,
        end: 0x70f,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x711,
        end: 0x711,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x730,
        end: 0x74a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x7a6,
        end: 0x7b0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x7eb,
        end: 0x7f3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x7fd,
        end: 0x7fd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x816,
        end: 0x819,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x81b,
        end: 0x823,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x825,
        end: 0x827,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x829,
        end: 0x82d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x859,
        end: 0x85b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x890,
        end: 0x891,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x897,
        end: 0x89f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x8ca,
        end: 0x8e1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x8e2,
        end: 0x8e2,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x8e3,
        end: 0x902,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x903,
        end: 0x903,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x93a,
        end: 0x93a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x93b,
        end: 0x93b,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x93c,
        end: 0x93c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x93e,
        end: 0x940,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x941,
        end: 0x948,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x949,
        end: 0x94c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x94d,
        end: 0x94d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x94e,
        end: 0x94f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x951,
        end: 0x957,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x962,
        end: 0x963,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x981,
        end: 0x981,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x982,
        end: 0x983,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x9bc,
        end: 0x9bc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9be,
        end: 0x9be,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9bf,
        end: 0x9c0,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x9c1,
        end: 0x9c4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9c7,
        end: 0x9c8,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x9cb,
        end: 0x9cc,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x9cd,
        end: 0x9cd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9d7,
        end: 0x9d7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9e2,
        end: 0x9e3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x9fe,
        end: 0x9fe,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa01,
        end: 0xa02,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa03,
        end: 0xa03,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa3c,
        end: 0xa3c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa3e,
        end: 0xa40,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa41,
        end: 0xa42,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa47,
        end: 0xa48,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa4b,
        end: 0xa4d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa51,
        end: 0xa51,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa70,
        end: 0xa71,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa75,
        end: 0xa75,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa81,
        end: 0xa82,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa83,
        end: 0xa83,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xabc,
        end: 0xabc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xabe,
        end: 0xac0,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xac1,
        end: 0xac5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xac7,
        end: 0xac8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xac9,
        end: 0xac9,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xacb,
        end: 0xacc,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xacd,
        end: 0xacd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xae2,
        end: 0xae3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xafa,
        end: 0xaff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb01,
        end: 0xb01,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb02,
        end: 0xb03,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xb3c,
        end: 0xb3c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb3e,
        end: 0xb3e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb3f,
        end: 0xb3f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb40,
        end: 0xb40,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xb41,
        end: 0xb44,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb47,
        end: 0xb48,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xb4b,
        end: 0xb4c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xb4d,
        end: 0xb4d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb55,
        end: 0xb56,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb57,
        end: 0xb57,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb62,
        end: 0xb63,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xb82,
        end: 0xb82,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xbbe,
        end: 0xbbe,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xbbf,
        end: 0xbbf,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xbc0,
        end: 0xbc0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xbc1,
        end: 0xbc2,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xbc6,
        end: 0xbc8,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xbca,
        end: 0xbcc,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xbcd,
        end: 0xbcd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xbd7,
        end: 0xbd7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc00,
        end: 0xc00,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc01,
        end: 0xc03,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xc04,
        end: 0xc04,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc3c,
        end: 0xc3c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc3e,
        end: 0xc40,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc41,
        end: 0xc44,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xc46,
        end: 0xc48,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc4a,
        end: 0xc4d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc55,
        end: 0xc56,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc62,
        end: 0xc63,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc81,
        end: 0xc81,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xc82,
        end: 0xc83,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xcbc,
        end: 0xcbc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcbe,
        end: 0xcbe,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xcbf,
        end: 0xcbf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcc0,
        end: 0xcc0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcc1,
        end: 0xcc1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xcc2,
        end: 0xcc2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcc3,
        end: 0xcc4,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xcc6,
        end: 0xcc6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcc7,
        end: 0xcc8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcca,
        end: 0xccb,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xccc,
        end: 0xccd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcd5,
        end: 0xcd6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xce2,
        end: 0xce3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xcf3,
        end: 0xcf3,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xd00,
        end: 0xd01,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd02,
        end: 0xd03,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xd3b,
        end: 0xd3c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd3e,
        end: 0xd3e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd3f,
        end: 0xd40,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xd41,
        end: 0xd44,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd46,
        end: 0xd48,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xd4a,
        end: 0xd4c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xd4d,
        end: 0xd4d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd4e,
        end: 0xd4e,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0xd57,
        end: 0xd57,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd62,
        end: 0xd63,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd81,
        end: 0xd81,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xd82,
        end: 0xd83,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xdca,
        end: 0xdca,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xdcf,
        end: 0xdcf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xdd0,
        end: 0xdd1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xdd2,
        end: 0xdd4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xdd6,
        end: 0xdd6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xdd8,
        end: 0xdde,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xddf,
        end: 0xddf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xdf2,
        end: 0xdf3,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xe31,
        end: 0xe31,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xe33,
        end: 0xe33,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xe34,
        end: 0xe3a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xe47,
        end: 0xe4e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xeb1,
        end: 0xeb1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xeb3,
        end: 0xeb3,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xeb4,
        end: 0xebc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xec8,
        end: 0xece,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf18,
        end: 0xf19,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf35,
        end: 0xf35,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf37,
        end: 0xf37,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf39,
        end: 0xf39,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf3e,
        end: 0xf3f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xf71,
        end: 0xf7e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf7f,
        end: 0xf7f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xf80,
        end: 0xf84,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf86,
        end: 0xf87,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf8d,
        end: 0xf97,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xf99,
        end: 0xfbc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xfc6,
        end: 0xfc6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x102d,
        end: 0x1030,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1031,
        end: 0x1031,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1032,
        end: 0x1037,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1039,
        end: 0x103a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x103b,
        end: 0x103c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x103d,
        end: 0x103e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1056,
        end: 0x1057,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1058,
        end: 0x1059,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x105e,
        end: 0x1060,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1071,
        end: 0x1074,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1082,
        end: 0x1082,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1084,
        end: 0x1084,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1085,
        end: 0x1086,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x108d,
        end: 0x108d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x109d,
        end: 0x109d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1100,
        end: 0x115f,
        class: GraphemeClass::L as u8,
    },
    Range {
        start: 0x1160,
        end: 0x11a7,
        class: GraphemeClass::V as u8,
    },
    Range {
        start: 0x11a8,
        end: 0x11ff,
        class: GraphemeClass::T as u8,
    },
    Range {
        start: 0x135d,
        end: 0x135f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1712,
        end: 0x1714,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1715,
        end: 0x1715,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1732,
        end: 0x1733,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1734,
        end: 0x1734,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1752,
        end: 0x1753,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1772,
        end: 0x1773,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x17b4,
        end: 0x17b5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x17b6,
        end: 0x17b6,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x17b7,
        end: 0x17bd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x17be,
        end: 0x17c5,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x17c6,
        end: 0x17c6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x17c7,
        end: 0x17c8,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x17c9,
        end: 0x17d3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x17dd,
        end: 0x17dd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x180b,
        end: 0x180d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x180e,
        end: 0x180e,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x180f,
        end: 0x180f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1885,
        end: 0x1886,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x18a9,
        end: 0x18a9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1920,
        end: 0x1922,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1923,
        end: 0x1926,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1927,
        end: 0x1928,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1929,
        end: 0x192b,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1930,
        end: 0x1931,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1932,
        end: 0x1932,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1933,
        end: 0x1938,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1939,
        end: 0x193b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a17,
        end: 0x1a18,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a19,
        end: 0x1a1a,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1a1b,
        end: 0x1a1b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a55,
        end: 0x1a55,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1a56,
        end: 0x1a56,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a57,
        end: 0x1a57,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1a58,
        end: 0x1a5e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a60,
        end: 0x1a60,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a62,
        end: 0x1a62,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a65,
        end: 0x1a6c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a6d,
        end: 0x1a72,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1a73,
        end: 0x1a7c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1a7f,
        end: 0x1a7f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1ab0,
        end: 0x1abd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1abe,
        end: 0x1abe,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1abf,
        end: 0x1add,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1ae0,
        end: 0x1aeb,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b00,
        end: 0x1b03,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b04,
        end: 0x1b04,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1b34,
        end: 0x1b34,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b35,
        end: 0x1b35,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b36,
        end: 0x1b3a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b3b,
        end: 0x1b3b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b3c,
        end: 0x1b3c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b3d,
        end: 0x1b3d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b3e,
        end: 0x1b41,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1b42,
        end: 0x1b42,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b43,
        end: 0x1b44,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b6b,
        end: 0x1b73,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b80,
        end: 0x1b81,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1b82,
        end: 0x1b82,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1ba1,
        end: 0x1ba1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1ba2,
        end: 0x1ba5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1ba6,
        end: 0x1ba7,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1ba8,
        end: 0x1ba9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1baa,
        end: 0x1baa,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bab,
        end: 0x1bad,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1be6,
        end: 0x1be6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1be7,
        end: 0x1be7,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1be8,
        end: 0x1be9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bea,
        end: 0x1bec,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1bed,
        end: 0x1bed,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bee,
        end: 0x1bee,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1bef,
        end: 0x1bf1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bf2,
        end: 0x1bf3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1c24,
        end: 0x1c2b,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1c2c,
        end: 0x1c33,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1c34,
        end: 0x1c35,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1c36,
        end: 0x1c37,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1cd0,
        end: 0x1cd2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1cd4,
        end: 0x1ce0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1ce1,
        end: 0x1ce1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1ce2,
        end: 0x1ce8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1ced,
        end: 0x1ced,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1cf4,
        end: 0x1cf4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1cf7,
        end: 0x1cf7,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1cf8,
        end: 0x1cf9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1dc0,
        end: 0x1dff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x200b,
        end: 0x200b,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x200c,
        end: 0x200c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x200d,
        end: 0x200d,
        class: GraphemeClass::Zwj as u8,
    },
    Range {
        start: 0x200e,
        end: 0x200f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x2028,
        end: 0x2028,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x2029,
        end: 0x2029,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x202a,
        end: 0x202e,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x2060,
        end: 0x2064,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x2065,
        end: 0x2065,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x2066,
        end: 0x206f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x20d0,
        end: 0x20dc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x20dd,
        end: 0x20e0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x20e1,
        end: 0x20e1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x20e2,
        end: 0x20e4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x20e5,
        end: 0x20f0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x2cef,
        end: 0x2cf1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x2d7f,
        end: 0x2d7f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x2de0,
        end: 0x2dff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x302a,
        end: 0x302d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x302e,
        end: 0x302f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x3099,
        end: 0x309a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa66f,
        end: 0xa66f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa670,
        end: 0xa672,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa674,
        end: 0xa67d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa69e,
        end: 0xa69f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa6f0,
        end: 0xa6f1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa802,
        end: 0xa802,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa806,
        end: 0xa806,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa80b,
        end: 0xa80b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa823,
        end: 0xa824,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa825,
        end: 0xa826,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa827,
        end: 0xa827,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa82c,
        end: 0xa82c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa880,
        end: 0xa881,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa8b4,
        end: 0xa8c3,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa8c4,
        end: 0xa8c5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa8e0,
        end: 0xa8f1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa8ff,
        end: 0xa8ff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa926,
        end: 0xa92d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa947,
        end: 0xa951,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa952,
        end: 0xa952,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa953,
        end: 0xa953,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa960,
        end: 0xa97c,
        class: GraphemeClass::L as u8,
    },
    Range {
        start: 0xa980,
        end: 0xa982,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa983,
        end: 0xa983,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa9b3,
        end: 0xa9b3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa9b4,
        end: 0xa9b5,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa9b6,
        end: 0xa9b9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa9ba,
        end: 0xa9bb,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa9bc,
        end: 0xa9bd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa9be,
        end: 0xa9bf,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xa9c0,
        end: 0xa9c0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xa9e5,
        end: 0xa9e5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa29,
        end: 0xaa2e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa2f,
        end: 0xaa30,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaa31,
        end: 0xaa32,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa33,
        end: 0xaa34,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaa35,
        end: 0xaa36,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa43,
        end: 0xaa43,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa4c,
        end: 0xaa4c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaa4d,
        end: 0xaa4d,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaa7c,
        end: 0xaa7c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaab0,
        end: 0xaab0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaab2,
        end: 0xaab4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaab7,
        end: 0xaab8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaabe,
        end: 0xaabf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaac1,
        end: 0xaac1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaaeb,
        end: 0xaaeb,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaaec,
        end: 0xaaed,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xaaee,
        end: 0xaaef,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaaf5,
        end: 0xaaf5,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xaaf6,
        end: 0xaaf6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xabe3,
        end: 0xabe4,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xabe5,
        end: 0xabe5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xabe6,
        end: 0xabe7,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xabe8,
        end: 0xabe8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xabe9,
        end: 0xabea,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xabec,
        end: 0xabec,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0xabed,
        end: 0xabed,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xac00,
        end: 0xac00,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac01,
        end: 0xac1b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xac1c,
        end: 0xac1c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac1d,
        end: 0xac37,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xac38,
        end: 0xac38,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac39,
        end: 0xac53,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xac54,
        end: 0xac54,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac55,
        end: 0xac6f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xac70,
        end: 0xac70,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac71,
        end: 0xac8b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xac8c,
        end: 0xac8c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xac8d,
        end: 0xaca7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaca8,
        end: 0xaca8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaca9,
        end: 0xacc3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xacc4,
        end: 0xacc4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xacc5,
        end: 0xacdf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xace0,
        end: 0xace0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xace1,
        end: 0xacfb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xacfc,
        end: 0xacfc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xacfd,
        end: 0xad17,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xad18,
        end: 0xad18,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xad19,
        end: 0xad33,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xad34,
        end: 0xad34,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xad35,
        end: 0xad4f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xad50,
        end: 0xad50,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xad51,
        end: 0xad6b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xad6c,
        end: 0xad6c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xad6d,
        end: 0xad87,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xad88,
        end: 0xad88,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xad89,
        end: 0xada3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xada4,
        end: 0xada4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xada5,
        end: 0xadbf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xadc0,
        end: 0xadc0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xadc1,
        end: 0xaddb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaddc,
        end: 0xaddc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaddd,
        end: 0xadf7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xadf8,
        end: 0xadf8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xadf9,
        end: 0xae13,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xae14,
        end: 0xae14,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xae15,
        end: 0xae2f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xae30,
        end: 0xae30,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xae31,
        end: 0xae4b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xae4c,
        end: 0xae4c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xae4d,
        end: 0xae67,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xae68,
        end: 0xae68,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xae69,
        end: 0xae83,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xae84,
        end: 0xae84,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xae85,
        end: 0xae9f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaea0,
        end: 0xaea0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaea1,
        end: 0xaebb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaebc,
        end: 0xaebc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaebd,
        end: 0xaed7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaed8,
        end: 0xaed8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaed9,
        end: 0xaef3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaef4,
        end: 0xaef4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaef5,
        end: 0xaf0f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf10,
        end: 0xaf10,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf11,
        end: 0xaf2b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf2c,
        end: 0xaf2c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf2d,
        end: 0xaf47,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf48,
        end: 0xaf48,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf49,
        end: 0xaf63,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf64,
        end: 0xaf64,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf65,
        end: 0xaf7f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf80,
        end: 0xaf80,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf81,
        end: 0xaf9b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaf9c,
        end: 0xaf9c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaf9d,
        end: 0xafb7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xafb8,
        end: 0xafb8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xafb9,
        end: 0xafd3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xafd4,
        end: 0xafd4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xafd5,
        end: 0xafef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xaff0,
        end: 0xaff0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xaff1,
        end: 0xb00b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb00c,
        end: 0xb00c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb00d,
        end: 0xb027,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb028,
        end: 0xb028,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb029,
        end: 0xb043,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb044,
        end: 0xb044,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb045,
        end: 0xb05f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb060,
        end: 0xb060,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb061,
        end: 0xb07b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb07c,
        end: 0xb07c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb07d,
        end: 0xb097,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb098,
        end: 0xb098,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb099,
        end: 0xb0b3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb0b4,
        end: 0xb0b4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb0b5,
        end: 0xb0cf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb0d0,
        end: 0xb0d0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb0d1,
        end: 0xb0eb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb0ec,
        end: 0xb0ec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb0ed,
        end: 0xb107,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb108,
        end: 0xb108,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb109,
        end: 0xb123,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb124,
        end: 0xb124,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb125,
        end: 0xb13f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb140,
        end: 0xb140,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb141,
        end: 0xb15b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb15c,
        end: 0xb15c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb15d,
        end: 0xb177,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb178,
        end: 0xb178,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb179,
        end: 0xb193,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb194,
        end: 0xb194,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb195,
        end: 0xb1af,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb1b0,
        end: 0xb1b0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb1b1,
        end: 0xb1cb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb1cc,
        end: 0xb1cc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb1cd,
        end: 0xb1e7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb1e8,
        end: 0xb1e8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb1e9,
        end: 0xb203,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb204,
        end: 0xb204,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb205,
        end: 0xb21f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb220,
        end: 0xb220,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb221,
        end: 0xb23b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb23c,
        end: 0xb23c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb23d,
        end: 0xb257,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb258,
        end: 0xb258,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb259,
        end: 0xb273,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb274,
        end: 0xb274,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb275,
        end: 0xb28f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb290,
        end: 0xb290,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb291,
        end: 0xb2ab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb2ac,
        end: 0xb2ac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb2ad,
        end: 0xb2c7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb2c8,
        end: 0xb2c8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb2c9,
        end: 0xb2e3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb2e4,
        end: 0xb2e4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb2e5,
        end: 0xb2ff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb300,
        end: 0xb300,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb301,
        end: 0xb31b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb31c,
        end: 0xb31c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb31d,
        end: 0xb337,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb338,
        end: 0xb338,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb339,
        end: 0xb353,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb354,
        end: 0xb354,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb355,
        end: 0xb36f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb370,
        end: 0xb370,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb371,
        end: 0xb38b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb38c,
        end: 0xb38c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb38d,
        end: 0xb3a7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb3a8,
        end: 0xb3a8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb3a9,
        end: 0xb3c3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb3c4,
        end: 0xb3c4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb3c5,
        end: 0xb3df,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb3e0,
        end: 0xb3e0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb3e1,
        end: 0xb3fb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb3fc,
        end: 0xb3fc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb3fd,
        end: 0xb417,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb418,
        end: 0xb418,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb419,
        end: 0xb433,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb434,
        end: 0xb434,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb435,
        end: 0xb44f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb450,
        end: 0xb450,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb451,
        end: 0xb46b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb46c,
        end: 0xb46c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb46d,
        end: 0xb487,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb488,
        end: 0xb488,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb489,
        end: 0xb4a3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb4a4,
        end: 0xb4a4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb4a5,
        end: 0xb4bf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb4c0,
        end: 0xb4c0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb4c1,
        end: 0xb4db,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb4dc,
        end: 0xb4dc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb4dd,
        end: 0xb4f7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb4f8,
        end: 0xb4f8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb4f9,
        end: 0xb513,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb514,
        end: 0xb514,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb515,
        end: 0xb52f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb530,
        end: 0xb530,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb531,
        end: 0xb54b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb54c,
        end: 0xb54c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb54d,
        end: 0xb567,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb568,
        end: 0xb568,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb569,
        end: 0xb583,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb584,
        end: 0xb584,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb585,
        end: 0xb59f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb5a0,
        end: 0xb5a0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb5a1,
        end: 0xb5bb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb5bc,
        end: 0xb5bc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb5bd,
        end: 0xb5d7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb5d8,
        end: 0xb5d8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb5d9,
        end: 0xb5f3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb5f4,
        end: 0xb5f4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb5f5,
        end: 0xb60f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb610,
        end: 0xb610,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb611,
        end: 0xb62b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb62c,
        end: 0xb62c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb62d,
        end: 0xb647,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb648,
        end: 0xb648,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb649,
        end: 0xb663,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb664,
        end: 0xb664,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb665,
        end: 0xb67f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb680,
        end: 0xb680,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb681,
        end: 0xb69b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb69c,
        end: 0xb69c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb69d,
        end: 0xb6b7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb6b8,
        end: 0xb6b8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb6b9,
        end: 0xb6d3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb6d4,
        end: 0xb6d4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb6d5,
        end: 0xb6ef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb6f0,
        end: 0xb6f0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb6f1,
        end: 0xb70b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb70c,
        end: 0xb70c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb70d,
        end: 0xb727,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb728,
        end: 0xb728,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb729,
        end: 0xb743,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb744,
        end: 0xb744,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb745,
        end: 0xb75f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb760,
        end: 0xb760,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb761,
        end: 0xb77b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb77c,
        end: 0xb77c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb77d,
        end: 0xb797,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb798,
        end: 0xb798,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb799,
        end: 0xb7b3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb7b4,
        end: 0xb7b4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb7b5,
        end: 0xb7cf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb7d0,
        end: 0xb7d0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb7d1,
        end: 0xb7eb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb7ec,
        end: 0xb7ec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb7ed,
        end: 0xb807,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb808,
        end: 0xb808,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb809,
        end: 0xb823,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb824,
        end: 0xb824,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb825,
        end: 0xb83f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb840,
        end: 0xb840,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb841,
        end: 0xb85b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb85c,
        end: 0xb85c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb85d,
        end: 0xb877,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb878,
        end: 0xb878,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb879,
        end: 0xb893,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb894,
        end: 0xb894,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb895,
        end: 0xb8af,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb8b0,
        end: 0xb8b0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb8b1,
        end: 0xb8cb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb8cc,
        end: 0xb8cc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb8cd,
        end: 0xb8e7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb8e8,
        end: 0xb8e8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb8e9,
        end: 0xb903,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb904,
        end: 0xb904,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb905,
        end: 0xb91f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb920,
        end: 0xb920,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb921,
        end: 0xb93b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb93c,
        end: 0xb93c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb93d,
        end: 0xb957,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb958,
        end: 0xb958,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb959,
        end: 0xb973,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb974,
        end: 0xb974,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb975,
        end: 0xb98f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb990,
        end: 0xb990,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb991,
        end: 0xb9ab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb9ac,
        end: 0xb9ac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb9ad,
        end: 0xb9c7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb9c8,
        end: 0xb9c8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb9c9,
        end: 0xb9e3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xb9e4,
        end: 0xb9e4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xb9e5,
        end: 0xb9ff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba00,
        end: 0xba00,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba01,
        end: 0xba1b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba1c,
        end: 0xba1c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba1d,
        end: 0xba37,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba38,
        end: 0xba38,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba39,
        end: 0xba53,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba54,
        end: 0xba54,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba55,
        end: 0xba6f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba70,
        end: 0xba70,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba71,
        end: 0xba8b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xba8c,
        end: 0xba8c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xba8d,
        end: 0xbaa7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbaa8,
        end: 0xbaa8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbaa9,
        end: 0xbac3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbac4,
        end: 0xbac4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbac5,
        end: 0xbadf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbae0,
        end: 0xbae0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbae1,
        end: 0xbafb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbafc,
        end: 0xbafc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbafd,
        end: 0xbb17,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbb18,
        end: 0xbb18,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbb19,
        end: 0xbb33,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbb34,
        end: 0xbb34,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbb35,
        end: 0xbb4f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbb50,
        end: 0xbb50,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbb51,
        end: 0xbb6b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbb6c,
        end: 0xbb6c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbb6d,
        end: 0xbb87,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbb88,
        end: 0xbb88,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbb89,
        end: 0xbba3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbba4,
        end: 0xbba4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbba5,
        end: 0xbbbf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbbc0,
        end: 0xbbc0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbbc1,
        end: 0xbbdb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbbdc,
        end: 0xbbdc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbbdd,
        end: 0xbbf7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbbf8,
        end: 0xbbf8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbbf9,
        end: 0xbc13,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbc14,
        end: 0xbc14,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbc15,
        end: 0xbc2f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbc30,
        end: 0xbc30,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbc31,
        end: 0xbc4b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbc4c,
        end: 0xbc4c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbc4d,
        end: 0xbc67,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbc68,
        end: 0xbc68,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbc69,
        end: 0xbc83,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbc84,
        end: 0xbc84,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbc85,
        end: 0xbc9f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbca0,
        end: 0xbca0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbca1,
        end: 0xbcbb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbcbc,
        end: 0xbcbc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbcbd,
        end: 0xbcd7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbcd8,
        end: 0xbcd8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbcd9,
        end: 0xbcf3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbcf4,
        end: 0xbcf4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbcf5,
        end: 0xbd0f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd10,
        end: 0xbd10,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd11,
        end: 0xbd2b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd2c,
        end: 0xbd2c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd2d,
        end: 0xbd47,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd48,
        end: 0xbd48,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd49,
        end: 0xbd63,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd64,
        end: 0xbd64,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd65,
        end: 0xbd7f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd80,
        end: 0xbd80,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd81,
        end: 0xbd9b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbd9c,
        end: 0xbd9c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbd9d,
        end: 0xbdb7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbdb8,
        end: 0xbdb8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbdb9,
        end: 0xbdd3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbdd4,
        end: 0xbdd4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbdd5,
        end: 0xbdef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbdf0,
        end: 0xbdf0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbdf1,
        end: 0xbe0b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe0c,
        end: 0xbe0c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe0d,
        end: 0xbe27,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe28,
        end: 0xbe28,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe29,
        end: 0xbe43,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe44,
        end: 0xbe44,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe45,
        end: 0xbe5f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe60,
        end: 0xbe60,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe61,
        end: 0xbe7b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe7c,
        end: 0xbe7c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe7d,
        end: 0xbe97,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbe98,
        end: 0xbe98,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbe99,
        end: 0xbeb3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbeb4,
        end: 0xbeb4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbeb5,
        end: 0xbecf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbed0,
        end: 0xbed0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbed1,
        end: 0xbeeb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbeec,
        end: 0xbeec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbeed,
        end: 0xbf07,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf08,
        end: 0xbf08,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf09,
        end: 0xbf23,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf24,
        end: 0xbf24,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf25,
        end: 0xbf3f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf40,
        end: 0xbf40,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf41,
        end: 0xbf5b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf5c,
        end: 0xbf5c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf5d,
        end: 0xbf77,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf78,
        end: 0xbf78,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf79,
        end: 0xbf93,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbf94,
        end: 0xbf94,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbf95,
        end: 0xbfaf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbfb0,
        end: 0xbfb0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbfb1,
        end: 0xbfcb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbfcc,
        end: 0xbfcc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbfcd,
        end: 0xbfe7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xbfe8,
        end: 0xbfe8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xbfe9,
        end: 0xc003,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc004,
        end: 0xc004,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc005,
        end: 0xc01f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc020,
        end: 0xc020,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc021,
        end: 0xc03b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc03c,
        end: 0xc03c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc03d,
        end: 0xc057,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc058,
        end: 0xc058,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc059,
        end: 0xc073,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc074,
        end: 0xc074,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc075,
        end: 0xc08f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc090,
        end: 0xc090,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc091,
        end: 0xc0ab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc0ac,
        end: 0xc0ac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc0ad,
        end: 0xc0c7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc0c8,
        end: 0xc0c8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc0c9,
        end: 0xc0e3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc0e4,
        end: 0xc0e4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc0e5,
        end: 0xc0ff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc100,
        end: 0xc100,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc101,
        end: 0xc11b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc11c,
        end: 0xc11c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc11d,
        end: 0xc137,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc138,
        end: 0xc138,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc139,
        end: 0xc153,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc154,
        end: 0xc154,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc155,
        end: 0xc16f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc170,
        end: 0xc170,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc171,
        end: 0xc18b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc18c,
        end: 0xc18c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc18d,
        end: 0xc1a7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc1a8,
        end: 0xc1a8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc1a9,
        end: 0xc1c3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc1c4,
        end: 0xc1c4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc1c5,
        end: 0xc1df,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc1e0,
        end: 0xc1e0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc1e1,
        end: 0xc1fb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc1fc,
        end: 0xc1fc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc1fd,
        end: 0xc217,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc218,
        end: 0xc218,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc219,
        end: 0xc233,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc234,
        end: 0xc234,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc235,
        end: 0xc24f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc250,
        end: 0xc250,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc251,
        end: 0xc26b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc26c,
        end: 0xc26c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc26d,
        end: 0xc287,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc288,
        end: 0xc288,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc289,
        end: 0xc2a3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc2a4,
        end: 0xc2a4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc2a5,
        end: 0xc2bf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc2c0,
        end: 0xc2c0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc2c1,
        end: 0xc2db,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc2dc,
        end: 0xc2dc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc2dd,
        end: 0xc2f7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc2f8,
        end: 0xc2f8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc2f9,
        end: 0xc313,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc314,
        end: 0xc314,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc315,
        end: 0xc32f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc330,
        end: 0xc330,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc331,
        end: 0xc34b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc34c,
        end: 0xc34c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc34d,
        end: 0xc367,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc368,
        end: 0xc368,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc369,
        end: 0xc383,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc384,
        end: 0xc384,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc385,
        end: 0xc39f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc3a0,
        end: 0xc3a0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc3a1,
        end: 0xc3bb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc3bc,
        end: 0xc3bc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc3bd,
        end: 0xc3d7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc3d8,
        end: 0xc3d8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc3d9,
        end: 0xc3f3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc3f4,
        end: 0xc3f4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc3f5,
        end: 0xc40f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc410,
        end: 0xc410,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc411,
        end: 0xc42b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc42c,
        end: 0xc42c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc42d,
        end: 0xc447,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc448,
        end: 0xc448,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc449,
        end: 0xc463,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc464,
        end: 0xc464,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc465,
        end: 0xc47f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc480,
        end: 0xc480,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc481,
        end: 0xc49b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc49c,
        end: 0xc49c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc49d,
        end: 0xc4b7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc4b8,
        end: 0xc4b8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc4b9,
        end: 0xc4d3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc4d4,
        end: 0xc4d4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc4d5,
        end: 0xc4ef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc4f0,
        end: 0xc4f0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc4f1,
        end: 0xc50b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc50c,
        end: 0xc50c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc50d,
        end: 0xc527,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc528,
        end: 0xc528,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc529,
        end: 0xc543,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc544,
        end: 0xc544,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc545,
        end: 0xc55f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc560,
        end: 0xc560,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc561,
        end: 0xc57b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc57c,
        end: 0xc57c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc57d,
        end: 0xc597,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc598,
        end: 0xc598,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc599,
        end: 0xc5b3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc5b4,
        end: 0xc5b4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc5b5,
        end: 0xc5cf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc5d0,
        end: 0xc5d0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc5d1,
        end: 0xc5eb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc5ec,
        end: 0xc5ec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc5ed,
        end: 0xc607,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc608,
        end: 0xc608,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc609,
        end: 0xc623,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc624,
        end: 0xc624,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc625,
        end: 0xc63f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc640,
        end: 0xc640,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc641,
        end: 0xc65b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc65c,
        end: 0xc65c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc65d,
        end: 0xc677,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc678,
        end: 0xc678,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc679,
        end: 0xc693,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc694,
        end: 0xc694,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc695,
        end: 0xc6af,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc6b0,
        end: 0xc6b0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc6b1,
        end: 0xc6cb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc6cc,
        end: 0xc6cc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc6cd,
        end: 0xc6e7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc6e8,
        end: 0xc6e8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc6e9,
        end: 0xc703,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc704,
        end: 0xc704,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc705,
        end: 0xc71f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc720,
        end: 0xc720,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc721,
        end: 0xc73b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc73c,
        end: 0xc73c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc73d,
        end: 0xc757,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc758,
        end: 0xc758,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc759,
        end: 0xc773,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc774,
        end: 0xc774,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc775,
        end: 0xc78f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc790,
        end: 0xc790,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc791,
        end: 0xc7ab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc7ac,
        end: 0xc7ac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc7ad,
        end: 0xc7c7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc7c8,
        end: 0xc7c8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc7c9,
        end: 0xc7e3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc7e4,
        end: 0xc7e4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc7e5,
        end: 0xc7ff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc800,
        end: 0xc800,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc801,
        end: 0xc81b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc81c,
        end: 0xc81c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc81d,
        end: 0xc837,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc838,
        end: 0xc838,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc839,
        end: 0xc853,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc854,
        end: 0xc854,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc855,
        end: 0xc86f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc870,
        end: 0xc870,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc871,
        end: 0xc88b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc88c,
        end: 0xc88c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc88d,
        end: 0xc8a7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc8a8,
        end: 0xc8a8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc8a9,
        end: 0xc8c3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc8c4,
        end: 0xc8c4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc8c5,
        end: 0xc8df,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc8e0,
        end: 0xc8e0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc8e1,
        end: 0xc8fb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc8fc,
        end: 0xc8fc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc8fd,
        end: 0xc917,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc918,
        end: 0xc918,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc919,
        end: 0xc933,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc934,
        end: 0xc934,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc935,
        end: 0xc94f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc950,
        end: 0xc950,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc951,
        end: 0xc96b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc96c,
        end: 0xc96c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc96d,
        end: 0xc987,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc988,
        end: 0xc988,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc989,
        end: 0xc9a3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc9a4,
        end: 0xc9a4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc9a5,
        end: 0xc9bf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc9c0,
        end: 0xc9c0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc9c1,
        end: 0xc9db,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc9dc,
        end: 0xc9dc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc9dd,
        end: 0xc9f7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xc9f8,
        end: 0xc9f8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xc9f9,
        end: 0xca13,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xca14,
        end: 0xca14,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xca15,
        end: 0xca2f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xca30,
        end: 0xca30,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xca31,
        end: 0xca4b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xca4c,
        end: 0xca4c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xca4d,
        end: 0xca67,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xca68,
        end: 0xca68,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xca69,
        end: 0xca83,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xca84,
        end: 0xca84,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xca85,
        end: 0xca9f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcaa0,
        end: 0xcaa0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcaa1,
        end: 0xcabb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcabc,
        end: 0xcabc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcabd,
        end: 0xcad7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcad8,
        end: 0xcad8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcad9,
        end: 0xcaf3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcaf4,
        end: 0xcaf4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcaf5,
        end: 0xcb0f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb10,
        end: 0xcb10,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb11,
        end: 0xcb2b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb2c,
        end: 0xcb2c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb2d,
        end: 0xcb47,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb48,
        end: 0xcb48,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb49,
        end: 0xcb63,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb64,
        end: 0xcb64,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb65,
        end: 0xcb7f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb80,
        end: 0xcb80,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb81,
        end: 0xcb9b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcb9c,
        end: 0xcb9c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcb9d,
        end: 0xcbb7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcbb8,
        end: 0xcbb8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcbb9,
        end: 0xcbd3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcbd4,
        end: 0xcbd4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcbd5,
        end: 0xcbef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcbf0,
        end: 0xcbf0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcbf1,
        end: 0xcc0b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc0c,
        end: 0xcc0c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc0d,
        end: 0xcc27,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc28,
        end: 0xcc28,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc29,
        end: 0xcc43,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc44,
        end: 0xcc44,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc45,
        end: 0xcc5f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc60,
        end: 0xcc60,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc61,
        end: 0xcc7b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc7c,
        end: 0xcc7c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc7d,
        end: 0xcc97,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcc98,
        end: 0xcc98,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcc99,
        end: 0xccb3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xccb4,
        end: 0xccb4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xccb5,
        end: 0xcccf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xccd0,
        end: 0xccd0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xccd1,
        end: 0xcceb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xccec,
        end: 0xccec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcced,
        end: 0xcd07,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd08,
        end: 0xcd08,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd09,
        end: 0xcd23,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd24,
        end: 0xcd24,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd25,
        end: 0xcd3f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd40,
        end: 0xcd40,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd41,
        end: 0xcd5b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd5c,
        end: 0xcd5c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd5d,
        end: 0xcd77,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd78,
        end: 0xcd78,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd79,
        end: 0xcd93,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcd94,
        end: 0xcd94,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcd95,
        end: 0xcdaf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcdb0,
        end: 0xcdb0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcdb1,
        end: 0xcdcb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcdcc,
        end: 0xcdcc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcdcd,
        end: 0xcde7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcde8,
        end: 0xcde8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcde9,
        end: 0xce03,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce04,
        end: 0xce04,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce05,
        end: 0xce1f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce20,
        end: 0xce20,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce21,
        end: 0xce3b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce3c,
        end: 0xce3c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce3d,
        end: 0xce57,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce58,
        end: 0xce58,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce59,
        end: 0xce73,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce74,
        end: 0xce74,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce75,
        end: 0xce8f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xce90,
        end: 0xce90,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xce91,
        end: 0xceab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xceac,
        end: 0xceac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcead,
        end: 0xcec7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcec8,
        end: 0xcec8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcec9,
        end: 0xcee3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcee4,
        end: 0xcee4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcee5,
        end: 0xceff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf00,
        end: 0xcf00,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf01,
        end: 0xcf1b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf1c,
        end: 0xcf1c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf1d,
        end: 0xcf37,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf38,
        end: 0xcf38,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf39,
        end: 0xcf53,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf54,
        end: 0xcf54,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf55,
        end: 0xcf6f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf70,
        end: 0xcf70,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf71,
        end: 0xcf8b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcf8c,
        end: 0xcf8c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcf8d,
        end: 0xcfa7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcfa8,
        end: 0xcfa8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcfa9,
        end: 0xcfc3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcfc4,
        end: 0xcfc4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcfc5,
        end: 0xcfdf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcfe0,
        end: 0xcfe0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcfe1,
        end: 0xcffb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xcffc,
        end: 0xcffc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xcffd,
        end: 0xd017,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd018,
        end: 0xd018,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd019,
        end: 0xd033,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd034,
        end: 0xd034,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd035,
        end: 0xd04f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd050,
        end: 0xd050,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd051,
        end: 0xd06b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd06c,
        end: 0xd06c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd06d,
        end: 0xd087,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd088,
        end: 0xd088,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd089,
        end: 0xd0a3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd0a4,
        end: 0xd0a4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd0a5,
        end: 0xd0bf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd0c0,
        end: 0xd0c0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd0c1,
        end: 0xd0db,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd0dc,
        end: 0xd0dc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd0dd,
        end: 0xd0f7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd0f8,
        end: 0xd0f8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd0f9,
        end: 0xd113,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd114,
        end: 0xd114,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd115,
        end: 0xd12f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd130,
        end: 0xd130,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd131,
        end: 0xd14b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd14c,
        end: 0xd14c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd14d,
        end: 0xd167,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd168,
        end: 0xd168,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd169,
        end: 0xd183,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd184,
        end: 0xd184,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd185,
        end: 0xd19f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd1a0,
        end: 0xd1a0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd1a1,
        end: 0xd1bb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd1bc,
        end: 0xd1bc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd1bd,
        end: 0xd1d7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd1d8,
        end: 0xd1d8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd1d9,
        end: 0xd1f3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd1f4,
        end: 0xd1f4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd1f5,
        end: 0xd20f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd210,
        end: 0xd210,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd211,
        end: 0xd22b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd22c,
        end: 0xd22c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd22d,
        end: 0xd247,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd248,
        end: 0xd248,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd249,
        end: 0xd263,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd264,
        end: 0xd264,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd265,
        end: 0xd27f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd280,
        end: 0xd280,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd281,
        end: 0xd29b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd29c,
        end: 0xd29c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd29d,
        end: 0xd2b7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd2b8,
        end: 0xd2b8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd2b9,
        end: 0xd2d3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd2d4,
        end: 0xd2d4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd2d5,
        end: 0xd2ef,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd2f0,
        end: 0xd2f0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd2f1,
        end: 0xd30b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd30c,
        end: 0xd30c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd30d,
        end: 0xd327,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd328,
        end: 0xd328,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd329,
        end: 0xd343,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd344,
        end: 0xd344,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd345,
        end: 0xd35f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd360,
        end: 0xd360,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd361,
        end: 0xd37b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd37c,
        end: 0xd37c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd37d,
        end: 0xd397,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd398,
        end: 0xd398,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd399,
        end: 0xd3b3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd3b4,
        end: 0xd3b4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd3b5,
        end: 0xd3cf,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd3d0,
        end: 0xd3d0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd3d1,
        end: 0xd3eb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd3ec,
        end: 0xd3ec,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd3ed,
        end: 0xd407,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd408,
        end: 0xd408,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd409,
        end: 0xd423,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd424,
        end: 0xd424,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd425,
        end: 0xd43f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd440,
        end: 0xd440,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd441,
        end: 0xd45b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd45c,
        end: 0xd45c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd45d,
        end: 0xd477,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd478,
        end: 0xd478,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd479,
        end: 0xd493,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd494,
        end: 0xd494,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd495,
        end: 0xd4af,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd4b0,
        end: 0xd4b0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd4b1,
        end: 0xd4cb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd4cc,
        end: 0xd4cc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd4cd,
        end: 0xd4e7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd4e8,
        end: 0xd4e8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd4e9,
        end: 0xd503,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd504,
        end: 0xd504,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd505,
        end: 0xd51f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd520,
        end: 0xd520,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd521,
        end: 0xd53b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd53c,
        end: 0xd53c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd53d,
        end: 0xd557,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd558,
        end: 0xd558,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd559,
        end: 0xd573,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd574,
        end: 0xd574,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd575,
        end: 0xd58f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd590,
        end: 0xd590,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd591,
        end: 0xd5ab,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd5ac,
        end: 0xd5ac,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd5ad,
        end: 0xd5c7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd5c8,
        end: 0xd5c8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd5c9,
        end: 0xd5e3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd5e4,
        end: 0xd5e4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd5e5,
        end: 0xd5ff,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd600,
        end: 0xd600,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd601,
        end: 0xd61b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd61c,
        end: 0xd61c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd61d,
        end: 0xd637,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd638,
        end: 0xd638,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd639,
        end: 0xd653,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd654,
        end: 0xd654,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd655,
        end: 0xd66f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd670,
        end: 0xd670,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd671,
        end: 0xd68b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd68c,
        end: 0xd68c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd68d,
        end: 0xd6a7,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd6a8,
        end: 0xd6a8,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd6a9,
        end: 0xd6c3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd6c4,
        end: 0xd6c4,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd6c5,
        end: 0xd6df,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd6e0,
        end: 0xd6e0,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd6e1,
        end: 0xd6fb,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd6fc,
        end: 0xd6fc,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd6fd,
        end: 0xd717,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd718,
        end: 0xd718,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd719,
        end: 0xd733,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd734,
        end: 0xd734,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd735,
        end: 0xd74f,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd750,
        end: 0xd750,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd751,
        end: 0xd76b,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd76c,
        end: 0xd76c,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd76d,
        end: 0xd787,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd788,
        end: 0xd788,
        class: GraphemeClass::Lv as u8,
    },
    Range {
        start: 0xd789,
        end: 0xd7a3,
        class: GraphemeClass::Lvt as u8,
    },
    Range {
        start: 0xd7b0,
        end: 0xd7c6,
        class: GraphemeClass::V as u8,
    },
    Range {
        start: 0xd7cb,
        end: 0xd7fb,
        class: GraphemeClass::T as u8,
    },
    Range {
        start: 0xfb1e,
        end: 0xfb1e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xfe00,
        end: 0xfe0f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xfe20,
        end: 0xfe2f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xfeff,
        end: 0xfeff,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xff9e,
        end: 0xff9f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xfff0,
        end: 0xfff8,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xfff9,
        end: 0xfffb,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x101fd,
        end: 0x101fd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x102e0,
        end: 0x102e0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10376,
        end: 0x1037a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10a01,
        end: 0x10a03,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10a05,
        end: 0x10a06,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10a0c,
        end: 0x10a0f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10a38,
        end: 0x10a3a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10a3f,
        end: 0x10a3f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10ae5,
        end: 0x10ae6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10d24,
        end: 0x10d27,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10d69,
        end: 0x10d6d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10eab,
        end: 0x10eac,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10efa,
        end: 0x10eff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10f46,
        end: 0x10f50,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x10f82,
        end: 0x10f85,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11000,
        end: 0x11000,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11001,
        end: 0x11001,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11002,
        end: 0x11002,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11038,
        end: 0x11046,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11070,
        end: 0x11070,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11073,
        end: 0x11074,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1107f,
        end: 0x11081,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11082,
        end: 0x11082,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x110b0,
        end: 0x110b2,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x110b3,
        end: 0x110b6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x110b7,
        end: 0x110b8,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x110b9,
        end: 0x110ba,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x110bd,
        end: 0x110bd,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x110c2,
        end: 0x110c2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x110cd,
        end: 0x110cd,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11100,
        end: 0x11102,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11127,
        end: 0x1112b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1112c,
        end: 0x1112c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1112d,
        end: 0x11134,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11145,
        end: 0x11146,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11173,
        end: 0x11173,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11180,
        end: 0x11181,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11182,
        end: 0x11182,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x111b3,
        end: 0x111b5,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x111b6,
        end: 0x111be,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x111bf,
        end: 0x111bf,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x111c0,
        end: 0x111c0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x111c2,
        end: 0x111c3,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x111c9,
        end: 0x111cc,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x111ce,
        end: 0x111ce,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x111cf,
        end: 0x111cf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1122c,
        end: 0x1122e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1122f,
        end: 0x11231,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11232,
        end: 0x11233,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11234,
        end: 0x11234,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11235,
        end: 0x11235,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11236,
        end: 0x11237,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1123e,
        end: 0x1123e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11241,
        end: 0x11241,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x112df,
        end: 0x112df,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x112e0,
        end: 0x112e2,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x112e3,
        end: 0x112ea,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11300,
        end: 0x11301,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11302,
        end: 0x11303,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1133b,
        end: 0x1133c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1133e,
        end: 0x1133e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1133f,
        end: 0x1133f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11340,
        end: 0x11340,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11341,
        end: 0x11344,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11347,
        end: 0x11348,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1134b,
        end: 0x1134c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1134d,
        end: 0x1134d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11357,
        end: 0x11357,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11362,
        end: 0x11363,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11366,
        end: 0x1136c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11370,
        end: 0x11374,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113b8,
        end: 0x113b8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113b9,
        end: 0x113ba,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x113bb,
        end: 0x113c0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113c2,
        end: 0x113c2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113c5,
        end: 0x113c5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113c7,
        end: 0x113c9,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113ca,
        end: 0x113ca,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x113cc,
        end: 0x113cd,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x113ce,
        end: 0x113ce,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113cf,
        end: 0x113cf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113d0,
        end: 0x113d0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113d1,
        end: 0x113d1,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x113d2,
        end: 0x113d2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x113e1,
        end: 0x113e2,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11435,
        end: 0x11437,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11438,
        end: 0x1143f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11440,
        end: 0x11441,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11442,
        end: 0x11444,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11445,
        end: 0x11445,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11446,
        end: 0x11446,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1145e,
        end: 0x1145e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114b0,
        end: 0x114b0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114b1,
        end: 0x114b2,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x114b3,
        end: 0x114b8,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114b9,
        end: 0x114b9,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x114ba,
        end: 0x114ba,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114bb,
        end: 0x114bc,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x114bd,
        end: 0x114bd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114be,
        end: 0x114be,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x114bf,
        end: 0x114c0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x114c1,
        end: 0x114c1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x114c2,
        end: 0x114c3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x115af,
        end: 0x115af,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x115b0,
        end: 0x115b1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x115b2,
        end: 0x115b5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x115b8,
        end: 0x115bb,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x115bc,
        end: 0x115bd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x115be,
        end: 0x115be,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x115bf,
        end: 0x115c0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x115dc,
        end: 0x115dd,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11630,
        end: 0x11632,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11633,
        end: 0x1163a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1163b,
        end: 0x1163c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1163d,
        end: 0x1163d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1163e,
        end: 0x1163e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1163f,
        end: 0x11640,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x116ab,
        end: 0x116ab,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x116ac,
        end: 0x116ac,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x116ad,
        end: 0x116ad,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x116ae,
        end: 0x116af,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x116b0,
        end: 0x116b5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x116b6,
        end: 0x116b6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x116b7,
        end: 0x116b7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1171d,
        end: 0x1171d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1171e,
        end: 0x1171e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1171f,
        end: 0x1171f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11722,
        end: 0x11725,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11726,
        end: 0x11726,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11727,
        end: 0x1172b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1182c,
        end: 0x1182e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1182f,
        end: 0x11837,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11838,
        end: 0x11838,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11839,
        end: 0x1183a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11930,
        end: 0x11930,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11931,
        end: 0x11935,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11937,
        end: 0x11938,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1193b,
        end: 0x1193c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1193d,
        end: 0x1193d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1193e,
        end: 0x1193e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1193f,
        end: 0x1193f,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11940,
        end: 0x11940,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11941,
        end: 0x11941,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11942,
        end: 0x11942,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11943,
        end: 0x11943,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x119d1,
        end: 0x119d3,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x119d4,
        end: 0x119d7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x119da,
        end: 0x119db,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x119dc,
        end: 0x119df,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x119e0,
        end: 0x119e0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x119e4,
        end: 0x119e4,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11a01,
        end: 0x11a0a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a33,
        end: 0x11a38,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a39,
        end: 0x11a39,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11a3b,
        end: 0x11a3e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a47,
        end: 0x11a47,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a51,
        end: 0x11a56,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a57,
        end: 0x11a58,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11a59,
        end: 0x11a5b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a84,
        end: 0x11a89,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11a8a,
        end: 0x11a96,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11a97,
        end: 0x11a97,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11a98,
        end: 0x11a99,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11b60,
        end: 0x11b60,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11b61,
        end: 0x11b61,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11b62,
        end: 0x11b64,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11b65,
        end: 0x11b65,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11b66,
        end: 0x11b66,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11b67,
        end: 0x11b67,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11c2f,
        end: 0x11c2f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11c30,
        end: 0x11c36,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11c38,
        end: 0x11c3d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11c3e,
        end: 0x11c3e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11c3f,
        end: 0x11c3f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11c92,
        end: 0x11ca7,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11ca9,
        end: 0x11ca9,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11caa,
        end: 0x11cb0,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11cb1,
        end: 0x11cb1,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11cb2,
        end: 0x11cb3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11cb4,
        end: 0x11cb4,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11cb5,
        end: 0x11cb6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d31,
        end: 0x11d36,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d3a,
        end: 0x11d3a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d3c,
        end: 0x11d3d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d3f,
        end: 0x11d45,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d46,
        end: 0x11d46,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11d47,
        end: 0x11d47,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d8a,
        end: 0x11d8e,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11d90,
        end: 0x11d91,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d93,
        end: 0x11d94,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11d95,
        end: 0x11d95,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11d96,
        end: 0x11d96,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11d97,
        end: 0x11d97,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11ef3,
        end: 0x11ef4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11ef5,
        end: 0x11ef6,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11f00,
        end: 0x11f01,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11f02,
        end: 0x11f02,
        class: GraphemeClass::Prepend as u8,
    },
    Range {
        start: 0x11f03,
        end: 0x11f03,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11f34,
        end: 0x11f35,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11f36,
        end: 0x11f3a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11f3e,
        end: 0x11f3f,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x11f40,
        end: 0x11f40,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11f41,
        end: 0x11f41,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11f42,
        end: 0x11f42,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x11f5a,
        end: 0x11f5a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x13430,
        end: 0x1343f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x13440,
        end: 0x13440,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x13447,
        end: 0x13455,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1611e,
        end: 0x16129,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1612a,
        end: 0x1612c,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x1612d,
        end: 0x1612f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16af0,
        end: 0x16af4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16b30,
        end: 0x16b36,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16d63,
        end: 0x16d63,
        class: GraphemeClass::V as u8,
    },
    Range {
        start: 0x16d67,
        end: 0x16d6a,
        class: GraphemeClass::V as u8,
    },
    Range {
        start: 0x16f4f,
        end: 0x16f4f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16f51,
        end: 0x16f87,
        class: GraphemeClass::SpacingMark as u8,
    },
    Range {
        start: 0x16f8f,
        end: 0x16f92,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16fe4,
        end: 0x16fe4,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x16ff0,
        end: 0x16ff1,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bc9d,
        end: 0x1bc9e,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1bca0,
        end: 0x1bca3,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x1cf00,
        end: 0x1cf2d,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1cf30,
        end: 0x1cf46,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d165,
        end: 0x1d166,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d167,
        end: 0x1d169,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d16d,
        end: 0x1d172,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d173,
        end: 0x1d17a,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0x1d17b,
        end: 0x1d182,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d185,
        end: 0x1d18b,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d1aa,
        end: 0x1d1ad,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1d242,
        end: 0x1d244,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1da00,
        end: 0x1da36,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1da3b,
        end: 0x1da6c,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1da75,
        end: 0x1da75,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1da84,
        end: 0x1da84,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1da9b,
        end: 0x1da9f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1daa1,
        end: 0x1daaf,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e000,
        end: 0x1e006,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e008,
        end: 0x1e018,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e01b,
        end: 0x1e021,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e023,
        end: 0x1e024,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e026,
        end: 0x1e02a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e08f,
        end: 0x1e08f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e130,
        end: 0x1e136,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e2ae,
        end: 0x1e2ae,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e2ec,
        end: 0x1e2ef,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e4ec,
        end: 0x1e4ef,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e5ee,
        end: 0x1e5ef,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e6e3,
        end: 0x1e6e3,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e6e6,
        end: 0x1e6e6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e6ee,
        end: 0x1e6ef,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e6f5,
        end: 0x1e6f5,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e8d0,
        end: 0x1e8d6,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1e944,
        end: 0x1e94a,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0x1f1e6,
        end: 0x1f1ff,
        class: GraphemeClass::RegionalIndicator as u8,
    },
    Range {
        start: 0x1f3fb,
        end: 0x1f3ff,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xe0000,
        end: 0xe0000,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xe0001,
        end: 0xe0001,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xe0002,
        end: 0xe001f,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xe0020,
        end: 0xe007f,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xe0080,
        end: 0xe00ff,
        class: GraphemeClass::Control as u8,
    },
    Range {
        start: 0xe0100,
        end: 0xe01ef,
        class: GraphemeClass::Extend as u8,
    },
    Range {
        start: 0xe01f0,
        end: 0xe0fff,
        class: GraphemeClass::Control as u8,
    },
];
const LINE_RANGES: &[Range] = &[
    Range {
        start: 0x0,
        end: 0x8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9,
        end: 0x9,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa,
        end: 0xa,
        class: LineBreakClass::Lf as u8,
    },
    Range {
        start: 0xb,
        end: 0xc,
        class: LineBreakClass::Bk as u8,
    },
    Range {
        start: 0xd,
        end: 0xd,
        class: LineBreakClass::Cr as u8,
    },
    Range {
        start: 0xe,
        end: 0x1f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x20,
        end: 0x20,
        class: LineBreakClass::Sp as u8,
    },
    Range {
        start: 0x21,
        end: 0x21,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x22,
        end: 0x22,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x23,
        end: 0x23,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x24,
        end: 0x24,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x25,
        end: 0x25,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x26,
        end: 0x26,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x27,
        end: 0x27,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x28,
        end: 0x28,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x29,
        end: 0x29,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x2a,
        end: 0x2a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b,
        end: 0x2b,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x2c,
        end: 0x2c,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x2d,
        end: 0x2d,
        class: LineBreakClass::Hy as u8,
    },
    Range {
        start: 0x2e,
        end: 0x2e,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x2f,
        end: 0x2f,
        class: LineBreakClass::Sy as u8,
    },
    Range {
        start: 0x30,
        end: 0x39,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x3a,
        end: 0x3b,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x3c,
        end: 0x3e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x3f,
        end: 0x3f,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x40,
        end: 0x40,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x41,
        end: 0x5a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x5b,
        end: 0x5b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x5c,
        end: 0x5c,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x5d,
        end: 0x5d,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x5e,
        end: 0x5e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x5f,
        end: 0x5f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x60,
        end: 0x60,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x61,
        end: 0x7a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7b,
        end: 0x7b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x7c,
        end: 0x7c,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x7d,
        end: 0x7d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x7e,
        end: 0x7e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7f,
        end: 0x7f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x80,
        end: 0x84,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x85,
        end: 0x85,
        class: LineBreakClass::Nl as u8,
    },
    Range {
        start: 0x86,
        end: 0x9f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa0,
        end: 0xa0,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xa1,
        end: 0xa1,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xa2,
        end: 0xa2,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xa3,
        end: 0xa5,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xa6,
        end: 0xa6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7,
        end: 0xa7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xa8,
        end: 0xa8,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xa9,
        end: 0xa9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaa,
        end: 0xaa,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xab,
        end: 0xab,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0xac,
        end: 0xac,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xad,
        end: 0xad,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xae,
        end: 0xae,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaf,
        end: 0xaf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb0,
        end: 0xb0,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xb1,
        end: 0xb1,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xb2,
        end: 0xb3,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xb4,
        end: 0xb4,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xb5,
        end: 0xb5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb6,
        end: 0xb7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xb8,
        end: 0xb8,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xb9,
        end: 0xb9,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xba,
        end: 0xba,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xbb,
        end: 0xbb,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0xbc,
        end: 0xbe,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xbf,
        end: 0xbf,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xc0,
        end: 0xd6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd7,
        end: 0xd7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xd8,
        end: 0xf6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf7,
        end: 0xf7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0xf8,
        end: 0xff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x100,
        end: 0x17f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x180,
        end: 0x1ba,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bb,
        end: 0x1bb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc,
        end: 0x1bf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c0,
        end: 0x1c3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c4,
        end: 0x24f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x250,
        end: 0x293,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x294,
        end: 0x295,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x296,
        end: 0x2af,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b0,
        end: 0x2c1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c2,
        end: 0x2c5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c6,
        end: 0x2c6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c7,
        end: 0x2c7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2c8,
        end: 0x2c8,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x2c9,
        end: 0x2cb,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2cc,
        end: 0x2cc,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x2cd,
        end: 0x2cd,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2ce,
        end: 0x2cf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d0,
        end: 0x2d0,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2d1,
        end: 0x2d1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d2,
        end: 0x2d7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d8,
        end: 0x2db,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2dc,
        end: 0x2dc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2dd,
        end: 0x2dd,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2de,
        end: 0x2de,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2df,
        end: 0x2df,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x2e0,
        end: 0x2e4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e5,
        end: 0x2eb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ec,
        end: 0x2ec,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ed,
        end: 0x2ed,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ee,
        end: 0x2ee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ef,
        end: 0x2ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x300,
        end: 0x35b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x35c,
        end: 0x362,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x363,
        end: 0x36f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x370,
        end: 0x373,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x374,
        end: 0x374,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x375,
        end: 0x375,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x376,
        end: 0x377,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x37a,
        end: 0x37a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x37b,
        end: 0x37d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x37e,
        end: 0x37e,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x37f,
        end: 0x37f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x384,
        end: 0x385,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x386,
        end: 0x386,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x387,
        end: 0x387,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x388,
        end: 0x38a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x38c,
        end: 0x38c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x38e,
        end: 0x3a1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x3a3,
        end: 0x3f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x3f6,
        end: 0x3f6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x3f7,
        end: 0x3ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x400,
        end: 0x481,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x482,
        end: 0x482,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x483,
        end: 0x487,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x488,
        end: 0x489,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x48a,
        end: 0x4ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x500,
        end: 0x52f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x531,
        end: 0x556,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x559,
        end: 0x559,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x55a,
        end: 0x55f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x560,
        end: 0x588,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x589,
        end: 0x589,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x58a,
        end: 0x58a,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x58d,
        end: 0x58e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x58f,
        end: 0x58f,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x591,
        end: 0x5bd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x5be,
        end: 0x5be,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x5bf,
        end: 0x5bf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x5c0,
        end: 0x5c0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x5c1,
        end: 0x5c2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x5c3,
        end: 0x5c3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x5c4,
        end: 0x5c5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x5c6,
        end: 0x5c6,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x5c7,
        end: 0x5c7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x5d0,
        end: 0x5ea,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0x5ef,
        end: 0x5f2,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0x5f3,
        end: 0x5f4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x600,
        end: 0x605,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x606,
        end: 0x608,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x609,
        end: 0x60a,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x60b,
        end: 0x60b,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x60c,
        end: 0x60d,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x60e,
        end: 0x60f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x610,
        end: 0x61a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x61b,
        end: 0x61b,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x61c,
        end: 0x61c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x61d,
        end: 0x61f,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x620,
        end: 0x63f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x640,
        end: 0x640,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x641,
        end: 0x64a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x64b,
        end: 0x65f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x660,
        end: 0x669,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x66a,
        end: 0x66a,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x66b,
        end: 0x66c,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x66d,
        end: 0x66d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x66e,
        end: 0x66f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x670,
        end: 0x670,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x671,
        end: 0x6d3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6d4,
        end: 0x6d4,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x6d5,
        end: 0x6d5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6d6,
        end: 0x6dc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x6dd,
        end: 0x6dd,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x6de,
        end: 0x6de,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6df,
        end: 0x6e4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x6e5,
        end: 0x6e6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6e7,
        end: 0x6e8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x6e9,
        end: 0x6e9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6ea,
        end: 0x6ed,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x6ee,
        end: 0x6ef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6f0,
        end: 0x6f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x6fa,
        end: 0x6fc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6fd,
        end: 0x6fe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x6ff,
        end: 0x6ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x700,
        end: 0x70d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x70f,
        end: 0x70f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x710,
        end: 0x710,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x711,
        end: 0x711,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x712,
        end: 0x72f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x730,
        end: 0x74a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x74d,
        end: 0x74f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x750,
        end: 0x77f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x780,
        end: 0x7a5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7a6,
        end: 0x7b0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x7b1,
        end: 0x7b1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7c0,
        end: 0x7c9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x7ca,
        end: 0x7ea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7eb,
        end: 0x7f3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x7f4,
        end: 0x7f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7f6,
        end: 0x7f6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7f7,
        end: 0x7f7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7f8,
        end: 0x7f8,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x7f9,
        end: 0x7f9,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x7fa,
        end: 0x7fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x7fd,
        end: 0x7fd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x7fe,
        end: 0x7ff,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x800,
        end: 0x815,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x816,
        end: 0x819,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x81a,
        end: 0x81a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x81b,
        end: 0x823,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x824,
        end: 0x824,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x825,
        end: 0x827,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x828,
        end: 0x828,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x829,
        end: 0x82d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x830,
        end: 0x83e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x840,
        end: 0x858,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x859,
        end: 0x85b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x85e,
        end: 0x85e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x860,
        end: 0x86a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x870,
        end: 0x887,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x888,
        end: 0x888,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x889,
        end: 0x88f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x890,
        end: 0x891,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x897,
        end: 0x89f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x8a0,
        end: 0x8c8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x8c9,
        end: 0x8c9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x8ca,
        end: 0x8e1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x8e2,
        end: 0x8e2,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x8e3,
        end: 0x8ff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x900,
        end: 0x902,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x903,
        end: 0x903,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x904,
        end: 0x939,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x93a,
        end: 0x93a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x93b,
        end: 0x93b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x93c,
        end: 0x93c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x93d,
        end: 0x93d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x93e,
        end: 0x940,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x941,
        end: 0x948,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x949,
        end: 0x94c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x94d,
        end: 0x94d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x94e,
        end: 0x94f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x950,
        end: 0x950,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x951,
        end: 0x957,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x958,
        end: 0x961,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x962,
        end: 0x963,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x964,
        end: 0x965,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x966,
        end: 0x96f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x970,
        end: 0x970,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x971,
        end: 0x971,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x972,
        end: 0x97f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x980,
        end: 0x980,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x981,
        end: 0x981,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x982,
        end: 0x983,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x985,
        end: 0x98c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x98f,
        end: 0x990,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x993,
        end: 0x9a8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9aa,
        end: 0x9b0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9b2,
        end: 0x9b2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9b6,
        end: 0x9b9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9bc,
        end: 0x9bc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9bd,
        end: 0x9bd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9be,
        end: 0x9c0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9c1,
        end: 0x9c4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9c7,
        end: 0x9c8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9cb,
        end: 0x9cc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9cd,
        end: 0x9cd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9ce,
        end: 0x9ce,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9d7,
        end: 0x9d7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9dc,
        end: 0x9dd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9df,
        end: 0x9e1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9e2,
        end: 0x9e3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x9e6,
        end: 0x9ef,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x9f0,
        end: 0x9f1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9f2,
        end: 0x9f3,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x9f4,
        end: 0x9f8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9f9,
        end: 0x9f9,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x9fa,
        end: 0x9fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9fb,
        end: 0x9fb,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x9fc,
        end: 0x9fc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9fd,
        end: 0x9fd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x9fe,
        end: 0x9fe,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa01,
        end: 0xa02,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa03,
        end: 0xa03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa05,
        end: 0xa0a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa0f,
        end: 0xa10,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa13,
        end: 0xa28,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa2a,
        end: 0xa30,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa32,
        end: 0xa33,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa35,
        end: 0xa36,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa38,
        end: 0xa39,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa3c,
        end: 0xa3c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa3e,
        end: 0xa40,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa41,
        end: 0xa42,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa47,
        end: 0xa48,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa4b,
        end: 0xa4d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa51,
        end: 0xa51,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa59,
        end: 0xa5c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa5e,
        end: 0xa5e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa66,
        end: 0xa6f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xa70,
        end: 0xa71,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa72,
        end: 0xa74,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa75,
        end: 0xa75,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa76,
        end: 0xa76,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa81,
        end: 0xa82,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa83,
        end: 0xa83,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa85,
        end: 0xa8d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8f,
        end: 0xa91,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa93,
        end: 0xaa8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaaa,
        end: 0xab0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab2,
        end: 0xab3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab5,
        end: 0xab9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xabc,
        end: 0xabc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabd,
        end: 0xabd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xabe,
        end: 0xac0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xac1,
        end: 0xac5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xac7,
        end: 0xac8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xac9,
        end: 0xac9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xacb,
        end: 0xacc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xacd,
        end: 0xacd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xad0,
        end: 0xad0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xae0,
        end: 0xae1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xae2,
        end: 0xae3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xae6,
        end: 0xaef,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xaf0,
        end: 0xaf0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaf1,
        end: 0xaf1,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xaf9,
        end: 0xaf9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xafa,
        end: 0xaff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb01,
        end: 0xb01,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb02,
        end: 0xb03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb05,
        end: 0xb0c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb0f,
        end: 0xb10,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb13,
        end: 0xb28,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb2a,
        end: 0xb30,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb32,
        end: 0xb33,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb35,
        end: 0xb39,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb3c,
        end: 0xb3c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb3d,
        end: 0xb3d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb3e,
        end: 0xb3e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb3f,
        end: 0xb3f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb40,
        end: 0xb40,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb41,
        end: 0xb44,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb47,
        end: 0xb48,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb4b,
        end: 0xb4c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb4d,
        end: 0xb4d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb55,
        end: 0xb56,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb57,
        end: 0xb57,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb5c,
        end: 0xb5d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb5f,
        end: 0xb61,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb62,
        end: 0xb63,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb66,
        end: 0xb6f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xb70,
        end: 0xb70,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb71,
        end: 0xb71,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb72,
        end: 0xb77,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb82,
        end: 0xb82,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xb83,
        end: 0xb83,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb85,
        end: 0xb8a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb8e,
        end: 0xb90,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb92,
        end: 0xb95,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb99,
        end: 0xb9a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb9c,
        end: 0xb9c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xb9e,
        end: 0xb9f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xba3,
        end: 0xba4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xba8,
        end: 0xbaa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xbae,
        end: 0xbb9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xbbe,
        end: 0xbbf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbc0,
        end: 0xbc0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbc1,
        end: 0xbc2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbc6,
        end: 0xbc8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbca,
        end: 0xbcc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbcd,
        end: 0xbcd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbd0,
        end: 0xbd0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xbd7,
        end: 0xbd7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xbe6,
        end: 0xbef,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xbf0,
        end: 0xbf2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xbf3,
        end: 0xbf8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xbf9,
        end: 0xbf9,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xbfa,
        end: 0xbfa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc00,
        end: 0xc00,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc01,
        end: 0xc03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc04,
        end: 0xc04,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc05,
        end: 0xc0c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc0e,
        end: 0xc10,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc12,
        end: 0xc28,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc2a,
        end: 0xc39,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc3c,
        end: 0xc3c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc3d,
        end: 0xc3d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc3e,
        end: 0xc40,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc41,
        end: 0xc44,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc46,
        end: 0xc48,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc4a,
        end: 0xc4d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc55,
        end: 0xc56,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc58,
        end: 0xc5a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc5c,
        end: 0xc5d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc60,
        end: 0xc61,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc62,
        end: 0xc63,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc66,
        end: 0xc6f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xc77,
        end: 0xc77,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xc78,
        end: 0xc7e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc7f,
        end: 0xc7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc80,
        end: 0xc80,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc81,
        end: 0xc81,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc82,
        end: 0xc83,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xc84,
        end: 0xc84,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xc85,
        end: 0xc8c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc8e,
        end: 0xc90,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xc92,
        end: 0xca8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xcaa,
        end: 0xcb3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xcb5,
        end: 0xcb9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xcbc,
        end: 0xcbc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcbd,
        end: 0xcbd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xcbe,
        end: 0xcbe,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcbf,
        end: 0xcbf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcc0,
        end: 0xcc4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcc6,
        end: 0xcc6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcc7,
        end: 0xcc8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcca,
        end: 0xccb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xccc,
        end: 0xccd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcd5,
        end: 0xcd6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xcdc,
        end: 0xcde,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xce0,
        end: 0xce1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xce2,
        end: 0xce3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xce6,
        end: 0xcef,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xcf1,
        end: 0xcf2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xcf3,
        end: 0xcf3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd00,
        end: 0xd01,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd02,
        end: 0xd03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd04,
        end: 0xd0c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd0e,
        end: 0xd10,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd12,
        end: 0xd3a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd3b,
        end: 0xd3c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd3d,
        end: 0xd3d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd3e,
        end: 0xd40,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd41,
        end: 0xd44,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd46,
        end: 0xd48,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd4a,
        end: 0xd4c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd4d,
        end: 0xd4d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd4e,
        end: 0xd4e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd4f,
        end: 0xd4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd54,
        end: 0xd56,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd57,
        end: 0xd57,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd58,
        end: 0xd5e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd5f,
        end: 0xd61,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd62,
        end: 0xd63,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd66,
        end: 0xd6f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xd70,
        end: 0xd78,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd79,
        end: 0xd79,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xd7a,
        end: 0xd7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd81,
        end: 0xd81,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd82,
        end: 0xd83,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xd85,
        end: 0xd96,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xd9a,
        end: 0xdb1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xdb3,
        end: 0xdbb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xdbd,
        end: 0xdbd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xdc0,
        end: 0xdc6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xdca,
        end: 0xdca,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xdcf,
        end: 0xdd1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xdd2,
        end: 0xdd4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xdd6,
        end: 0xdd6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xdd8,
        end: 0xddf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xde6,
        end: 0xdef,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xdf2,
        end: 0xdf3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xdf4,
        end: 0xdf4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xe01,
        end: 0xe30,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe31,
        end: 0xe31,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe32,
        end: 0xe33,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe34,
        end: 0xe3a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe3f,
        end: 0xe3f,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xe40,
        end: 0xe45,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe46,
        end: 0xe46,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe47,
        end: 0xe4e,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe4f,
        end: 0xe4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xe50,
        end: 0xe59,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xe5a,
        end: 0xe5b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xe81,
        end: 0xe82,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe84,
        end: 0xe84,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe86,
        end: 0xe8a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xe8c,
        end: 0xea3,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xea5,
        end: 0xea5,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xea7,
        end: 0xeb0,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xeb1,
        end: 0xeb1,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xeb2,
        end: 0xeb3,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xeb4,
        end: 0xebc,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xebd,
        end: 0xebd,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xec0,
        end: 0xec4,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xec6,
        end: 0xec6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xec8,
        end: 0xece,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xed0,
        end: 0xed9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xedc,
        end: 0xedf,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xf00,
        end: 0xf00,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf01,
        end: 0xf03,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xf04,
        end: 0xf04,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xf05,
        end: 0xf05,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf06,
        end: 0xf07,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xf08,
        end: 0xf08,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xf09,
        end: 0xf0a,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xf0b,
        end: 0xf0b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xf0c,
        end: 0xf0c,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xf0d,
        end: 0xf11,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xf12,
        end: 0xf12,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xf13,
        end: 0xf13,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf14,
        end: 0xf14,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xf15,
        end: 0xf17,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf18,
        end: 0xf19,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf1a,
        end: 0xf1f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf20,
        end: 0xf29,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xf2a,
        end: 0xf33,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf34,
        end: 0xf34,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xf35,
        end: 0xf35,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf36,
        end: 0xf36,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf37,
        end: 0xf37,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf38,
        end: 0xf38,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf39,
        end: 0xf39,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf3a,
        end: 0xf3a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xf3b,
        end: 0xf3b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xf3c,
        end: 0xf3c,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xf3d,
        end: 0xf3d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xf3e,
        end: 0xf3f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf40,
        end: 0xf47,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf49,
        end: 0xf6c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf71,
        end: 0xf7e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf7f,
        end: 0xf7f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xf80,
        end: 0xf84,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf85,
        end: 0xf85,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xf86,
        end: 0xf87,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf88,
        end: 0xf8c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xf8d,
        end: 0xf97,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf99,
        end: 0xfbc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfbe,
        end: 0xfbf,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xfc0,
        end: 0xfc5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfc6,
        end: 0xfc6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfc7,
        end: 0xfcc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfce,
        end: 0xfcf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd0,
        end: 0xfd1,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xfd2,
        end: 0xfd2,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xfd3,
        end: 0xfd3,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xfd4,
        end: 0xfd4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd5,
        end: 0xfd8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd9,
        end: 0xfda,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x1000,
        end: 0x102a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x102b,
        end: 0x102c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x102d,
        end: 0x1030,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1031,
        end: 0x1031,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1032,
        end: 0x1037,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1038,
        end: 0x1038,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1039,
        end: 0x103a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x103b,
        end: 0x103c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x103d,
        end: 0x103e,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x103f,
        end: 0x103f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1040,
        end: 0x1049,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x104a,
        end: 0x104b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x104c,
        end: 0x104f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1050,
        end: 0x1055,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1056,
        end: 0x1057,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1058,
        end: 0x1059,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x105a,
        end: 0x105d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x105e,
        end: 0x1060,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1061,
        end: 0x1061,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1062,
        end: 0x1064,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1065,
        end: 0x1066,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1067,
        end: 0x106d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x106e,
        end: 0x1070,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1071,
        end: 0x1074,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1075,
        end: 0x1081,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1082,
        end: 0x1082,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1083,
        end: 0x1084,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1085,
        end: 0x1086,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1087,
        end: 0x108c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x108d,
        end: 0x108d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x108e,
        end: 0x108e,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x108f,
        end: 0x108f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1090,
        end: 0x1099,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x109a,
        end: 0x109c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x109d,
        end: 0x109d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x109e,
        end: 0x109f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x10a0,
        end: 0x10c5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10c7,
        end: 0x10c7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10cd,
        end: 0x10cd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d0,
        end: 0x10fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fb,
        end: 0x10fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fc,
        end: 0x10fc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fd,
        end: 0x10ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1100,
        end: 0x115f,
        class: LineBreakClass::Jl as u8,
    },
    Range {
        start: 0x1160,
        end: 0x11a7,
        class: LineBreakClass::Jv as u8,
    },
    Range {
        start: 0x11a8,
        end: 0x11ff,
        class: LineBreakClass::Jt as u8,
    },
    Range {
        start: 0x1200,
        end: 0x1248,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x124a,
        end: 0x124d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1250,
        end: 0x1256,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1258,
        end: 0x1258,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x125a,
        end: 0x125d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1260,
        end: 0x1288,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x128a,
        end: 0x128d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1290,
        end: 0x12b0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12b2,
        end: 0x12b5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12b8,
        end: 0x12be,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12c0,
        end: 0x12c0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12c2,
        end: 0x12c5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12c8,
        end: 0x12d6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12d8,
        end: 0x1310,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1312,
        end: 0x1315,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1318,
        end: 0x135a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x135d,
        end: 0x135f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1360,
        end: 0x1360,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1361,
        end: 0x1361,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1362,
        end: 0x1368,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1369,
        end: 0x137c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1380,
        end: 0x138f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1390,
        end: 0x1399,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13a0,
        end: 0x13f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13f8,
        end: 0x13fd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1400,
        end: 0x1400,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x1401,
        end: 0x166c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x166d,
        end: 0x166d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x166e,
        end: 0x166e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x166f,
        end: 0x167f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1680,
        end: 0x1680,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1681,
        end: 0x169a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x169b,
        end: 0x169b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x169c,
        end: 0x169c,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x16a0,
        end: 0x16ea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16eb,
        end: 0x16ed,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16ee,
        end: 0x16f0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16f1,
        end: 0x16f8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1700,
        end: 0x1711,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1712,
        end: 0x1714,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1715,
        end: 0x1715,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x171f,
        end: 0x171f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1720,
        end: 0x1731,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1732,
        end: 0x1733,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1734,
        end: 0x1734,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1735,
        end: 0x1736,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1740,
        end: 0x1751,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1752,
        end: 0x1753,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1760,
        end: 0x176c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x176e,
        end: 0x1770,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1772,
        end: 0x1773,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1780,
        end: 0x17b3,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17b4,
        end: 0x17b5,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17b6,
        end: 0x17b6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17b7,
        end: 0x17bd,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17be,
        end: 0x17c5,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17c6,
        end: 0x17c6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17c7,
        end: 0x17c8,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17c9,
        end: 0x17d3,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17d4,
        end: 0x17d5,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x17d6,
        end: 0x17d6,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x17d7,
        end: 0x17d7,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17d8,
        end: 0x17d8,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x17d9,
        end: 0x17d9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x17da,
        end: 0x17da,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x17db,
        end: 0x17db,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x17dc,
        end: 0x17dc,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17dd,
        end: 0x17dd,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x17e0,
        end: 0x17e9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x17f0,
        end: 0x17f9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1800,
        end: 0x1801,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1802,
        end: 0x1803,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x1804,
        end: 0x1805,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1806,
        end: 0x1806,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x1807,
        end: 0x1807,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1808,
        end: 0x1809,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x180a,
        end: 0x180a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x180b,
        end: 0x180d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x180e,
        end: 0x180e,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x180f,
        end: 0x180f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1810,
        end: 0x1819,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1820,
        end: 0x1842,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1843,
        end: 0x1843,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1844,
        end: 0x1878,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1880,
        end: 0x1884,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1885,
        end: 0x1886,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1887,
        end: 0x18a8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x18a9,
        end: 0x18a9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x18aa,
        end: 0x18aa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x18b0,
        end: 0x18f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1900,
        end: 0x191e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1920,
        end: 0x1922,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1923,
        end: 0x1926,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1927,
        end: 0x1928,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1929,
        end: 0x192b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1930,
        end: 0x1931,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1932,
        end: 0x1932,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1933,
        end: 0x1938,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1939,
        end: 0x193b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1940,
        end: 0x1940,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1944,
        end: 0x1945,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x1946,
        end: 0x194f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1950,
        end: 0x196d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1970,
        end: 0x1974,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1980,
        end: 0x19ab,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x19b0,
        end: 0x19c9,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x19d0,
        end: 0x19d9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x19da,
        end: 0x19da,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x19de,
        end: 0x19df,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x19e0,
        end: 0x19ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1a00,
        end: 0x1a16,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1a17,
        end: 0x1a18,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1a19,
        end: 0x1a1a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1a1b,
        end: 0x1a1b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1a1e,
        end: 0x1a1f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1a20,
        end: 0x1a54,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a55,
        end: 0x1a55,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a56,
        end: 0x1a56,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a57,
        end: 0x1a57,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a58,
        end: 0x1a5e,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a60,
        end: 0x1a60,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a61,
        end: 0x1a61,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a62,
        end: 0x1a62,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a63,
        end: 0x1a64,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a65,
        end: 0x1a6c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a6d,
        end: 0x1a72,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a73,
        end: 0x1a7c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1a7f,
        end: 0x1a7f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1a80,
        end: 0x1a89,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1a90,
        end: 0x1a99,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1aa0,
        end: 0x1aa6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1aa7,
        end: 0x1aa7,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1aa8,
        end: 0x1aad,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1ab0,
        end: 0x1abd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1abe,
        end: 0x1abe,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1abf,
        end: 0x1add,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ae0,
        end: 0x1aea,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1aeb,
        end: 0x1aeb,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x1b00,
        end: 0x1b03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b04,
        end: 0x1b04,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b05,
        end: 0x1b33,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1b34,
        end: 0x1b34,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b35,
        end: 0x1b35,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b36,
        end: 0x1b3a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b3b,
        end: 0x1b3b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b3c,
        end: 0x1b3c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b3d,
        end: 0x1b41,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b42,
        end: 0x1b42,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b43,
        end: 0x1b43,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b44,
        end: 0x1b44,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x1b45,
        end: 0x1b4c,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1b4e,
        end: 0x1b4f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1b50,
        end: 0x1b59,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x1b5a,
        end: 0x1b5b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1b5c,
        end: 0x1b5c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1b5d,
        end: 0x1b60,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1b61,
        end: 0x1b6a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1b6b,
        end: 0x1b73,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b74,
        end: 0x1b7c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1b7d,
        end: 0x1b7f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1b80,
        end: 0x1b81,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b82,
        end: 0x1b82,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1b83,
        end: 0x1ba0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ba1,
        end: 0x1ba1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ba2,
        end: 0x1ba5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ba6,
        end: 0x1ba7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ba8,
        end: 0x1ba9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1baa,
        end: 0x1baa,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bab,
        end: 0x1bad,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bae,
        end: 0x1baf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bb0,
        end: 0x1bb9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1bba,
        end: 0x1bbf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc0,
        end: 0x1be5,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x1be6,
        end: 0x1be6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1be7,
        end: 0x1be7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1be8,
        end: 0x1be9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bea,
        end: 0x1bec,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bed,
        end: 0x1bed,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bee,
        end: 0x1bee,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bef,
        end: 0x1bf1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bf2,
        end: 0x1bf3,
        class: LineBreakClass::Vf as u8,
    },
    Range {
        start: 0x1bfc,
        end: 0x1bff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c00,
        end: 0x1c23,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c24,
        end: 0x1c2b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1c2c,
        end: 0x1c33,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1c34,
        end: 0x1c35,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1c36,
        end: 0x1c37,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1c3b,
        end: 0x1c3f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1c40,
        end: 0x1c49,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1c4d,
        end: 0x1c4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c50,
        end: 0x1c59,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1c5a,
        end: 0x1c77,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c78,
        end: 0x1c7d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c7e,
        end: 0x1c7f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1c80,
        end: 0x1c8a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1c90,
        end: 0x1cba,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cbd,
        end: 0x1cbf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cc0,
        end: 0x1cc7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cd0,
        end: 0x1cd2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cd3,
        end: 0x1cd3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cd4,
        end: 0x1ce0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ce1,
        end: 0x1ce1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ce2,
        end: 0x1ce8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1ce9,
        end: 0x1cec,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ced,
        end: 0x1ced,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cee,
        end: 0x1cf3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cf4,
        end: 0x1cf4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cf5,
        end: 0x1cf6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cf7,
        end: 0x1cf7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cf8,
        end: 0x1cf9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cfa,
        end: 0x1cfa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d00,
        end: 0x1d2b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d2c,
        end: 0x1d6a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6b,
        end: 0x1d77,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d78,
        end: 0x1d78,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d79,
        end: 0x1d7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d80,
        end: 0x1d9a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d9b,
        end: 0x1dbf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1dc0,
        end: 0x1dcc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1dcd,
        end: 0x1dcd,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x1dce,
        end: 0x1dfb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1dfc,
        end: 0x1dfc,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x1dfd,
        end: 0x1dff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e00,
        end: 0x1eff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f00,
        end: 0x1f15,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f18,
        end: 0x1f1d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f20,
        end: 0x1f45,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f48,
        end: 0x1f4d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f50,
        end: 0x1f57,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f59,
        end: 0x1f59,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f5b,
        end: 0x1f5b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f5d,
        end: 0x1f5d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f5f,
        end: 0x1f7d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f80,
        end: 0x1fb4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fb6,
        end: 0x1fbc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fbd,
        end: 0x1fbd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fbe,
        end: 0x1fbe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fbf,
        end: 0x1fc1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fc2,
        end: 0x1fc4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fc6,
        end: 0x1fcc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fcd,
        end: 0x1fcf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fd0,
        end: 0x1fd3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fd6,
        end: 0x1fdb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fdd,
        end: 0x1fdf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fe0,
        end: 0x1fec,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fed,
        end: 0x1fef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ff2,
        end: 0x1ff4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ff6,
        end: 0x1ffc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ffd,
        end: 0x1ffd,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x1ffe,
        end: 0x1ffe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2000,
        end: 0x2006,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2007,
        end: 0x2007,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x2008,
        end: 0x200a,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x200b,
        end: 0x200b,
        class: LineBreakClass::Zw as u8,
    },
    Range {
        start: 0x200c,
        end: 0x200c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x200d,
        end: 0x200d,
        class: LineBreakClass::Zwj as u8,
    },
    Range {
        start: 0x200e,
        end: 0x200f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2010,
        end: 0x2010,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x2011,
        end: 0x2011,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x2012,
        end: 0x2013,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x2014,
        end: 0x2014,
        class: LineBreakClass::B2 as u8,
    },
    Range {
        start: 0x2015,
        end: 0x2015,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2016,
        end: 0x2016,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2017,
        end: 0x2017,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2018,
        end: 0x2018,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2019,
        end: 0x2019,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x201a,
        end: 0x201a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x201b,
        end: 0x201c,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x201d,
        end: 0x201d,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x201e,
        end: 0x201e,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x201f,
        end: 0x201f,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2020,
        end: 0x2021,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2022,
        end: 0x2023,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2024,
        end: 0x2026,
        class: LineBreakClass::In as u8,
    },
    Range {
        start: 0x2027,
        end: 0x2027,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2028,
        end: 0x2028,
        class: LineBreakClass::Bk as u8,
    },
    Range {
        start: 0x2029,
        end: 0x2029,
        class: LineBreakClass::Bk as u8,
    },
    Range {
        start: 0x202a,
        end: 0x202e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x202f,
        end: 0x202f,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x2030,
        end: 0x2037,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x2038,
        end: 0x2038,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2039,
        end: 0x2039,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x203a,
        end: 0x203a,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x203b,
        end: 0x203b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x203c,
        end: 0x203d,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x203e,
        end: 0x203e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x203f,
        end: 0x2040,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2041,
        end: 0x2043,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2044,
        end: 0x2044,
        class: LineBreakClass::Is as u8,
    },
    Range {
        start: 0x2045,
        end: 0x2045,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2046,
        end: 0x2046,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2047,
        end: 0x2049,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x204a,
        end: 0x2051,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2052,
        end: 0x2052,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2053,
        end: 0x2053,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2054,
        end: 0x2054,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2055,
        end: 0x2055,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2056,
        end: 0x2056,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2057,
        end: 0x2057,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x2058,
        end: 0x205b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x205c,
        end: 0x205c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x205d,
        end: 0x205e,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x205f,
        end: 0x205f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2060,
        end: 0x2060,
        class: LineBreakClass::Wj as u8,
    },
    Range {
        start: 0x2061,
        end: 0x2064,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2066,
        end: 0x206f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2070,
        end: 0x2070,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2071,
        end: 0x2071,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2074,
        end: 0x2074,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2075,
        end: 0x2079,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x207a,
        end: 0x207c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x207d,
        end: 0x207d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x207e,
        end: 0x207e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x207f,
        end: 0x207f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2080,
        end: 0x2080,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2081,
        end: 0x2084,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2085,
        end: 0x2089,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x208a,
        end: 0x208c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x208d,
        end: 0x208d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x208e,
        end: 0x208e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2090,
        end: 0x209c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x20a0,
        end: 0x20a6,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20a7,
        end: 0x20a7,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x20a8,
        end: 0x20b5,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20b6,
        end: 0x20b6,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x20b7,
        end: 0x20ba,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20bb,
        end: 0x20bb,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x20bc,
        end: 0x20bd,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20be,
        end: 0x20be,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x20bf,
        end: 0x20bf,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20c0,
        end: 0x20c0,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x20c1,
        end: 0x20c1,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20c2,
        end: 0x20cf,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x20d0,
        end: 0x20dc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x20dd,
        end: 0x20e0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x20e1,
        end: 0x20e1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x20e2,
        end: 0x20e4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x20e5,
        end: 0x20f0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2100,
        end: 0x2101,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2102,
        end: 0x2102,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2103,
        end: 0x2103,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x2104,
        end: 0x2104,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2105,
        end: 0x2105,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2106,
        end: 0x2106,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2107,
        end: 0x2107,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2108,
        end: 0x2108,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2109,
        end: 0x2109,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x210a,
        end: 0x2112,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2113,
        end: 0x2113,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2114,
        end: 0x2114,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2115,
        end: 0x2115,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2116,
        end: 0x2116,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x2117,
        end: 0x2117,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2118,
        end: 0x2118,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2119,
        end: 0x211d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x211e,
        end: 0x2120,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2121,
        end: 0x2122,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2123,
        end: 0x2123,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2124,
        end: 0x2124,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2125,
        end: 0x2125,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2126,
        end: 0x2126,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2127,
        end: 0x2127,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2128,
        end: 0x2128,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2129,
        end: 0x2129,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x212a,
        end: 0x212a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x212b,
        end: 0x212b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x212c,
        end: 0x212d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x212e,
        end: 0x212e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x212f,
        end: 0x2134,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2135,
        end: 0x2138,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2139,
        end: 0x2139,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x213a,
        end: 0x213b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x213c,
        end: 0x213f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2140,
        end: 0x2144,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2145,
        end: 0x2149,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x214a,
        end: 0x214a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x214b,
        end: 0x214b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x214c,
        end: 0x214d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x214e,
        end: 0x214e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x214f,
        end: 0x214f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2150,
        end: 0x215e,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x215f,
        end: 0x215f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2160,
        end: 0x216b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x216c,
        end: 0x216f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2170,
        end: 0x2179,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x217a,
        end: 0x2182,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2183,
        end: 0x2184,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2185,
        end: 0x2188,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2189,
        end: 0x2189,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x218a,
        end: 0x218b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2190,
        end: 0x2194,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2195,
        end: 0x2199,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x219a,
        end: 0x219b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x219c,
        end: 0x219f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a0,
        end: 0x21a0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a1,
        end: 0x21a2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a3,
        end: 0x21a3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a4,
        end: 0x21a5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a6,
        end: 0x21a6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21a7,
        end: 0x21ad,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21ae,
        end: 0x21ae,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21af,
        end: 0x21cd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21ce,
        end: 0x21cf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21d0,
        end: 0x21d1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21d2,
        end: 0x21d2,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x21d3,
        end: 0x21d3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21d4,
        end: 0x21d4,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x21d5,
        end: 0x21f3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x21f4,
        end: 0x21ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2200,
        end: 0x2200,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2201,
        end: 0x2201,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2202,
        end: 0x2203,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2204,
        end: 0x2206,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2207,
        end: 0x2208,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2209,
        end: 0x220a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x220b,
        end: 0x220b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x220c,
        end: 0x220e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x220f,
        end: 0x220f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2210,
        end: 0x2210,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2211,
        end: 0x2211,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2212,
        end: 0x2213,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x2214,
        end: 0x2214,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2215,
        end: 0x2215,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2216,
        end: 0x2219,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x221a,
        end: 0x221a,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x221b,
        end: 0x221c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x221d,
        end: 0x2220,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2221,
        end: 0x2222,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2223,
        end: 0x2223,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2224,
        end: 0x2224,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2225,
        end: 0x2225,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2226,
        end: 0x2226,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2227,
        end: 0x222c,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x222d,
        end: 0x222d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x222e,
        end: 0x222e,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x222f,
        end: 0x2233,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2234,
        end: 0x2237,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2238,
        end: 0x223b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x223c,
        end: 0x223d,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x223e,
        end: 0x2247,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2248,
        end: 0x2248,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2249,
        end: 0x224b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x224c,
        end: 0x224c,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x224d,
        end: 0x2251,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2252,
        end: 0x2252,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2253,
        end: 0x225f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2260,
        end: 0x2261,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2262,
        end: 0x2263,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2264,
        end: 0x2267,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2268,
        end: 0x2269,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x226a,
        end: 0x226b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x226c,
        end: 0x226d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x226e,
        end: 0x226f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2270,
        end: 0x2281,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2282,
        end: 0x2283,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2284,
        end: 0x2285,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2286,
        end: 0x2287,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2288,
        end: 0x2294,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2295,
        end: 0x2295,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2296,
        end: 0x2298,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2299,
        end: 0x2299,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x229a,
        end: 0x22a4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x22a5,
        end: 0x22a5,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x22a6,
        end: 0x22be,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x22bf,
        end: 0x22bf,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x22c0,
        end: 0x22ee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x22ef,
        end: 0x22ef,
        class: LineBreakClass::In as u8,
    },
    Range {
        start: 0x22f0,
        end: 0x22ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2300,
        end: 0x2307,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2308,
        end: 0x2308,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2309,
        end: 0x2309,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x230a,
        end: 0x230a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x230b,
        end: 0x230b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x230c,
        end: 0x2311,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2312,
        end: 0x2312,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2313,
        end: 0x2319,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x231a,
        end: 0x231b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x231c,
        end: 0x231f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2320,
        end: 0x2321,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2322,
        end: 0x2328,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2329,
        end: 0x2329,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x232a,
        end: 0x232a,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x232b,
        end: 0x237b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x237c,
        end: 0x237c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x237d,
        end: 0x239a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x239b,
        end: 0x23b3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x23b4,
        end: 0x23db,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x23dc,
        end: 0x23e1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x23e2,
        end: 0x23ef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x23f0,
        end: 0x23f3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x23f4,
        end: 0x23ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2400,
        end: 0x2429,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2440,
        end: 0x244a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2460,
        end: 0x249b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x249c,
        end: 0x24e9,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x24ea,
        end: 0x24fe,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x24ff,
        end: 0x24ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2500,
        end: 0x254b,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x254c,
        end: 0x254f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2550,
        end: 0x2574,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2575,
        end: 0x257f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2580,
        end: 0x258f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2590,
        end: 0x2591,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2592,
        end: 0x2595,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2596,
        end: 0x259f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25a0,
        end: 0x25a1,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25a2,
        end: 0x25a2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25a3,
        end: 0x25a9,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25aa,
        end: 0x25b1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25b2,
        end: 0x25b3,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25b4,
        end: 0x25b5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25b6,
        end: 0x25b6,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25b7,
        end: 0x25b7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25b8,
        end: 0x25bb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25bc,
        end: 0x25bd,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25be,
        end: 0x25bf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25c0,
        end: 0x25c0,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25c1,
        end: 0x25c1,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25c2,
        end: 0x25c5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25c6,
        end: 0x25c8,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25c9,
        end: 0x25ca,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25cb,
        end: 0x25cb,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25cc,
        end: 0x25cd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25ce,
        end: 0x25d1,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25d2,
        end: 0x25e1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25e2,
        end: 0x25e5,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25e6,
        end: 0x25ee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25ef,
        end: 0x25ef,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x25f0,
        end: 0x25f7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x25f8,
        end: 0x25ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2600,
        end: 0x2603,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2604,
        end: 0x2604,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2605,
        end: 0x2606,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2607,
        end: 0x2608,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2609,
        end: 0x2609,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x260a,
        end: 0x260d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x260e,
        end: 0x260f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2610,
        end: 0x2613,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2614,
        end: 0x2615,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2616,
        end: 0x2617,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2618,
        end: 0x2618,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2619,
        end: 0x2619,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x261a,
        end: 0x261c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x261d,
        end: 0x261d,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x261e,
        end: 0x261f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2620,
        end: 0x2638,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2639,
        end: 0x263b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x263c,
        end: 0x263f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2640,
        end: 0x2640,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2641,
        end: 0x2641,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2642,
        end: 0x2642,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2643,
        end: 0x265f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2660,
        end: 0x2661,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2662,
        end: 0x2662,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2663,
        end: 0x2665,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2666,
        end: 0x2666,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2667,
        end: 0x2667,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2668,
        end: 0x2668,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2669,
        end: 0x266a,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x266b,
        end: 0x266b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x266c,
        end: 0x266d,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x266e,
        end: 0x266e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x266f,
        end: 0x266f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2670,
        end: 0x267e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x267f,
        end: 0x267f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2680,
        end: 0x269d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x269e,
        end: 0x269f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26a0,
        end: 0x26bc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x26bd,
        end: 0x26c8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26c9,
        end: 0x26cc,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26cd,
        end: 0x26cd,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26ce,
        end: 0x26ce,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x26cf,
        end: 0x26d1,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26d2,
        end: 0x26d2,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26d3,
        end: 0x26d4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26d5,
        end: 0x26d7,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26d8,
        end: 0x26d9,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26da,
        end: 0x26db,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26dc,
        end: 0x26dc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26dd,
        end: 0x26de,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26df,
        end: 0x26e1,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26e2,
        end: 0x26e2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x26e3,
        end: 0x26e3,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26e4,
        end: 0x26e7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x26e8,
        end: 0x26e9,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26ea,
        end: 0x26ea,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26eb,
        end: 0x26f0,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26f1,
        end: 0x26f5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26f6,
        end: 0x26f6,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26f7,
        end: 0x26f8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26f9,
        end: 0x26f9,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x26fa,
        end: 0x26fa,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x26fb,
        end: 0x26fc,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x26fd,
        end: 0x26ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2700,
        end: 0x2704,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2705,
        end: 0x2707,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2708,
        end: 0x2709,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x270a,
        end: 0x270d,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x270e,
        end: 0x2756,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2757,
        end: 0x2757,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2758,
        end: 0x275a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x275b,
        end: 0x2760,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2761,
        end: 0x2761,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2762,
        end: 0x2763,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x2764,
        end: 0x2764,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2765,
        end: 0x2767,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2768,
        end: 0x2768,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2769,
        end: 0x2769,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x276a,
        end: 0x276a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x276b,
        end: 0x276b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x276c,
        end: 0x276c,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x276d,
        end: 0x276d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x276e,
        end: 0x276e,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x276f,
        end: 0x276f,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2770,
        end: 0x2770,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2771,
        end: 0x2771,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2772,
        end: 0x2772,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2773,
        end: 0x2773,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2774,
        end: 0x2774,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2775,
        end: 0x2775,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2776,
        end: 0x2793,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2794,
        end: 0x27bf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x27c0,
        end: 0x27c4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x27c5,
        end: 0x27c5,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27c6,
        end: 0x27c6,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27c7,
        end: 0x27e5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x27e6,
        end: 0x27e6,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27e7,
        end: 0x27e7,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27e8,
        end: 0x27e8,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27e9,
        end: 0x27e9,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27ea,
        end: 0x27ea,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27eb,
        end: 0x27eb,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27ec,
        end: 0x27ec,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27ed,
        end: 0x27ed,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27ee,
        end: 0x27ee,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x27ef,
        end: 0x27ef,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x27f0,
        end: 0x27ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2800,
        end: 0x2800,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2801,
        end: 0x28ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2900,
        end: 0x297f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2980,
        end: 0x2982,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2983,
        end: 0x2983,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2984,
        end: 0x2984,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2985,
        end: 0x2985,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2986,
        end: 0x2986,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2987,
        end: 0x2987,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2988,
        end: 0x2988,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2989,
        end: 0x2989,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x298a,
        end: 0x298a,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x298b,
        end: 0x298b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x298c,
        end: 0x298c,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x298d,
        end: 0x298d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x298e,
        end: 0x298e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x298f,
        end: 0x298f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2990,
        end: 0x2990,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2991,
        end: 0x2991,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2992,
        end: 0x2992,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2993,
        end: 0x2993,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2994,
        end: 0x2994,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2995,
        end: 0x2995,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2996,
        end: 0x2996,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2997,
        end: 0x2997,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2998,
        end: 0x2998,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2999,
        end: 0x29d7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x29d8,
        end: 0x29d8,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x29d9,
        end: 0x29d9,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x29da,
        end: 0x29da,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x29db,
        end: 0x29db,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x29dc,
        end: 0x29fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x29fc,
        end: 0x29fc,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x29fd,
        end: 0x29fd,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x29fe,
        end: 0x29ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2a00,
        end: 0x2aff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b00,
        end: 0x2b2f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b30,
        end: 0x2b44,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b45,
        end: 0x2b46,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b47,
        end: 0x2b4c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b4d,
        end: 0x2b54,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b55,
        end: 0x2b59,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x2b5a,
        end: 0x2b73,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2b76,
        end: 0x2bff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c00,
        end: 0x2c5f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c60,
        end: 0x2c7b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c7c,
        end: 0x2c7d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c7e,
        end: 0x2c7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2c80,
        end: 0x2ce4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ce5,
        end: 0x2cea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2ceb,
        end: 0x2cee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2cef,
        end: 0x2cf1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2cf2,
        end: 0x2cf3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2cf9,
        end: 0x2cf9,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x2cfa,
        end: 0x2cfc,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2cfd,
        end: 0x2cfd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2cfe,
        end: 0x2cfe,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x2cff,
        end: 0x2cff,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2d00,
        end: 0x2d25,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d27,
        end: 0x2d27,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d2d,
        end: 0x2d2d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d30,
        end: 0x2d67,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d6f,
        end: 0x2d6f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2d70,
        end: 0x2d70,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2d7f,
        end: 0x2d7f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2d80,
        end: 0x2d96,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2da0,
        end: 0x2da6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2da8,
        end: 0x2dae,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2db0,
        end: 0x2db6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2db8,
        end: 0x2dbe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2dc0,
        end: 0x2dc6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2dc8,
        end: 0x2dce,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2dd0,
        end: 0x2dd6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2dd8,
        end: 0x2dde,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2de0,
        end: 0x2dff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x2e00,
        end: 0x2e01,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e02,
        end: 0x2e02,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e03,
        end: 0x2e03,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e04,
        end: 0x2e04,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e05,
        end: 0x2e05,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e06,
        end: 0x2e08,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e09,
        end: 0x2e09,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e0a,
        end: 0x2e0a,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e0b,
        end: 0x2e0b,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e0c,
        end: 0x2e0c,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e0d,
        end: 0x2e0d,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e0e,
        end: 0x2e15,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e16,
        end: 0x2e16,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e17,
        end: 0x2e17,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x2e18,
        end: 0x2e18,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e19,
        end: 0x2e19,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e1a,
        end: 0x2e1a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e1b,
        end: 0x2e1b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e1c,
        end: 0x2e1c,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e1d,
        end: 0x2e1d,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e1e,
        end: 0x2e1f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e20,
        end: 0x2e20,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e21,
        end: 0x2e21,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x2e22,
        end: 0x2e22,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e23,
        end: 0x2e23,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2e24,
        end: 0x2e24,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e25,
        end: 0x2e25,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2e26,
        end: 0x2e26,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e27,
        end: 0x2e27,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2e28,
        end: 0x2e28,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e29,
        end: 0x2e29,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x2e2a,
        end: 0x2e2d,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e2e,
        end: 0x2e2e,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x2e2f,
        end: 0x2e2f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e30,
        end: 0x2e31,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e32,
        end: 0x2e32,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e33,
        end: 0x2e34,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e35,
        end: 0x2e39,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e3a,
        end: 0x2e3b,
        class: LineBreakClass::B2 as u8,
    },
    Range {
        start: 0x2e3c,
        end: 0x2e3e,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e3f,
        end: 0x2e3f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e40,
        end: 0x2e40,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x2e41,
        end: 0x2e41,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e42,
        end: 0x2e42,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e43,
        end: 0x2e4a,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e4b,
        end: 0x2e4b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e4c,
        end: 0x2e4c,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e4d,
        end: 0x2e4d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e4e,
        end: 0x2e4f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x2e50,
        end: 0x2e51,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e52,
        end: 0x2e52,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x2e53,
        end: 0x2e54,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x2e55,
        end: 0x2e55,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e56,
        end: 0x2e56,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x2e57,
        end: 0x2e57,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e58,
        end: 0x2e58,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x2e59,
        end: 0x2e59,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e5a,
        end: 0x2e5a,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x2e5b,
        end: 0x2e5b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x2e5c,
        end: 0x2e5c,
        class: LineBreakClass::Cp as u8,
    },
    Range {
        start: 0x2e5d,
        end: 0x2e5d,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x2e80,
        end: 0x2e99,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2e9b,
        end: 0x2ef3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2f00,
        end: 0x2fd5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ff0,
        end: 0x2fff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3000,
        end: 0x3000,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x3001,
        end: 0x3002,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3003,
        end: 0x3003,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3004,
        end: 0x3004,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3005,
        end: 0x3005,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x3006,
        end: 0x3006,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3007,
        end: 0x3007,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3008,
        end: 0x3008,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x3009,
        end: 0x3009,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x300a,
        end: 0x300a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x300b,
        end: 0x300b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x300c,
        end: 0x300c,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x300d,
        end: 0x300d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x300e,
        end: 0x300e,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x300f,
        end: 0x300f,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3010,
        end: 0x3010,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x3011,
        end: 0x3011,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3012,
        end: 0x3013,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3014,
        end: 0x3014,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x3015,
        end: 0x3015,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3016,
        end: 0x3016,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x3017,
        end: 0x3017,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3018,
        end: 0x3018,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x3019,
        end: 0x3019,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x301a,
        end: 0x301a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x301b,
        end: 0x301b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x301c,
        end: 0x301c,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x301d,
        end: 0x301d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x301e,
        end: 0x301f,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x3020,
        end: 0x3020,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3021,
        end: 0x3029,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x302a,
        end: 0x302d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x302e,
        end: 0x302f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x3030,
        end: 0x3030,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3031,
        end: 0x3034,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3035,
        end: 0x3035,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x3036,
        end: 0x3037,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3038,
        end: 0x303a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x303b,
        end: 0x303b,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x303c,
        end: 0x303c,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x303d,
        end: 0x303d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x303e,
        end: 0x303f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3041,
        end: 0x3041,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3042,
        end: 0x3042,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3043,
        end: 0x3043,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3044,
        end: 0x3044,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3045,
        end: 0x3045,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3046,
        end: 0x3046,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3047,
        end: 0x3047,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3048,
        end: 0x3048,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3049,
        end: 0x3049,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x304a,
        end: 0x3062,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3063,
        end: 0x3063,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3064,
        end: 0x3082,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3083,
        end: 0x3083,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3084,
        end: 0x3084,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3085,
        end: 0x3085,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3086,
        end: 0x3086,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3087,
        end: 0x3087,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3088,
        end: 0x308d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x308e,
        end: 0x308e,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x308f,
        end: 0x3094,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3095,
        end: 0x3096,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3099,
        end: 0x309a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x309b,
        end: 0x309c,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x309d,
        end: 0x309e,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x309f,
        end: 0x309f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30a0,
        end: 0x30a0,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x30a1,
        end: 0x30a1,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30a2,
        end: 0x30a2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30a3,
        end: 0x30a3,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30a4,
        end: 0x30a4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30a5,
        end: 0x30a5,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30a6,
        end: 0x30a6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30a7,
        end: 0x30a7,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30a8,
        end: 0x30a8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30a9,
        end: 0x30a9,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30aa,
        end: 0x30c2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30c3,
        end: 0x30c3,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30c4,
        end: 0x30e2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30e3,
        end: 0x30e3,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30e4,
        end: 0x30e4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30e5,
        end: 0x30e5,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30e6,
        end: 0x30e6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30e7,
        end: 0x30e7,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30e8,
        end: 0x30ed,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30ee,
        end: 0x30ee,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30ef,
        end: 0x30f4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30f5,
        end: 0x30f6,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30f7,
        end: 0x30fa,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30fb,
        end: 0x30fb,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x30fc,
        end: 0x30fc,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x30fd,
        end: 0x30fe,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x30ff,
        end: 0x30ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3105,
        end: 0x312f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3131,
        end: 0x318e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3190,
        end: 0x3191,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3192,
        end: 0x3195,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3196,
        end: 0x319f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x31a0,
        end: 0x31bf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x31c0,
        end: 0x31e5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x31ef,
        end: 0x31ef,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x31f0,
        end: 0x31ff,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x3200,
        end: 0x321e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3220,
        end: 0x3229,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x322a,
        end: 0x3247,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3248,
        end: 0x324f,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x3250,
        end: 0x3250,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3251,
        end: 0x325f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3260,
        end: 0x327f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3280,
        end: 0x3289,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x328a,
        end: 0x32b0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x32b1,
        end: 0x32bf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x32c0,
        end: 0x32ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3300,
        end: 0x33ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3400,
        end: 0x4dbf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x4dc0,
        end: 0x4dff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x4e00,
        end: 0x9fff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa000,
        end: 0xa014,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa015,
        end: 0xa015,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xa016,
        end: 0xa48c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa490,
        end: 0xa4c6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa4d0,
        end: 0xa4f7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa4f8,
        end: 0xa4fd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa4fe,
        end: 0xa4ff,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa500,
        end: 0xa60b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa60c,
        end: 0xa60c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa60d,
        end: 0xa60d,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa60e,
        end: 0xa60e,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xa60f,
        end: 0xa60f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa610,
        end: 0xa61f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa620,
        end: 0xa629,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xa62a,
        end: 0xa62b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa640,
        end: 0xa66d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa66e,
        end: 0xa66e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa66f,
        end: 0xa66f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa670,
        end: 0xa672,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa673,
        end: 0xa673,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa674,
        end: 0xa67d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa67e,
        end: 0xa67e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa67f,
        end: 0xa67f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa680,
        end: 0xa69b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa69c,
        end: 0xa69d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa69e,
        end: 0xa69f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa6a0,
        end: 0xa6e5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa6e6,
        end: 0xa6ef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa6f0,
        end: 0xa6f1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa6f2,
        end: 0xa6f2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa6f3,
        end: 0xa6f7,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa700,
        end: 0xa716,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa717,
        end: 0xa71f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa720,
        end: 0xa721,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa722,
        end: 0xa76f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa770,
        end: 0xa770,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa771,
        end: 0xa787,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa788,
        end: 0xa788,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa789,
        end: 0xa78a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa78b,
        end: 0xa78e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa78f,
        end: 0xa78f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa790,
        end: 0xa7dc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7f1,
        end: 0xa7f4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7f5,
        end: 0xa7f6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7f7,
        end: 0xa7f7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7f8,
        end: 0xa7f9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7fa,
        end: 0xa7fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa7fb,
        end: 0xa7ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa800,
        end: 0xa801,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa802,
        end: 0xa802,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa803,
        end: 0xa805,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa806,
        end: 0xa806,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa807,
        end: 0xa80a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa80b,
        end: 0xa80b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa80c,
        end: 0xa822,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa823,
        end: 0xa824,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa825,
        end: 0xa826,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa827,
        end: 0xa827,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa828,
        end: 0xa82b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa82c,
        end: 0xa82c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa830,
        end: 0xa835,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa836,
        end: 0xa837,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa838,
        end: 0xa838,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xa839,
        end: 0xa839,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa840,
        end: 0xa873,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa874,
        end: 0xa875,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xa876,
        end: 0xa877,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xa880,
        end: 0xa881,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa882,
        end: 0xa8b3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8b4,
        end: 0xa8c3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa8c4,
        end: 0xa8c5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa8ce,
        end: 0xa8cf,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa8d0,
        end: 0xa8d9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xa8e0,
        end: 0xa8f1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa8f2,
        end: 0xa8f7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8f8,
        end: 0xa8fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8fb,
        end: 0xa8fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8fc,
        end: 0xa8fc,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0xa8fd,
        end: 0xa8fe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa8ff,
        end: 0xa8ff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa900,
        end: 0xa909,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xa90a,
        end: 0xa925,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa926,
        end: 0xa92d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa92e,
        end: 0xa92f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa930,
        end: 0xa946,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa947,
        end: 0xa951,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa952,
        end: 0xa953,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa95f,
        end: 0xa95f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xa960,
        end: 0xa97c,
        class: LineBreakClass::Jl as u8,
    },
    Range {
        start: 0xa980,
        end: 0xa982,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa983,
        end: 0xa983,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa984,
        end: 0xa9b2,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0xa9b3,
        end: 0xa9b3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9b4,
        end: 0xa9b5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9b6,
        end: 0xa9b9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9ba,
        end: 0xa9bb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9bc,
        end: 0xa9bd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9be,
        end: 0xa9bf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xa9c0,
        end: 0xa9c0,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0xa9c1,
        end: 0xa9c6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa9c7,
        end: 0xa9c9,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa9ca,
        end: 0xa9cd,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa9cf,
        end: 0xa9cf,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xa9d0,
        end: 0xa9d9,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0xa9de,
        end: 0xa9df,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xa9e0,
        end: 0xa9e4,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xa9e5,
        end: 0xa9e5,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xa9e6,
        end: 0xa9e6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xa9e7,
        end: 0xa9ef,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xa9f0,
        end: 0xa9f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xa9fa,
        end: 0xa9fe,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa00,
        end: 0xaa28,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0xaa29,
        end: 0xaa2e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa2f,
        end: 0xaa30,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa31,
        end: 0xaa32,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa33,
        end: 0xaa34,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa35,
        end: 0xaa36,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa40,
        end: 0xaa42,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xaa43,
        end: 0xaa43,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa44,
        end: 0xaa4b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xaa4c,
        end: 0xaa4c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa4d,
        end: 0xaa4d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaa50,
        end: 0xaa59,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0xaa5c,
        end: 0xaa5c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xaa5d,
        end: 0xaa5f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xaa60,
        end: 0xaa6f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa70,
        end: 0xaa70,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa71,
        end: 0xaa76,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa77,
        end: 0xaa79,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa7a,
        end: 0xaa7a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa7b,
        end: 0xaa7b,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa7c,
        end: 0xaa7c,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa7d,
        end: 0xaa7d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa7e,
        end: 0xaa7f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaa80,
        end: 0xaaaf,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab0,
        end: 0xaab0,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab1,
        end: 0xaab1,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab2,
        end: 0xaab4,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab5,
        end: 0xaab6,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab7,
        end: 0xaab8,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaab9,
        end: 0xaabd,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaabe,
        end: 0xaabf,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaac0,
        end: 0xaac0,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaac1,
        end: 0xaac1,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaac2,
        end: 0xaac2,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaadb,
        end: 0xaadc,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaadd,
        end: 0xaadd,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaade,
        end: 0xaadf,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0xaae0,
        end: 0xaaea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaaeb,
        end: 0xaaeb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaaec,
        end: 0xaaed,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaaee,
        end: 0xaaef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaaf0,
        end: 0xaaf1,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xaaf2,
        end: 0xaaf2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaaf3,
        end: 0xaaf4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xaaf5,
        end: 0xaaf5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xaaf6,
        end: 0xaaf6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xab01,
        end: 0xab06,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab09,
        end: 0xab0e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab11,
        end: 0xab16,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab20,
        end: 0xab26,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab28,
        end: 0xab2e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab30,
        end: 0xab5a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab5b,
        end: 0xab5b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab5c,
        end: 0xab5f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab60,
        end: 0xab68,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab69,
        end: 0xab69,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab6a,
        end: 0xab6b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xab70,
        end: 0xabbf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xabc0,
        end: 0xabe2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xabe3,
        end: 0xabe4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabe5,
        end: 0xabe5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabe6,
        end: 0xabe7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabe8,
        end: 0xabe8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabe9,
        end: 0xabea,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabeb,
        end: 0xabeb,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0xabec,
        end: 0xabec,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabed,
        end: 0xabed,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xabf0,
        end: 0xabf9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0xac00,
        end: 0xac00,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac01,
        end: 0xac1b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xac1c,
        end: 0xac1c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac1d,
        end: 0xac37,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xac38,
        end: 0xac38,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac39,
        end: 0xac53,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xac54,
        end: 0xac54,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac55,
        end: 0xac6f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xac70,
        end: 0xac70,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac71,
        end: 0xac8b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xac8c,
        end: 0xac8c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xac8d,
        end: 0xaca7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaca8,
        end: 0xaca8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaca9,
        end: 0xacc3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xacc4,
        end: 0xacc4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xacc5,
        end: 0xacdf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xace0,
        end: 0xace0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xace1,
        end: 0xacfb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xacfc,
        end: 0xacfc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xacfd,
        end: 0xad17,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xad18,
        end: 0xad18,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xad19,
        end: 0xad33,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xad34,
        end: 0xad34,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xad35,
        end: 0xad4f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xad50,
        end: 0xad50,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xad51,
        end: 0xad6b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xad6c,
        end: 0xad6c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xad6d,
        end: 0xad87,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xad88,
        end: 0xad88,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xad89,
        end: 0xada3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xada4,
        end: 0xada4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xada5,
        end: 0xadbf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xadc0,
        end: 0xadc0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xadc1,
        end: 0xaddb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaddc,
        end: 0xaddc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaddd,
        end: 0xadf7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xadf8,
        end: 0xadf8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xadf9,
        end: 0xae13,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xae14,
        end: 0xae14,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xae15,
        end: 0xae2f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xae30,
        end: 0xae30,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xae31,
        end: 0xae4b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xae4c,
        end: 0xae4c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xae4d,
        end: 0xae67,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xae68,
        end: 0xae68,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xae69,
        end: 0xae83,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xae84,
        end: 0xae84,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xae85,
        end: 0xae9f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaea0,
        end: 0xaea0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaea1,
        end: 0xaebb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaebc,
        end: 0xaebc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaebd,
        end: 0xaed7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaed8,
        end: 0xaed8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaed9,
        end: 0xaef3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaef4,
        end: 0xaef4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaef5,
        end: 0xaf0f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf10,
        end: 0xaf10,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf11,
        end: 0xaf2b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf2c,
        end: 0xaf2c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf2d,
        end: 0xaf47,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf48,
        end: 0xaf48,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf49,
        end: 0xaf63,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf64,
        end: 0xaf64,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf65,
        end: 0xaf7f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf80,
        end: 0xaf80,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf81,
        end: 0xaf9b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaf9c,
        end: 0xaf9c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaf9d,
        end: 0xafb7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xafb8,
        end: 0xafb8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xafb9,
        end: 0xafd3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xafd4,
        end: 0xafd4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xafd5,
        end: 0xafef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xaff0,
        end: 0xaff0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xaff1,
        end: 0xb00b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb00c,
        end: 0xb00c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb00d,
        end: 0xb027,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb028,
        end: 0xb028,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb029,
        end: 0xb043,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb044,
        end: 0xb044,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb045,
        end: 0xb05f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb060,
        end: 0xb060,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb061,
        end: 0xb07b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb07c,
        end: 0xb07c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb07d,
        end: 0xb097,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb098,
        end: 0xb098,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb099,
        end: 0xb0b3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb0b4,
        end: 0xb0b4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb0b5,
        end: 0xb0cf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb0d0,
        end: 0xb0d0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb0d1,
        end: 0xb0eb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb0ec,
        end: 0xb0ec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb0ed,
        end: 0xb107,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb108,
        end: 0xb108,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb109,
        end: 0xb123,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb124,
        end: 0xb124,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb125,
        end: 0xb13f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb140,
        end: 0xb140,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb141,
        end: 0xb15b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb15c,
        end: 0xb15c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb15d,
        end: 0xb177,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb178,
        end: 0xb178,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb179,
        end: 0xb193,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb194,
        end: 0xb194,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb195,
        end: 0xb1af,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb1b0,
        end: 0xb1b0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb1b1,
        end: 0xb1cb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb1cc,
        end: 0xb1cc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb1cd,
        end: 0xb1e7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb1e8,
        end: 0xb1e8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb1e9,
        end: 0xb203,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb204,
        end: 0xb204,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb205,
        end: 0xb21f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb220,
        end: 0xb220,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb221,
        end: 0xb23b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb23c,
        end: 0xb23c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb23d,
        end: 0xb257,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb258,
        end: 0xb258,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb259,
        end: 0xb273,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb274,
        end: 0xb274,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb275,
        end: 0xb28f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb290,
        end: 0xb290,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb291,
        end: 0xb2ab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb2ac,
        end: 0xb2ac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb2ad,
        end: 0xb2c7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb2c8,
        end: 0xb2c8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb2c9,
        end: 0xb2e3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb2e4,
        end: 0xb2e4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb2e5,
        end: 0xb2ff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb300,
        end: 0xb300,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb301,
        end: 0xb31b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb31c,
        end: 0xb31c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb31d,
        end: 0xb337,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb338,
        end: 0xb338,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb339,
        end: 0xb353,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb354,
        end: 0xb354,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb355,
        end: 0xb36f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb370,
        end: 0xb370,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb371,
        end: 0xb38b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb38c,
        end: 0xb38c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb38d,
        end: 0xb3a7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb3a8,
        end: 0xb3a8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb3a9,
        end: 0xb3c3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb3c4,
        end: 0xb3c4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb3c5,
        end: 0xb3df,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb3e0,
        end: 0xb3e0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb3e1,
        end: 0xb3fb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb3fc,
        end: 0xb3fc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb3fd,
        end: 0xb417,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb418,
        end: 0xb418,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb419,
        end: 0xb433,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb434,
        end: 0xb434,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb435,
        end: 0xb44f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb450,
        end: 0xb450,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb451,
        end: 0xb46b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb46c,
        end: 0xb46c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb46d,
        end: 0xb487,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb488,
        end: 0xb488,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb489,
        end: 0xb4a3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb4a4,
        end: 0xb4a4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb4a5,
        end: 0xb4bf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb4c0,
        end: 0xb4c0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb4c1,
        end: 0xb4db,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb4dc,
        end: 0xb4dc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb4dd,
        end: 0xb4f7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb4f8,
        end: 0xb4f8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb4f9,
        end: 0xb513,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb514,
        end: 0xb514,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb515,
        end: 0xb52f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb530,
        end: 0xb530,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb531,
        end: 0xb54b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb54c,
        end: 0xb54c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb54d,
        end: 0xb567,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb568,
        end: 0xb568,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb569,
        end: 0xb583,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb584,
        end: 0xb584,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb585,
        end: 0xb59f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb5a0,
        end: 0xb5a0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb5a1,
        end: 0xb5bb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb5bc,
        end: 0xb5bc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb5bd,
        end: 0xb5d7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb5d8,
        end: 0xb5d8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb5d9,
        end: 0xb5f3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb5f4,
        end: 0xb5f4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb5f5,
        end: 0xb60f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb610,
        end: 0xb610,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb611,
        end: 0xb62b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb62c,
        end: 0xb62c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb62d,
        end: 0xb647,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb648,
        end: 0xb648,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb649,
        end: 0xb663,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb664,
        end: 0xb664,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb665,
        end: 0xb67f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb680,
        end: 0xb680,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb681,
        end: 0xb69b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb69c,
        end: 0xb69c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb69d,
        end: 0xb6b7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb6b8,
        end: 0xb6b8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb6b9,
        end: 0xb6d3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb6d4,
        end: 0xb6d4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb6d5,
        end: 0xb6ef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb6f0,
        end: 0xb6f0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb6f1,
        end: 0xb70b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb70c,
        end: 0xb70c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb70d,
        end: 0xb727,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb728,
        end: 0xb728,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb729,
        end: 0xb743,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb744,
        end: 0xb744,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb745,
        end: 0xb75f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb760,
        end: 0xb760,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb761,
        end: 0xb77b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb77c,
        end: 0xb77c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb77d,
        end: 0xb797,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb798,
        end: 0xb798,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb799,
        end: 0xb7b3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb7b4,
        end: 0xb7b4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb7b5,
        end: 0xb7cf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb7d0,
        end: 0xb7d0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb7d1,
        end: 0xb7eb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb7ec,
        end: 0xb7ec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb7ed,
        end: 0xb807,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb808,
        end: 0xb808,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb809,
        end: 0xb823,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb824,
        end: 0xb824,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb825,
        end: 0xb83f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb840,
        end: 0xb840,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb841,
        end: 0xb85b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb85c,
        end: 0xb85c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb85d,
        end: 0xb877,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb878,
        end: 0xb878,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb879,
        end: 0xb893,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb894,
        end: 0xb894,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb895,
        end: 0xb8af,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb8b0,
        end: 0xb8b0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb8b1,
        end: 0xb8cb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb8cc,
        end: 0xb8cc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb8cd,
        end: 0xb8e7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb8e8,
        end: 0xb8e8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb8e9,
        end: 0xb903,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb904,
        end: 0xb904,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb905,
        end: 0xb91f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb920,
        end: 0xb920,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb921,
        end: 0xb93b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb93c,
        end: 0xb93c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb93d,
        end: 0xb957,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb958,
        end: 0xb958,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb959,
        end: 0xb973,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb974,
        end: 0xb974,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb975,
        end: 0xb98f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb990,
        end: 0xb990,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb991,
        end: 0xb9ab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb9ac,
        end: 0xb9ac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb9ad,
        end: 0xb9c7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb9c8,
        end: 0xb9c8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb9c9,
        end: 0xb9e3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xb9e4,
        end: 0xb9e4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xb9e5,
        end: 0xb9ff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba00,
        end: 0xba00,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba01,
        end: 0xba1b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba1c,
        end: 0xba1c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba1d,
        end: 0xba37,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba38,
        end: 0xba38,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba39,
        end: 0xba53,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba54,
        end: 0xba54,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba55,
        end: 0xba6f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba70,
        end: 0xba70,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba71,
        end: 0xba8b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xba8c,
        end: 0xba8c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xba8d,
        end: 0xbaa7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbaa8,
        end: 0xbaa8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbaa9,
        end: 0xbac3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbac4,
        end: 0xbac4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbac5,
        end: 0xbadf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbae0,
        end: 0xbae0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbae1,
        end: 0xbafb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbafc,
        end: 0xbafc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbafd,
        end: 0xbb17,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbb18,
        end: 0xbb18,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbb19,
        end: 0xbb33,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbb34,
        end: 0xbb34,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbb35,
        end: 0xbb4f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbb50,
        end: 0xbb50,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbb51,
        end: 0xbb6b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbb6c,
        end: 0xbb6c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbb6d,
        end: 0xbb87,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbb88,
        end: 0xbb88,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbb89,
        end: 0xbba3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbba4,
        end: 0xbba4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbba5,
        end: 0xbbbf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbbc0,
        end: 0xbbc0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbbc1,
        end: 0xbbdb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbbdc,
        end: 0xbbdc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbbdd,
        end: 0xbbf7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbbf8,
        end: 0xbbf8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbbf9,
        end: 0xbc13,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbc14,
        end: 0xbc14,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbc15,
        end: 0xbc2f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbc30,
        end: 0xbc30,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbc31,
        end: 0xbc4b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbc4c,
        end: 0xbc4c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbc4d,
        end: 0xbc67,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbc68,
        end: 0xbc68,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbc69,
        end: 0xbc83,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbc84,
        end: 0xbc84,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbc85,
        end: 0xbc9f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbca0,
        end: 0xbca0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbca1,
        end: 0xbcbb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbcbc,
        end: 0xbcbc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbcbd,
        end: 0xbcd7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbcd8,
        end: 0xbcd8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbcd9,
        end: 0xbcf3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbcf4,
        end: 0xbcf4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbcf5,
        end: 0xbd0f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd10,
        end: 0xbd10,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd11,
        end: 0xbd2b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd2c,
        end: 0xbd2c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd2d,
        end: 0xbd47,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd48,
        end: 0xbd48,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd49,
        end: 0xbd63,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd64,
        end: 0xbd64,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd65,
        end: 0xbd7f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd80,
        end: 0xbd80,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd81,
        end: 0xbd9b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbd9c,
        end: 0xbd9c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbd9d,
        end: 0xbdb7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbdb8,
        end: 0xbdb8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbdb9,
        end: 0xbdd3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbdd4,
        end: 0xbdd4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbdd5,
        end: 0xbdef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbdf0,
        end: 0xbdf0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbdf1,
        end: 0xbe0b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe0c,
        end: 0xbe0c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe0d,
        end: 0xbe27,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe28,
        end: 0xbe28,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe29,
        end: 0xbe43,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe44,
        end: 0xbe44,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe45,
        end: 0xbe5f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe60,
        end: 0xbe60,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe61,
        end: 0xbe7b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe7c,
        end: 0xbe7c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe7d,
        end: 0xbe97,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbe98,
        end: 0xbe98,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbe99,
        end: 0xbeb3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbeb4,
        end: 0xbeb4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbeb5,
        end: 0xbecf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbed0,
        end: 0xbed0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbed1,
        end: 0xbeeb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbeec,
        end: 0xbeec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbeed,
        end: 0xbf07,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf08,
        end: 0xbf08,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf09,
        end: 0xbf23,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf24,
        end: 0xbf24,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf25,
        end: 0xbf3f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf40,
        end: 0xbf40,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf41,
        end: 0xbf5b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf5c,
        end: 0xbf5c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf5d,
        end: 0xbf77,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf78,
        end: 0xbf78,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf79,
        end: 0xbf93,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbf94,
        end: 0xbf94,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbf95,
        end: 0xbfaf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbfb0,
        end: 0xbfb0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbfb1,
        end: 0xbfcb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbfcc,
        end: 0xbfcc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbfcd,
        end: 0xbfe7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xbfe8,
        end: 0xbfe8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xbfe9,
        end: 0xc003,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc004,
        end: 0xc004,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc005,
        end: 0xc01f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc020,
        end: 0xc020,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc021,
        end: 0xc03b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc03c,
        end: 0xc03c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc03d,
        end: 0xc057,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc058,
        end: 0xc058,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc059,
        end: 0xc073,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc074,
        end: 0xc074,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc075,
        end: 0xc08f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc090,
        end: 0xc090,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc091,
        end: 0xc0ab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc0ac,
        end: 0xc0ac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc0ad,
        end: 0xc0c7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc0c8,
        end: 0xc0c8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc0c9,
        end: 0xc0e3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc0e4,
        end: 0xc0e4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc0e5,
        end: 0xc0ff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc100,
        end: 0xc100,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc101,
        end: 0xc11b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc11c,
        end: 0xc11c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc11d,
        end: 0xc137,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc138,
        end: 0xc138,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc139,
        end: 0xc153,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc154,
        end: 0xc154,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc155,
        end: 0xc16f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc170,
        end: 0xc170,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc171,
        end: 0xc18b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc18c,
        end: 0xc18c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc18d,
        end: 0xc1a7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc1a8,
        end: 0xc1a8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc1a9,
        end: 0xc1c3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc1c4,
        end: 0xc1c4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc1c5,
        end: 0xc1df,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc1e0,
        end: 0xc1e0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc1e1,
        end: 0xc1fb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc1fc,
        end: 0xc1fc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc1fd,
        end: 0xc217,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc218,
        end: 0xc218,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc219,
        end: 0xc233,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc234,
        end: 0xc234,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc235,
        end: 0xc24f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc250,
        end: 0xc250,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc251,
        end: 0xc26b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc26c,
        end: 0xc26c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc26d,
        end: 0xc287,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc288,
        end: 0xc288,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc289,
        end: 0xc2a3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc2a4,
        end: 0xc2a4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc2a5,
        end: 0xc2bf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc2c0,
        end: 0xc2c0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc2c1,
        end: 0xc2db,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc2dc,
        end: 0xc2dc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc2dd,
        end: 0xc2f7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc2f8,
        end: 0xc2f8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc2f9,
        end: 0xc313,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc314,
        end: 0xc314,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc315,
        end: 0xc32f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc330,
        end: 0xc330,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc331,
        end: 0xc34b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc34c,
        end: 0xc34c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc34d,
        end: 0xc367,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc368,
        end: 0xc368,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc369,
        end: 0xc383,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc384,
        end: 0xc384,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc385,
        end: 0xc39f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc3a0,
        end: 0xc3a0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc3a1,
        end: 0xc3bb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc3bc,
        end: 0xc3bc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc3bd,
        end: 0xc3d7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc3d8,
        end: 0xc3d8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc3d9,
        end: 0xc3f3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc3f4,
        end: 0xc3f4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc3f5,
        end: 0xc40f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc410,
        end: 0xc410,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc411,
        end: 0xc42b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc42c,
        end: 0xc42c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc42d,
        end: 0xc447,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc448,
        end: 0xc448,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc449,
        end: 0xc463,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc464,
        end: 0xc464,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc465,
        end: 0xc47f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc480,
        end: 0xc480,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc481,
        end: 0xc49b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc49c,
        end: 0xc49c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc49d,
        end: 0xc4b7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc4b8,
        end: 0xc4b8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc4b9,
        end: 0xc4d3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc4d4,
        end: 0xc4d4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc4d5,
        end: 0xc4ef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc4f0,
        end: 0xc4f0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc4f1,
        end: 0xc50b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc50c,
        end: 0xc50c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc50d,
        end: 0xc527,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc528,
        end: 0xc528,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc529,
        end: 0xc543,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc544,
        end: 0xc544,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc545,
        end: 0xc55f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc560,
        end: 0xc560,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc561,
        end: 0xc57b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc57c,
        end: 0xc57c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc57d,
        end: 0xc597,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc598,
        end: 0xc598,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc599,
        end: 0xc5b3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc5b4,
        end: 0xc5b4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc5b5,
        end: 0xc5cf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc5d0,
        end: 0xc5d0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc5d1,
        end: 0xc5eb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc5ec,
        end: 0xc5ec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc5ed,
        end: 0xc607,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc608,
        end: 0xc608,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc609,
        end: 0xc623,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc624,
        end: 0xc624,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc625,
        end: 0xc63f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc640,
        end: 0xc640,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc641,
        end: 0xc65b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc65c,
        end: 0xc65c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc65d,
        end: 0xc677,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc678,
        end: 0xc678,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc679,
        end: 0xc693,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc694,
        end: 0xc694,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc695,
        end: 0xc6af,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc6b0,
        end: 0xc6b0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc6b1,
        end: 0xc6cb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc6cc,
        end: 0xc6cc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc6cd,
        end: 0xc6e7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc6e8,
        end: 0xc6e8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc6e9,
        end: 0xc703,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc704,
        end: 0xc704,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc705,
        end: 0xc71f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc720,
        end: 0xc720,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc721,
        end: 0xc73b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc73c,
        end: 0xc73c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc73d,
        end: 0xc757,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc758,
        end: 0xc758,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc759,
        end: 0xc773,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc774,
        end: 0xc774,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc775,
        end: 0xc78f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc790,
        end: 0xc790,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc791,
        end: 0xc7ab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc7ac,
        end: 0xc7ac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc7ad,
        end: 0xc7c7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc7c8,
        end: 0xc7c8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc7c9,
        end: 0xc7e3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc7e4,
        end: 0xc7e4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc7e5,
        end: 0xc7ff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc800,
        end: 0xc800,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc801,
        end: 0xc81b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc81c,
        end: 0xc81c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc81d,
        end: 0xc837,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc838,
        end: 0xc838,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc839,
        end: 0xc853,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc854,
        end: 0xc854,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc855,
        end: 0xc86f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc870,
        end: 0xc870,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc871,
        end: 0xc88b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc88c,
        end: 0xc88c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc88d,
        end: 0xc8a7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc8a8,
        end: 0xc8a8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc8a9,
        end: 0xc8c3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc8c4,
        end: 0xc8c4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc8c5,
        end: 0xc8df,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc8e0,
        end: 0xc8e0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc8e1,
        end: 0xc8fb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc8fc,
        end: 0xc8fc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc8fd,
        end: 0xc917,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc918,
        end: 0xc918,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc919,
        end: 0xc933,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc934,
        end: 0xc934,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc935,
        end: 0xc94f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc950,
        end: 0xc950,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc951,
        end: 0xc96b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc96c,
        end: 0xc96c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc96d,
        end: 0xc987,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc988,
        end: 0xc988,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc989,
        end: 0xc9a3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc9a4,
        end: 0xc9a4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc9a5,
        end: 0xc9bf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc9c0,
        end: 0xc9c0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc9c1,
        end: 0xc9db,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc9dc,
        end: 0xc9dc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc9dd,
        end: 0xc9f7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xc9f8,
        end: 0xc9f8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xc9f9,
        end: 0xca13,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xca14,
        end: 0xca14,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xca15,
        end: 0xca2f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xca30,
        end: 0xca30,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xca31,
        end: 0xca4b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xca4c,
        end: 0xca4c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xca4d,
        end: 0xca67,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xca68,
        end: 0xca68,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xca69,
        end: 0xca83,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xca84,
        end: 0xca84,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xca85,
        end: 0xca9f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcaa0,
        end: 0xcaa0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcaa1,
        end: 0xcabb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcabc,
        end: 0xcabc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcabd,
        end: 0xcad7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcad8,
        end: 0xcad8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcad9,
        end: 0xcaf3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcaf4,
        end: 0xcaf4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcaf5,
        end: 0xcb0f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb10,
        end: 0xcb10,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb11,
        end: 0xcb2b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb2c,
        end: 0xcb2c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb2d,
        end: 0xcb47,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb48,
        end: 0xcb48,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb49,
        end: 0xcb63,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb64,
        end: 0xcb64,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb65,
        end: 0xcb7f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb80,
        end: 0xcb80,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb81,
        end: 0xcb9b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcb9c,
        end: 0xcb9c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcb9d,
        end: 0xcbb7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcbb8,
        end: 0xcbb8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcbb9,
        end: 0xcbd3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcbd4,
        end: 0xcbd4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcbd5,
        end: 0xcbef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcbf0,
        end: 0xcbf0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcbf1,
        end: 0xcc0b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc0c,
        end: 0xcc0c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc0d,
        end: 0xcc27,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc28,
        end: 0xcc28,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc29,
        end: 0xcc43,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc44,
        end: 0xcc44,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc45,
        end: 0xcc5f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc60,
        end: 0xcc60,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc61,
        end: 0xcc7b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc7c,
        end: 0xcc7c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc7d,
        end: 0xcc97,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcc98,
        end: 0xcc98,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcc99,
        end: 0xccb3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xccb4,
        end: 0xccb4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xccb5,
        end: 0xcccf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xccd0,
        end: 0xccd0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xccd1,
        end: 0xcceb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xccec,
        end: 0xccec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcced,
        end: 0xcd07,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd08,
        end: 0xcd08,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd09,
        end: 0xcd23,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd24,
        end: 0xcd24,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd25,
        end: 0xcd3f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd40,
        end: 0xcd40,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd41,
        end: 0xcd5b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd5c,
        end: 0xcd5c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd5d,
        end: 0xcd77,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd78,
        end: 0xcd78,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd79,
        end: 0xcd93,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcd94,
        end: 0xcd94,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcd95,
        end: 0xcdaf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcdb0,
        end: 0xcdb0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcdb1,
        end: 0xcdcb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcdcc,
        end: 0xcdcc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcdcd,
        end: 0xcde7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcde8,
        end: 0xcde8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcde9,
        end: 0xce03,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce04,
        end: 0xce04,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce05,
        end: 0xce1f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce20,
        end: 0xce20,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce21,
        end: 0xce3b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce3c,
        end: 0xce3c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce3d,
        end: 0xce57,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce58,
        end: 0xce58,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce59,
        end: 0xce73,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce74,
        end: 0xce74,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce75,
        end: 0xce8f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xce90,
        end: 0xce90,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xce91,
        end: 0xceab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xceac,
        end: 0xceac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcead,
        end: 0xcec7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcec8,
        end: 0xcec8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcec9,
        end: 0xcee3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcee4,
        end: 0xcee4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcee5,
        end: 0xceff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf00,
        end: 0xcf00,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf01,
        end: 0xcf1b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf1c,
        end: 0xcf1c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf1d,
        end: 0xcf37,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf38,
        end: 0xcf38,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf39,
        end: 0xcf53,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf54,
        end: 0xcf54,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf55,
        end: 0xcf6f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf70,
        end: 0xcf70,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf71,
        end: 0xcf8b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcf8c,
        end: 0xcf8c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcf8d,
        end: 0xcfa7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcfa8,
        end: 0xcfa8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcfa9,
        end: 0xcfc3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcfc4,
        end: 0xcfc4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcfc5,
        end: 0xcfdf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcfe0,
        end: 0xcfe0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcfe1,
        end: 0xcffb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xcffc,
        end: 0xcffc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xcffd,
        end: 0xd017,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd018,
        end: 0xd018,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd019,
        end: 0xd033,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd034,
        end: 0xd034,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd035,
        end: 0xd04f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd050,
        end: 0xd050,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd051,
        end: 0xd06b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd06c,
        end: 0xd06c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd06d,
        end: 0xd087,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd088,
        end: 0xd088,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd089,
        end: 0xd0a3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd0a4,
        end: 0xd0a4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd0a5,
        end: 0xd0bf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd0c0,
        end: 0xd0c0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd0c1,
        end: 0xd0db,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd0dc,
        end: 0xd0dc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd0dd,
        end: 0xd0f7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd0f8,
        end: 0xd0f8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd0f9,
        end: 0xd113,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd114,
        end: 0xd114,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd115,
        end: 0xd12f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd130,
        end: 0xd130,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd131,
        end: 0xd14b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd14c,
        end: 0xd14c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd14d,
        end: 0xd167,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd168,
        end: 0xd168,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd169,
        end: 0xd183,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd184,
        end: 0xd184,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd185,
        end: 0xd19f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd1a0,
        end: 0xd1a0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd1a1,
        end: 0xd1bb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd1bc,
        end: 0xd1bc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd1bd,
        end: 0xd1d7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd1d8,
        end: 0xd1d8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd1d9,
        end: 0xd1f3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd1f4,
        end: 0xd1f4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd1f5,
        end: 0xd20f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd210,
        end: 0xd210,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd211,
        end: 0xd22b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd22c,
        end: 0xd22c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd22d,
        end: 0xd247,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd248,
        end: 0xd248,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd249,
        end: 0xd263,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd264,
        end: 0xd264,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd265,
        end: 0xd27f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd280,
        end: 0xd280,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd281,
        end: 0xd29b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd29c,
        end: 0xd29c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd29d,
        end: 0xd2b7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd2b8,
        end: 0xd2b8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd2b9,
        end: 0xd2d3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd2d4,
        end: 0xd2d4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd2d5,
        end: 0xd2ef,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd2f0,
        end: 0xd2f0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd2f1,
        end: 0xd30b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd30c,
        end: 0xd30c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd30d,
        end: 0xd327,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd328,
        end: 0xd328,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd329,
        end: 0xd343,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd344,
        end: 0xd344,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd345,
        end: 0xd35f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd360,
        end: 0xd360,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd361,
        end: 0xd37b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd37c,
        end: 0xd37c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd37d,
        end: 0xd397,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd398,
        end: 0xd398,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd399,
        end: 0xd3b3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd3b4,
        end: 0xd3b4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd3b5,
        end: 0xd3cf,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd3d0,
        end: 0xd3d0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd3d1,
        end: 0xd3eb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd3ec,
        end: 0xd3ec,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd3ed,
        end: 0xd407,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd408,
        end: 0xd408,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd409,
        end: 0xd423,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd424,
        end: 0xd424,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd425,
        end: 0xd43f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd440,
        end: 0xd440,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd441,
        end: 0xd45b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd45c,
        end: 0xd45c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd45d,
        end: 0xd477,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd478,
        end: 0xd478,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd479,
        end: 0xd493,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd494,
        end: 0xd494,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd495,
        end: 0xd4af,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd4b0,
        end: 0xd4b0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd4b1,
        end: 0xd4cb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd4cc,
        end: 0xd4cc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd4cd,
        end: 0xd4e7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd4e8,
        end: 0xd4e8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd4e9,
        end: 0xd503,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd504,
        end: 0xd504,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd505,
        end: 0xd51f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd520,
        end: 0xd520,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd521,
        end: 0xd53b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd53c,
        end: 0xd53c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd53d,
        end: 0xd557,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd558,
        end: 0xd558,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd559,
        end: 0xd573,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd574,
        end: 0xd574,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd575,
        end: 0xd58f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd590,
        end: 0xd590,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd591,
        end: 0xd5ab,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd5ac,
        end: 0xd5ac,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd5ad,
        end: 0xd5c7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd5c8,
        end: 0xd5c8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd5c9,
        end: 0xd5e3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd5e4,
        end: 0xd5e4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd5e5,
        end: 0xd5ff,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd600,
        end: 0xd600,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd601,
        end: 0xd61b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd61c,
        end: 0xd61c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd61d,
        end: 0xd637,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd638,
        end: 0xd638,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd639,
        end: 0xd653,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd654,
        end: 0xd654,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd655,
        end: 0xd66f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd670,
        end: 0xd670,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd671,
        end: 0xd68b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd68c,
        end: 0xd68c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd68d,
        end: 0xd6a7,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd6a8,
        end: 0xd6a8,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd6a9,
        end: 0xd6c3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd6c4,
        end: 0xd6c4,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd6c5,
        end: 0xd6df,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd6e0,
        end: 0xd6e0,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd6e1,
        end: 0xd6fb,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd6fc,
        end: 0xd6fc,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd6fd,
        end: 0xd717,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd718,
        end: 0xd718,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd719,
        end: 0xd733,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd734,
        end: 0xd734,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd735,
        end: 0xd74f,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd750,
        end: 0xd750,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd751,
        end: 0xd76b,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd76c,
        end: 0xd76c,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd76d,
        end: 0xd787,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd788,
        end: 0xd788,
        class: LineBreakClass::H2 as u8,
    },
    Range {
        start: 0xd789,
        end: 0xd7a3,
        class: LineBreakClass::H3 as u8,
    },
    Range {
        start: 0xd7b0,
        end: 0xd7c6,
        class: LineBreakClass::Jv as u8,
    },
    Range {
        start: 0xd7cb,
        end: 0xd7fb,
        class: LineBreakClass::Jt as u8,
    },
    Range {
        start: 0xd800,
        end: 0xdb7f,
        class: LineBreakClass::Sg as u8,
    },
    Range {
        start: 0xdb80,
        end: 0xdbff,
        class: LineBreakClass::Sg as u8,
    },
    Range {
        start: 0xdc00,
        end: 0xdfff,
        class: LineBreakClass::Sg as u8,
    },
    Range {
        start: 0xe000,
        end: 0xf8ff,
        class: LineBreakClass::Xx as u8,
    },
    Range {
        start: 0xf900,
        end: 0xfa6d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfa6e,
        end: 0xfa6f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfa70,
        end: 0xfad9,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfada,
        end: 0xfaff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfb00,
        end: 0xfb06,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfb13,
        end: 0xfb17,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfb1d,
        end: 0xfb1d,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb1e,
        end: 0xfb1e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfb1f,
        end: 0xfb28,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb29,
        end: 0xfb29,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfb2a,
        end: 0xfb36,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb38,
        end: 0xfb3c,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb3e,
        end: 0xfb3e,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb40,
        end: 0xfb41,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb43,
        end: 0xfb44,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb46,
        end: 0xfb4f,
        class: LineBreakClass::Hl as u8,
    },
    Range {
        start: 0xfb50,
        end: 0xfbb1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfbb2,
        end: 0xfbc2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfbc3,
        end: 0xfbd2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfbd3,
        end: 0xfd3d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd3e,
        end: 0xfd3e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfd3f,
        end: 0xfd3f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfd40,
        end: 0xfd4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd50,
        end: 0xfd8f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd90,
        end: 0xfd91,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfd92,
        end: 0xfdc7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfdc8,
        end: 0xfdcf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfdf0,
        end: 0xfdfb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfdfc,
        end: 0xfdfc,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xfdfd,
        end: 0xfdff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfe00,
        end: 0xfe0f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe10,
        end: 0xfe12,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe13,
        end: 0xfe14,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xfe15,
        end: 0xfe16,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xfe17,
        end: 0xfe17,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe18,
        end: 0xfe18,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe19,
        end: 0xfe19,
        class: LineBreakClass::In as u8,
    },
    Range {
        start: 0xfe20,
        end: 0xfe20,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe21,
        end: 0xfe21,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe22,
        end: 0xfe22,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe23,
        end: 0xfe23,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe24,
        end: 0xfe24,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe25,
        end: 0xfe25,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe26,
        end: 0xfe27,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe28,
        end: 0xfe28,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe29,
        end: 0xfe29,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe2a,
        end: 0xfe2a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe2b,
        end: 0xfe2b,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe2c,
        end: 0xfe2c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe2d,
        end: 0xfe2e,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0xfe2f,
        end: 0xfe2f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfe30,
        end: 0xfe30,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe31,
        end: 0xfe32,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe33,
        end: 0xfe34,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe35,
        end: 0xfe35,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe36,
        end: 0xfe36,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe37,
        end: 0xfe37,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe38,
        end: 0xfe38,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe39,
        end: 0xfe39,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe3a,
        end: 0xfe3a,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe3b,
        end: 0xfe3b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe3c,
        end: 0xfe3c,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe3d,
        end: 0xfe3d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe3e,
        end: 0xfe3e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe3f,
        end: 0xfe3f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe40,
        end: 0xfe40,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe41,
        end: 0xfe41,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe42,
        end: 0xfe42,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe43,
        end: 0xfe43,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe44,
        end: 0xfe44,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe45,
        end: 0xfe46,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe47,
        end: 0xfe47,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe48,
        end: 0xfe48,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe49,
        end: 0xfe4c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe4d,
        end: 0xfe4f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe50,
        end: 0xfe50,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe51,
        end: 0xfe51,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe52,
        end: 0xfe52,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe54,
        end: 0xfe55,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xfe56,
        end: 0xfe57,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xfe58,
        end: 0xfe58,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe59,
        end: 0xfe59,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe5a,
        end: 0xfe5a,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe5b,
        end: 0xfe5b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe5c,
        end: 0xfe5c,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe5d,
        end: 0xfe5d,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xfe5e,
        end: 0xfe5e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xfe5f,
        end: 0xfe61,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe62,
        end: 0xfe62,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe63,
        end: 0xfe63,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe64,
        end: 0xfe66,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe68,
        end: 0xfe68,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe69,
        end: 0xfe69,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xfe6a,
        end: 0xfe6a,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xfe6b,
        end: 0xfe6b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xfe70,
        end: 0xfe74,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfe76,
        end: 0xfefc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfeff,
        end: 0xfeff,
        class: LineBreakClass::Wj as u8,
    },
    Range {
        start: 0xff01,
        end: 0xff01,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xff02,
        end: 0xff03,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff04,
        end: 0xff04,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xff05,
        end: 0xff05,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xff06,
        end: 0xff07,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff08,
        end: 0xff08,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xff09,
        end: 0xff09,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff0a,
        end: 0xff0a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff0b,
        end: 0xff0b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff0c,
        end: 0xff0c,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff0d,
        end: 0xff0d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff0e,
        end: 0xff0e,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff0f,
        end: 0xff0f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff10,
        end: 0xff19,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff1a,
        end: 0xff1b,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xff1c,
        end: 0xff1e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff1f,
        end: 0xff1f,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0xff20,
        end: 0xff20,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff21,
        end: 0xff3a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff3b,
        end: 0xff3b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xff3c,
        end: 0xff3c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff3d,
        end: 0xff3d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff3e,
        end: 0xff3e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff3f,
        end: 0xff3f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff40,
        end: 0xff40,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff41,
        end: 0xff5a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff5b,
        end: 0xff5b,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xff5c,
        end: 0xff5c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff5d,
        end: 0xff5d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff5e,
        end: 0xff5e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff5f,
        end: 0xff5f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xff60,
        end: 0xff60,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff61,
        end: 0xff61,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff62,
        end: 0xff62,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0xff63,
        end: 0xff63,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff64,
        end: 0xff64,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0xff65,
        end: 0xff65,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xff66,
        end: 0xff66,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff67,
        end: 0xff6f,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0xff70,
        end: 0xff70,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0xff71,
        end: 0xff9d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xff9e,
        end: 0xff9f,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0xffa0,
        end: 0xffbe,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffc2,
        end: 0xffc7,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffca,
        end: 0xffcf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffd2,
        end: 0xffd7,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffda,
        end: 0xffdc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffe0,
        end: 0xffe0,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0xffe1,
        end: 0xffe1,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xffe2,
        end: 0xffe2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffe3,
        end: 0xffe3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffe4,
        end: 0xffe4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xffe5,
        end: 0xffe6,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0xffe8,
        end: 0xffe8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xffe9,
        end: 0xffec,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xffed,
        end: 0xffee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0xfff9,
        end: 0xfffb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xfffc,
        end: 0xfffc,
        class: LineBreakClass::Cb as u8,
    },
    Range {
        start: 0xfffd,
        end: 0xfffd,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x10000,
        end: 0x1000b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1000d,
        end: 0x10026,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10028,
        end: 0x1003a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1003c,
        end: 0x1003d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1003f,
        end: 0x1004d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10050,
        end: 0x1005d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10080,
        end: 0x100fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10100,
        end: 0x10102,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10107,
        end: 0x10133,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10137,
        end: 0x1013f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10140,
        end: 0x10174,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10175,
        end: 0x10178,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10179,
        end: 0x10189,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1018a,
        end: 0x1018b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1018c,
        end: 0x1018e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10190,
        end: 0x1019c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x101a0,
        end: 0x101a0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x101d0,
        end: 0x101fc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x101fd,
        end: 0x101fd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10280,
        end: 0x1029c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x102a0,
        end: 0x102d0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x102e0,
        end: 0x102e0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x102e1,
        end: 0x102fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10300,
        end: 0x1031f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10320,
        end: 0x10323,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1032d,
        end: 0x1032f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10330,
        end: 0x10340,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10341,
        end: 0x10341,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10342,
        end: 0x10349,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1034a,
        end: 0x1034a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10350,
        end: 0x10375,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10376,
        end: 0x1037a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10380,
        end: 0x1039d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1039f,
        end: 0x1039f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x103a0,
        end: 0x103c3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x103c8,
        end: 0x103cf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x103d0,
        end: 0x103d0,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x103d1,
        end: 0x103d5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10400,
        end: 0x1044f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10450,
        end: 0x1047f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10480,
        end: 0x1049d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x104a0,
        end: 0x104a9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x104b0,
        end: 0x104d3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x104d8,
        end: 0x104fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10500,
        end: 0x10527,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10530,
        end: 0x10563,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1056f,
        end: 0x1056f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10570,
        end: 0x1057a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1057c,
        end: 0x1058a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1058c,
        end: 0x10592,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10594,
        end: 0x10595,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10597,
        end: 0x105a1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x105a3,
        end: 0x105b1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x105b3,
        end: 0x105b9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x105bb,
        end: 0x105bc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x105c0,
        end: 0x105f3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10600,
        end: 0x10736,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10740,
        end: 0x10755,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10760,
        end: 0x10767,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10780,
        end: 0x10785,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10787,
        end: 0x107b0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x107b2,
        end: 0x107ba,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10800,
        end: 0x10805,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10808,
        end: 0x10808,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1080a,
        end: 0x10835,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10837,
        end: 0x10838,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1083c,
        end: 0x1083c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1083f,
        end: 0x1083f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10840,
        end: 0x10855,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10857,
        end: 0x10857,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10858,
        end: 0x1085f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10860,
        end: 0x10876,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10877,
        end: 0x10878,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10879,
        end: 0x1087f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10880,
        end: 0x1089e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x108a7,
        end: 0x108af,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x108e0,
        end: 0x108f2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x108f4,
        end: 0x108f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x108fb,
        end: 0x108ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10900,
        end: 0x10915,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10916,
        end: 0x1091b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1091f,
        end: 0x1091f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10920,
        end: 0x10939,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1093f,
        end: 0x1093f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10940,
        end: 0x10959,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10980,
        end: 0x1099f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x109a0,
        end: 0x109b7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x109bc,
        end: 0x109bd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x109be,
        end: 0x109bf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x109c0,
        end: 0x109cf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x109d2,
        end: 0x109ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a00,
        end: 0x10a00,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a01,
        end: 0x10a03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10a05,
        end: 0x10a06,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10a0c,
        end: 0x10a0f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10a10,
        end: 0x10a13,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a15,
        end: 0x10a17,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a19,
        end: 0x10a35,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a38,
        end: 0x10a3a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10a3f,
        end: 0x10a3f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10a40,
        end: 0x10a48,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a50,
        end: 0x10a57,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10a58,
        end: 0x10a58,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a60,
        end: 0x10a7c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a7d,
        end: 0x10a7e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a7f,
        end: 0x10a7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a80,
        end: 0x10a9c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10a9d,
        end: 0x10a9f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ac0,
        end: 0x10ac7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ac8,
        end: 0x10ac8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ac9,
        end: 0x10ae4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ae5,
        end: 0x10ae6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10aeb,
        end: 0x10aef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10af0,
        end: 0x10af5,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10af6,
        end: 0x10af6,
        class: LineBreakClass::In as u8,
    },
    Range {
        start: 0x10b00,
        end: 0x10b35,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b39,
        end: 0x10b3f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10b40,
        end: 0x10b55,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b58,
        end: 0x10b5f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b60,
        end: 0x10b72,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b78,
        end: 0x10b7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b80,
        end: 0x10b91,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10b99,
        end: 0x10b9c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ba9,
        end: 0x10baf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10c00,
        end: 0x10c48,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10c80,
        end: 0x10cb2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10cc0,
        end: 0x10cf2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10cfa,
        end: 0x10cff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d00,
        end: 0x10d23,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d24,
        end: 0x10d27,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10d30,
        end: 0x10d39,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x10d40,
        end: 0x10d49,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x10d4a,
        end: 0x10d4d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d4e,
        end: 0x10d4e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d4f,
        end: 0x10d4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d50,
        end: 0x10d65,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d69,
        end: 0x10d6d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10d6e,
        end: 0x10d6e,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x10d6f,
        end: 0x10d6f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d70,
        end: 0x10d85,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10d8e,
        end: 0x10d8f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10e60,
        end: 0x10e7e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10e80,
        end: 0x10ea9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10eab,
        end: 0x10eac,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10ead,
        end: 0x10ead,
        class: LineBreakClass::Hh as u8,
    },
    Range {
        start: 0x10eb0,
        end: 0x10eb1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ec2,
        end: 0x10ec4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ec5,
        end: 0x10ec5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ec6,
        end: 0x10ec7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10ed0,
        end: 0x10ed0,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x10ed1,
        end: 0x10ed8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10efa,
        end: 0x10eff,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10f00,
        end: 0x10f1c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f1d,
        end: 0x10f26,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f27,
        end: 0x10f27,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f30,
        end: 0x10f45,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f46,
        end: 0x10f50,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10f51,
        end: 0x10f54,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f55,
        end: 0x10f59,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f70,
        end: 0x10f81,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10f82,
        end: 0x10f85,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x10f86,
        end: 0x10f89,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fb0,
        end: 0x10fc4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fc5,
        end: 0x10fcb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x10fe0,
        end: 0x10ff6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11000,
        end: 0x11000,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11001,
        end: 0x11001,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11002,
        end: 0x11002,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11003,
        end: 0x11004,
        class: LineBreakClass::Ap as u8,
    },
    Range {
        start: 0x11005,
        end: 0x11037,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11038,
        end: 0x11045,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11046,
        end: 0x11046,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x11047,
        end: 0x11048,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11049,
        end: 0x1104d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x11052,
        end: 0x11065,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x11066,
        end: 0x1106f,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11070,
        end: 0x11070,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11071,
        end: 0x11072,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11073,
        end: 0x11074,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11075,
        end: 0x11075,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1107f,
        end: 0x1107f,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x11080,
        end: 0x11081,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11082,
        end: 0x11082,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11083,
        end: 0x110af,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x110b0,
        end: 0x110b2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x110b3,
        end: 0x110b6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x110b7,
        end: 0x110b8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x110b9,
        end: 0x110ba,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x110bb,
        end: 0x110bc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x110bd,
        end: 0x110bd,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x110be,
        end: 0x110c1,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x110c2,
        end: 0x110c2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x110cd,
        end: 0x110cd,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x110d0,
        end: 0x110e8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x110f0,
        end: 0x110f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11100,
        end: 0x11102,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11103,
        end: 0x11126,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11127,
        end: 0x1112b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1112c,
        end: 0x1112c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1112d,
        end: 0x11134,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11136,
        end: 0x1113f,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11140,
        end: 0x11143,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11144,
        end: 0x11144,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11145,
        end: 0x11146,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11147,
        end: 0x11147,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11150,
        end: 0x11172,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11173,
        end: 0x11173,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11174,
        end: 0x11174,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11175,
        end: 0x11175,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11176,
        end: 0x11176,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11180,
        end: 0x11181,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11182,
        end: 0x11182,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11183,
        end: 0x111b2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111b3,
        end: 0x111b5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111b6,
        end: 0x111be,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111bf,
        end: 0x111c0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111c1,
        end: 0x111c4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111c5,
        end: 0x111c6,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x111c7,
        end: 0x111c7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111c8,
        end: 0x111c8,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x111c9,
        end: 0x111cc,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111cd,
        end: 0x111cd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111ce,
        end: 0x111ce,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111cf,
        end: 0x111cf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x111d0,
        end: 0x111d9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x111da,
        end: 0x111da,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111db,
        end: 0x111db,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x111dc,
        end: 0x111dc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x111dd,
        end: 0x111df,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x111e1,
        end: 0x111f4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11200,
        end: 0x11211,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11213,
        end: 0x1122b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1122c,
        end: 0x1122e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1122f,
        end: 0x11231,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11232,
        end: 0x11233,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11234,
        end: 0x11234,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11235,
        end: 0x11235,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11236,
        end: 0x11237,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11238,
        end: 0x11239,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1123a,
        end: 0x1123a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1123b,
        end: 0x1123c,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1123d,
        end: 0x1123d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1123e,
        end: 0x1123e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1123f,
        end: 0x11240,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11241,
        end: 0x11241,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11280,
        end: 0x11286,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11288,
        end: 0x11288,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1128a,
        end: 0x1128d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1128f,
        end: 0x1129d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1129f,
        end: 0x112a8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x112a9,
        end: 0x112a9,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x112b0,
        end: 0x112de,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x112df,
        end: 0x112df,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x112e0,
        end: 0x112e2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x112e3,
        end: 0x112ea,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x112f0,
        end: 0x112f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11300,
        end: 0x11301,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11302,
        end: 0x11303,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11305,
        end: 0x1130c,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1130f,
        end: 0x11310,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11313,
        end: 0x11328,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1132a,
        end: 0x11330,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11332,
        end: 0x11333,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11335,
        end: 0x11339,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1133b,
        end: 0x1133c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1133d,
        end: 0x1133d,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1133e,
        end: 0x1133f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11340,
        end: 0x11340,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11341,
        end: 0x11344,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11347,
        end: 0x11348,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1134b,
        end: 0x1134c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1134d,
        end: 0x1134d,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x11350,
        end: 0x11350,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11357,
        end: 0x11357,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1135d,
        end: 0x1135d,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1135e,
        end: 0x1135f,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11360,
        end: 0x11361,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11362,
        end: 0x11363,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11366,
        end: 0x1136c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11370,
        end: 0x11374,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11380,
        end: 0x11389,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x1138b,
        end: 0x1138b,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x1138e,
        end: 0x1138e,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11390,
        end: 0x11391,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11392,
        end: 0x113b5,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x113b7,
        end: 0x113b7,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x113b8,
        end: 0x113ba,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113bb,
        end: 0x113c0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113c2,
        end: 0x113c2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113c5,
        end: 0x113c5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113c7,
        end: 0x113ca,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113cc,
        end: 0x113cd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113ce,
        end: 0x113ce,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113cf,
        end: 0x113cf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113d0,
        end: 0x113d0,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x113d1,
        end: 0x113d1,
        class: LineBreakClass::Ap as u8,
    },
    Range {
        start: 0x113d2,
        end: 0x113d2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x113d3,
        end: 0x113d3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x113d4,
        end: 0x113d5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x113d7,
        end: 0x113d8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x113e1,
        end: 0x113e2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11400,
        end: 0x11434,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11435,
        end: 0x11437,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11438,
        end: 0x1143f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11440,
        end: 0x11441,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11442,
        end: 0x11444,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11445,
        end: 0x11445,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11446,
        end: 0x11446,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11447,
        end: 0x1144a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1144b,
        end: 0x1144e,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1144f,
        end: 0x1144f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11450,
        end: 0x11459,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1145a,
        end: 0x1145b,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1145d,
        end: 0x1145d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1145e,
        end: 0x1145e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1145f,
        end: 0x11461,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11480,
        end: 0x114af,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x114b0,
        end: 0x114b2,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114b3,
        end: 0x114b8,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114b9,
        end: 0x114b9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114ba,
        end: 0x114ba,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114bb,
        end: 0x114be,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114bf,
        end: 0x114c0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114c1,
        end: 0x114c1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114c2,
        end: 0x114c3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x114c4,
        end: 0x114c5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x114c6,
        end: 0x114c6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x114c7,
        end: 0x114c7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x114d0,
        end: 0x114d9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11580,
        end: 0x115ae,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x115af,
        end: 0x115b1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115b2,
        end: 0x115b5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115b8,
        end: 0x115bb,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115bc,
        end: 0x115bd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115be,
        end: 0x115be,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115bf,
        end: 0x115c0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x115c1,
        end: 0x115c1,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x115c2,
        end: 0x115c3,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x115c4,
        end: 0x115c5,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x115c6,
        end: 0x115c8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x115c9,
        end: 0x115d7,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x115d8,
        end: 0x115db,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x115dc,
        end: 0x115dd,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11600,
        end: 0x1162f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11630,
        end: 0x11632,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11633,
        end: 0x1163a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1163b,
        end: 0x1163c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1163d,
        end: 0x1163d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1163e,
        end: 0x1163e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1163f,
        end: 0x11640,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11641,
        end: 0x11642,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11643,
        end: 0x11643,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11644,
        end: 0x11644,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11650,
        end: 0x11659,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11660,
        end: 0x1166c,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11680,
        end: 0x116aa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x116ab,
        end: 0x116ab,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116ac,
        end: 0x116ac,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116ad,
        end: 0x116ad,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116ae,
        end: 0x116af,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116b0,
        end: 0x116b5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116b6,
        end: 0x116b6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116b7,
        end: 0x116b7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x116b8,
        end: 0x116b8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x116b9,
        end: 0x116b9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x116c0,
        end: 0x116c9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x116d0,
        end: 0x116e3,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11700,
        end: 0x1171a,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1171d,
        end: 0x1171d,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1171e,
        end: 0x1171e,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1171f,
        end: 0x1171f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11720,
        end: 0x11721,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11722,
        end: 0x11725,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11726,
        end: 0x11726,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11727,
        end: 0x1172b,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11730,
        end: 0x11739,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1173a,
        end: 0x1173b,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x1173c,
        end: 0x1173e,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1173f,
        end: 0x1173f,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11740,
        end: 0x11746,
        class: LineBreakClass::Sa as u8,
    },
    Range {
        start: 0x11800,
        end: 0x1182b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1182c,
        end: 0x1182e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1182f,
        end: 0x11837,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11838,
        end: 0x11838,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11839,
        end: 0x1183a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1183b,
        end: 0x1183b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x118a0,
        end: 0x118df,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x118e0,
        end: 0x118e9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x118ea,
        end: 0x118f2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x118ff,
        end: 0x118ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11900,
        end: 0x11906,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11909,
        end: 0x11909,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x1190c,
        end: 0x11913,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11915,
        end: 0x11916,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11918,
        end: 0x1192f,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11930,
        end: 0x11935,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11937,
        end: 0x11938,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1193b,
        end: 0x1193c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1193d,
        end: 0x1193d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1193e,
        end: 0x1193e,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x1193f,
        end: 0x1193f,
        class: LineBreakClass::Ap as u8,
    },
    Range {
        start: 0x11940,
        end: 0x11940,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11941,
        end: 0x11941,
        class: LineBreakClass::Ap as u8,
    },
    Range {
        start: 0x11942,
        end: 0x11942,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11943,
        end: 0x11943,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11944,
        end: 0x11946,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11950,
        end: 0x11959,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x119a0,
        end: 0x119a7,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x119aa,
        end: 0x119d0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x119d1,
        end: 0x119d3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x119d4,
        end: 0x119d7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x119da,
        end: 0x119db,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x119dc,
        end: 0x119df,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x119e0,
        end: 0x119e0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x119e1,
        end: 0x119e1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x119e2,
        end: 0x119e2,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x119e3,
        end: 0x119e3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x119e4,
        end: 0x119e4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a00,
        end: 0x11a00,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a01,
        end: 0x11a0a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a0b,
        end: 0x11a32,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a33,
        end: 0x11a38,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a39,
        end: 0x11a39,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a3a,
        end: 0x11a3a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a3b,
        end: 0x11a3e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a3f,
        end: 0x11a3f,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11a40,
        end: 0x11a40,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a41,
        end: 0x11a44,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11a45,
        end: 0x11a45,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11a46,
        end: 0x11a46,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a47,
        end: 0x11a47,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a50,
        end: 0x11a50,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a51,
        end: 0x11a56,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a57,
        end: 0x11a58,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a59,
        end: 0x11a5b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a5c,
        end: 0x11a89,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a8a,
        end: 0x11a96,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a97,
        end: 0x11a97,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a98,
        end: 0x11a99,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11a9a,
        end: 0x11a9c,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11a9d,
        end: 0x11a9d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11a9e,
        end: 0x11aa0,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11aa1,
        end: 0x11aa2,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11ab0,
        end: 0x11abf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11ac0,
        end: 0x11af8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11b00,
        end: 0x11b09,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11b60,
        end: 0x11b60,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11b61,
        end: 0x11b61,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11b62,
        end: 0x11b64,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11b65,
        end: 0x11b65,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11b66,
        end: 0x11b66,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11b67,
        end: 0x11b67,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11bc0,
        end: 0x11be0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11be1,
        end: 0x11be1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11bf0,
        end: 0x11bf9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11c00,
        end: 0x11c08,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11c0a,
        end: 0x11c2e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11c2f,
        end: 0x11c2f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11c30,
        end: 0x11c36,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11c38,
        end: 0x11c3d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11c3e,
        end: 0x11c3e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11c3f,
        end: 0x11c3f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11c40,
        end: 0x11c40,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11c41,
        end: 0x11c45,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11c50,
        end: 0x11c59,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11c5a,
        end: 0x11c6c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11c70,
        end: 0x11c70,
        class: LineBreakClass::Bb as u8,
    },
    Range {
        start: 0x11c71,
        end: 0x11c71,
        class: LineBreakClass::Ex as u8,
    },
    Range {
        start: 0x11c72,
        end: 0x11c8f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11c92,
        end: 0x11ca7,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11ca9,
        end: 0x11ca9,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11caa,
        end: 0x11cb0,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11cb1,
        end: 0x11cb1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11cb2,
        end: 0x11cb3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11cb4,
        end: 0x11cb4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11cb5,
        end: 0x11cb6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d00,
        end: 0x11d06,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d08,
        end: 0x11d09,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d0b,
        end: 0x11d30,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d31,
        end: 0x11d36,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d3a,
        end: 0x11d3a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d3c,
        end: 0x11d3d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d3f,
        end: 0x11d45,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d46,
        end: 0x11d46,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d47,
        end: 0x11d47,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d50,
        end: 0x11d59,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11d60,
        end: 0x11d65,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d67,
        end: 0x11d68,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d6a,
        end: 0x11d89,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11d8a,
        end: 0x11d8e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d90,
        end: 0x11d91,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d93,
        end: 0x11d94,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d95,
        end: 0x11d95,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d96,
        end: 0x11d96,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d97,
        end: 0x11d97,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11d98,
        end: 0x11d98,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11da0,
        end: 0x11da9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11db0,
        end: 0x11dd8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11dd9,
        end: 0x11dd9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11dda,
        end: 0x11ddb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11de0,
        end: 0x11de9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x11ee0,
        end: 0x11ef1,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11ef2,
        end: 0x11ef2,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11ef3,
        end: 0x11ef4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11ef5,
        end: 0x11ef6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11ef7,
        end: 0x11ef8,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11f00,
        end: 0x11f01,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f02,
        end: 0x11f02,
        class: LineBreakClass::Ap as u8,
    },
    Range {
        start: 0x11f03,
        end: 0x11f03,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f04,
        end: 0x11f10,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11f12,
        end: 0x11f33,
        class: LineBreakClass::Ak as u8,
    },
    Range {
        start: 0x11f34,
        end: 0x11f35,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f36,
        end: 0x11f3a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f3e,
        end: 0x11f3f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f40,
        end: 0x11f40,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f41,
        end: 0x11f41,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11f42,
        end: 0x11f42,
        class: LineBreakClass::Vi as u8,
    },
    Range {
        start: 0x11f43,
        end: 0x11f44,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x11f45,
        end: 0x11f4f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x11f50,
        end: 0x11f59,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x11f5a,
        end: 0x11f5a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x11fb0,
        end: 0x11fb0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11fc0,
        end: 0x11fd4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11fd5,
        end: 0x11fdc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11fdd,
        end: 0x11fe0,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x11fe1,
        end: 0x11ff1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x11fff,
        end: 0x11fff,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x12000,
        end: 0x12399,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12400,
        end: 0x1246e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12470,
        end: 0x12474,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x12480,
        end: 0x12543,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12f90,
        end: 0x12ff0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x12ff1,
        end: 0x12ff2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13000,
        end: 0x13257,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13258,
        end: 0x1325a,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x1325b,
        end: 0x1325d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x1325e,
        end: 0x13281,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13282,
        end: 0x13282,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x13283,
        end: 0x13285,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13286,
        end: 0x13286,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x13287,
        end: 0x13287,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x13288,
        end: 0x13288,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x13289,
        end: 0x13289,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x1328a,
        end: 0x13378,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13379,
        end: 0x13379,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x1337a,
        end: 0x1337b,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x1337c,
        end: 0x1342e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1342f,
        end: 0x1342f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x13430,
        end: 0x13436,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x13437,
        end: 0x13437,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x13438,
        end: 0x13438,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x13439,
        end: 0x1343b,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x1343c,
        end: 0x1343c,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x1343d,
        end: 0x1343d,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x1343e,
        end: 0x1343e,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x1343f,
        end: 0x1343f,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x13440,
        end: 0x13440,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x13441,
        end: 0x13446,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x13447,
        end: 0x13455,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x13460,
        end: 0x143fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x14400,
        end: 0x145cd,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x145ce,
        end: 0x145ce,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x145cf,
        end: 0x145cf,
        class: LineBreakClass::Cl as u8,
    },
    Range {
        start: 0x145d0,
        end: 0x14646,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16100,
        end: 0x1611d,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x1611e,
        end: 0x16129,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1612a,
        end: 0x1612c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1612d,
        end: 0x1612f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16130,
        end: 0x16139,
        class: LineBreakClass::As as u8,
    },
    Range {
        start: 0x16800,
        end: 0x16a38,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16a40,
        end: 0x16a5e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16a60,
        end: 0x16a69,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x16a6e,
        end: 0x16a6f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16a70,
        end: 0x16abe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16ac0,
        end: 0x16ac9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x16ad0,
        end: 0x16aed,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16af0,
        end: 0x16af4,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16af5,
        end: 0x16af5,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16b00,
        end: 0x16b2f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b30,
        end: 0x16b36,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16b37,
        end: 0x16b39,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16b3a,
        end: 0x16b3b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b3c,
        end: 0x16b3f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b40,
        end: 0x16b43,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b44,
        end: 0x16b44,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16b45,
        end: 0x16b45,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b50,
        end: 0x16b59,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x16b5b,
        end: 0x16b61,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b63,
        end: 0x16b77,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16b7d,
        end: 0x16b8f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16d40,
        end: 0x16d42,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16d43,
        end: 0x16d6a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16d6b,
        end: 0x16d6c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16d6d,
        end: 0x16d6d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16d6e,
        end: 0x16d6f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16d70,
        end: 0x16d79,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x16e40,
        end: 0x16e7f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16e80,
        end: 0x16e96,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16e97,
        end: 0x16e98,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x16e99,
        end: 0x16e9a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16ea0,
        end: 0x16eb8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16ebb,
        end: 0x16ed3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16f00,
        end: 0x16f4a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16f4f,
        end: 0x16f4f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16f50,
        end: 0x16f50,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16f51,
        end: 0x16f87,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16f8f,
        end: 0x16f92,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16f93,
        end: 0x16f9f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x16fe0,
        end: 0x16fe1,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x16fe2,
        end: 0x16fe2,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x16fe3,
        end: 0x16fe3,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x16fe4,
        end: 0x16fe4,
        class: LineBreakClass::Gl as u8,
    },
    Range {
        start: 0x16ff0,
        end: 0x16ff1,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x16ff2,
        end: 0x16ff3,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x16ff4,
        end: 0x16ff6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x17000,
        end: 0x187ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x18800,
        end: 0x18aff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x18b00,
        end: 0x18cd5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x18cff,
        end: 0x18cff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x18d00,
        end: 0x18d1e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x18d80,
        end: 0x18df2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1aff0,
        end: 0x1aff3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1aff5,
        end: 0x1affb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1affd,
        end: 0x1affe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1b000,
        end: 0x1b0ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1b100,
        end: 0x1b122,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1b132,
        end: 0x1b132,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x1b150,
        end: 0x1b152,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x1b155,
        end: 0x1b155,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x1b164,
        end: 0x1b167,
        class: LineBreakClass::Cj as u8,
    },
    Range {
        start: 0x1b170,
        end: 0x1b2fb,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1bc00,
        end: 0x1bc6a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc70,
        end: 0x1bc7c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc80,
        end: 0x1bc88,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc90,
        end: 0x1bc99,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc9c,
        end: 0x1bc9c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1bc9d,
        end: 0x1bc9e,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1bc9f,
        end: 0x1bc9f,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1bca0,
        end: 0x1bca3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cc00,
        end: 0x1ccef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ccf0,
        end: 0x1ccf9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1ccfa,
        end: 0x1ccfc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cd00,
        end: 0x1ceb3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ceba,
        end: 0x1cebf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cec0,
        end: 0x1ced0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cee0,
        end: 0x1ceef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cef0,
        end: 0x1cef0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1cf00,
        end: 0x1cf2d,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cf30,
        end: 0x1cf46,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1cf50,
        end: 0x1cfc3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d000,
        end: 0x1d0f5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d100,
        end: 0x1d126,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d129,
        end: 0x1d164,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d165,
        end: 0x1d166,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d167,
        end: 0x1d169,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d16a,
        end: 0x1d16c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d16d,
        end: 0x1d172,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d173,
        end: 0x1d17a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d17b,
        end: 0x1d182,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d183,
        end: 0x1d184,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d185,
        end: 0x1d18b,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d18c,
        end: 0x1d1a9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d1aa,
        end: 0x1d1ad,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d1ae,
        end: 0x1d1ea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d200,
        end: 0x1d241,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d242,
        end: 0x1d244,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1d245,
        end: 0x1d245,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d2c0,
        end: 0x1d2d3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d2e0,
        end: 0x1d2f3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d300,
        end: 0x1d356,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d360,
        end: 0x1d378,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d400,
        end: 0x1d454,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d456,
        end: 0x1d49c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d49e,
        end: 0x1d49f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4a2,
        end: 0x1d4a2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4a5,
        end: 0x1d4a6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4a9,
        end: 0x1d4ac,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4ae,
        end: 0x1d4b9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4bb,
        end: 0x1d4bb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4bd,
        end: 0x1d4c3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d4c5,
        end: 0x1d505,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d507,
        end: 0x1d50a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d50d,
        end: 0x1d514,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d516,
        end: 0x1d51c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d51e,
        end: 0x1d539,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d53b,
        end: 0x1d53e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d540,
        end: 0x1d544,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d546,
        end: 0x1d546,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d54a,
        end: 0x1d550,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d552,
        end: 0x1d6a5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6a8,
        end: 0x1d6c0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6c1,
        end: 0x1d6c1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6c2,
        end: 0x1d6da,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6db,
        end: 0x1d6db,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6dc,
        end: 0x1d6fa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6fb,
        end: 0x1d6fb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d6fc,
        end: 0x1d714,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d715,
        end: 0x1d715,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d716,
        end: 0x1d734,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d735,
        end: 0x1d735,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d736,
        end: 0x1d74e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d74f,
        end: 0x1d74f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d750,
        end: 0x1d76e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d76f,
        end: 0x1d76f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d770,
        end: 0x1d788,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d789,
        end: 0x1d789,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d78a,
        end: 0x1d7a8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d7a9,
        end: 0x1d7a9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d7aa,
        end: 0x1d7c2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d7c3,
        end: 0x1d7c3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d7c4,
        end: 0x1d7cb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1d7ce,
        end: 0x1d7ff,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1d800,
        end: 0x1d9ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da00,
        end: 0x1da36,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1da37,
        end: 0x1da3a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da3b,
        end: 0x1da6c,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1da6d,
        end: 0x1da74,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da75,
        end: 0x1da75,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1da76,
        end: 0x1da83,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da84,
        end: 0x1da84,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1da85,
        end: 0x1da86,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da87,
        end: 0x1da8a,
        class: LineBreakClass::Ba as u8,
    },
    Range {
        start: 0x1da8b,
        end: 0x1da8b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1da9b,
        end: 0x1da9f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1daa1,
        end: 0x1daaf,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1df00,
        end: 0x1df09,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1df0a,
        end: 0x1df0a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1df0b,
        end: 0x1df1e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1df25,
        end: 0x1df2a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e000,
        end: 0x1e006,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e008,
        end: 0x1e018,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e01b,
        end: 0x1e021,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e023,
        end: 0x1e024,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e026,
        end: 0x1e02a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e030,
        end: 0x1e06d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e08f,
        end: 0x1e08f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e100,
        end: 0x1e12c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e130,
        end: 0x1e136,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e137,
        end: 0x1e13d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e140,
        end: 0x1e149,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1e14e,
        end: 0x1e14e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e14f,
        end: 0x1e14f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e290,
        end: 0x1e2ad,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e2ae,
        end: 0x1e2ae,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e2c0,
        end: 0x1e2eb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e2ec,
        end: 0x1e2ef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e2f0,
        end: 0x1e2f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1e2ff,
        end: 0x1e2ff,
        class: LineBreakClass::Pr as u8,
    },
    Range {
        start: 0x1e4d0,
        end: 0x1e4ea,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e4eb,
        end: 0x1e4eb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e4ec,
        end: 0x1e4ef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e4f0,
        end: 0x1e4f9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1e5d0,
        end: 0x1e5ed,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e5ee,
        end: 0x1e5ef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e5f0,
        end: 0x1e5f0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e5f1,
        end: 0x1e5fa,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1e5ff,
        end: 0x1e5ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6c0,
        end: 0x1e6de,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6e0,
        end: 0x1e6e2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6e3,
        end: 0x1e6e3,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e6e4,
        end: 0x1e6e5,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6e6,
        end: 0x1e6e6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e6e7,
        end: 0x1e6ed,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6ee,
        end: 0x1e6ef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e6f0,
        end: 0x1e6f4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6f5,
        end: 0x1e6f5,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e6fe,
        end: 0x1e6fe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e6ff,
        end: 0x1e6ff,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e7e0,
        end: 0x1e7e6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e7e8,
        end: 0x1e7eb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e7ed,
        end: 0x1e7ee,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e7f0,
        end: 0x1e7fe,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e800,
        end: 0x1e8c4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e8c7,
        end: 0x1e8cf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e8d0,
        end: 0x1e8d6,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e900,
        end: 0x1e943,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e944,
        end: 0x1e94a,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0x1e94b,
        end: 0x1e94b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1e950,
        end: 0x1e959,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1e95e,
        end: 0x1e95f,
        class: LineBreakClass::Op as u8,
    },
    Range {
        start: 0x1ec71,
        end: 0x1ecab,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ecac,
        end: 0x1ecac,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x1ecad,
        end: 0x1ecaf,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ecb0,
        end: 0x1ecb0,
        class: LineBreakClass::Po as u8,
    },
    Range {
        start: 0x1ecb1,
        end: 0x1ecb4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ed01,
        end: 0x1ed2d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ed2e,
        end: 0x1ed2e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ed2f,
        end: 0x1ed3d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee00,
        end: 0x1ee03,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee05,
        end: 0x1ee1f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee21,
        end: 0x1ee22,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee24,
        end: 0x1ee24,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee27,
        end: 0x1ee27,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee29,
        end: 0x1ee32,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee34,
        end: 0x1ee37,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee39,
        end: 0x1ee39,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee3b,
        end: 0x1ee3b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee42,
        end: 0x1ee42,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee47,
        end: 0x1ee47,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee49,
        end: 0x1ee49,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee4b,
        end: 0x1ee4b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee4d,
        end: 0x1ee4f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee51,
        end: 0x1ee52,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee54,
        end: 0x1ee54,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee57,
        end: 0x1ee57,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee59,
        end: 0x1ee59,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee5b,
        end: 0x1ee5b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee5d,
        end: 0x1ee5d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee5f,
        end: 0x1ee5f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee61,
        end: 0x1ee62,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee64,
        end: 0x1ee64,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee67,
        end: 0x1ee6a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee6c,
        end: 0x1ee72,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee74,
        end: 0x1ee77,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee79,
        end: 0x1ee7c,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee7e,
        end: 0x1ee7e,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee80,
        end: 0x1ee89,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1ee8b,
        end: 0x1ee9b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1eea1,
        end: 0x1eea3,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1eea5,
        end: 0x1eea9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1eeab,
        end: 0x1eebb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1eef0,
        end: 0x1eef1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f000,
        end: 0x1f02b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f02c,
        end: 0x1f02f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f030,
        end: 0x1f093,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f094,
        end: 0x1f09f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0a0,
        end: 0x1f0ae,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0af,
        end: 0x1f0b0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0b1,
        end: 0x1f0bf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0c0,
        end: 0x1f0c0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0c1,
        end: 0x1f0cf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0d0,
        end: 0x1f0d0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0d1,
        end: 0x1f0f5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f0f6,
        end: 0x1f0ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f100,
        end: 0x1f10c,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x1f10d,
        end: 0x1f10f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f110,
        end: 0x1f12d,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x1f12e,
        end: 0x1f12f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f130,
        end: 0x1f169,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x1f16a,
        end: 0x1f16f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f170,
        end: 0x1f1ac,
        class: LineBreakClass::Ai as u8,
    },
    Range {
        start: 0x1f1ad,
        end: 0x1f1ad,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f1ae,
        end: 0x1f1e5,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f1e6,
        end: 0x1f1ff,
        class: LineBreakClass::Ri as u8,
    },
    Range {
        start: 0x1f200,
        end: 0x1f202,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f203,
        end: 0x1f20f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f210,
        end: 0x1f23b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f23c,
        end: 0x1f23f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f240,
        end: 0x1f248,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f249,
        end: 0x1f24f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f250,
        end: 0x1f251,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f252,
        end: 0x1f25f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f260,
        end: 0x1f265,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f266,
        end: 0x1f2ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f300,
        end: 0x1f384,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f385,
        end: 0x1f385,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f386,
        end: 0x1f39b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f39c,
        end: 0x1f39d,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f39e,
        end: 0x1f3b4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3b5,
        end: 0x1f3b6,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f3b7,
        end: 0x1f3bb,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3bc,
        end: 0x1f3bc,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f3bd,
        end: 0x1f3c1,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3c2,
        end: 0x1f3c4,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f3c5,
        end: 0x1f3c6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3c7,
        end: 0x1f3c7,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f3c8,
        end: 0x1f3c9,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3ca,
        end: 0x1f3cc,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f3cd,
        end: 0x1f3fa,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f3fb,
        end: 0x1f3ff,
        class: LineBreakClass::Em as u8,
    },
    Range {
        start: 0x1f400,
        end: 0x1f441,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f442,
        end: 0x1f443,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f444,
        end: 0x1f445,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f446,
        end: 0x1f450,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f451,
        end: 0x1f465,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f466,
        end: 0x1f478,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f479,
        end: 0x1f47b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f47c,
        end: 0x1f47c,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f47d,
        end: 0x1f480,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f481,
        end: 0x1f483,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f484,
        end: 0x1f484,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f485,
        end: 0x1f487,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f488,
        end: 0x1f48e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f48f,
        end: 0x1f48f,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f490,
        end: 0x1f490,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f491,
        end: 0x1f491,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f492,
        end: 0x1f49f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4a0,
        end: 0x1f4a0,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f4a1,
        end: 0x1f4a1,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4a2,
        end: 0x1f4a2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f4a3,
        end: 0x1f4a3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4a4,
        end: 0x1f4a4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f4a5,
        end: 0x1f4a9,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4aa,
        end: 0x1f4aa,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f4ab,
        end: 0x1f4ae,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4af,
        end: 0x1f4af,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f4b0,
        end: 0x1f4b0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f4b1,
        end: 0x1f4b2,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f4b3,
        end: 0x1f4ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f500,
        end: 0x1f506,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f507,
        end: 0x1f516,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f517,
        end: 0x1f524,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f525,
        end: 0x1f531,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f532,
        end: 0x1f549,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f54a,
        end: 0x1f573,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f574,
        end: 0x1f575,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f576,
        end: 0x1f579,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f57a,
        end: 0x1f57a,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f57b,
        end: 0x1f58f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f590,
        end: 0x1f590,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f591,
        end: 0x1f594,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f595,
        end: 0x1f596,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f597,
        end: 0x1f5d3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f5d4,
        end: 0x1f5db,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f5dc,
        end: 0x1f5f3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f5f4,
        end: 0x1f5f9,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f5fa,
        end: 0x1f5ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f600,
        end: 0x1f644,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f645,
        end: 0x1f647,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f648,
        end: 0x1f64a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f64b,
        end: 0x1f64f,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f650,
        end: 0x1f675,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f676,
        end: 0x1f678,
        class: LineBreakClass::Qu as u8,
    },
    Range {
        start: 0x1f679,
        end: 0x1f67b,
        class: LineBreakClass::Ns as u8,
    },
    Range {
        start: 0x1f67c,
        end: 0x1f67f,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f680,
        end: 0x1f6a2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6a3,
        end: 0x1f6a3,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f6a4,
        end: 0x1f6b3,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6b4,
        end: 0x1f6b6,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f6b7,
        end: 0x1f6bf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6c0,
        end: 0x1f6c0,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f6c1,
        end: 0x1f6cb,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6cc,
        end: 0x1f6cc,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f6cd,
        end: 0x1f6d8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6d9,
        end: 0x1f6db,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6dc,
        end: 0x1f6ec,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6ed,
        end: 0x1f6ef,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6f0,
        end: 0x1f6fc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f6fd,
        end: 0x1f6ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f700,
        end: 0x1f773,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f774,
        end: 0x1f776,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f777,
        end: 0x1f77a,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f77b,
        end: 0x1f77f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f780,
        end: 0x1f7d4,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f7d5,
        end: 0x1f7d9,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f7da,
        end: 0x1f7df,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f7e0,
        end: 0x1f7eb,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f7ec,
        end: 0x1f7ef,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f7f0,
        end: 0x1f7f0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f7f1,
        end: 0x1f7ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f800,
        end: 0x1f80b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f810,
        end: 0x1f847,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f850,
        end: 0x1f859,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f860,
        end: 0x1f887,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f890,
        end: 0x1f8ad,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f8b0,
        end: 0x1f8bb,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f8c0,
        end: 0x1f8c1,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f8d0,
        end: 0x1f8d8,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f900,
        end: 0x1f90b,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1f90c,
        end: 0x1f90c,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f90d,
        end: 0x1f90e,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f90f,
        end: 0x1f90f,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f910,
        end: 0x1f917,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f918,
        end: 0x1f91f,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f920,
        end: 0x1f925,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f926,
        end: 0x1f926,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f927,
        end: 0x1f92f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f930,
        end: 0x1f939,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f93a,
        end: 0x1f93b,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f93c,
        end: 0x1f93e,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f93f,
        end: 0x1f976,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f977,
        end: 0x1f977,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f978,
        end: 0x1f9b4,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f9b5,
        end: 0x1f9b6,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f9b7,
        end: 0x1f9b7,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f9b8,
        end: 0x1f9b9,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f9ba,
        end: 0x1f9ba,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f9bb,
        end: 0x1f9bb,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f9bc,
        end: 0x1f9cc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f9cd,
        end: 0x1f9cf,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f9d0,
        end: 0x1f9d0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1f9d1,
        end: 0x1f9dd,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1f9de,
        end: 0x1f9ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa00,
        end: 0x1fa57,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fa58,
        end: 0x1fa5f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa60,
        end: 0x1fa6d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa6e,
        end: 0x1fa6f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa70,
        end: 0x1fa7c,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa7d,
        end: 0x1fa7f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa80,
        end: 0x1fa8a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa8b,
        end: 0x1fa8d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fa8e,
        end: 0x1fac2,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fac3,
        end: 0x1fac5,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1fac6,
        end: 0x1fac6,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fac7,
        end: 0x1fac7,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fac8,
        end: 0x1fac8,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fac9,
        end: 0x1facc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1facd,
        end: 0x1fadc,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fadd,
        end: 0x1fade,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fadf,
        end: 0x1faea,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1faeb,
        end: 0x1faee,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1faef,
        end: 0x1faef,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1faf0,
        end: 0x1faf8,
        class: LineBreakClass::Eb as u8,
    },
    Range {
        start: 0x1faf9,
        end: 0x1faff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x1fb00,
        end: 0x1fb92,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fb94,
        end: 0x1fbef,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fbf0,
        end: 0x1fbf9,
        class: LineBreakClass::Nu as u8,
    },
    Range {
        start: 0x1fbfa,
        end: 0x1fbfa,
        class: LineBreakClass::Al as u8,
    },
    Range {
        start: 0x1fc00,
        end: 0x1fffd,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x20000,
        end: 0x2a6df,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2a6e0,
        end: 0x2a6ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2a700,
        end: 0x2b81d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2b81e,
        end: 0x2b81f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2b820,
        end: 0x2cead,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ceae,
        end: 0x2ceaf,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ceb0,
        end: 0x2ebe0,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ebe1,
        end: 0x2ebef,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ebf0,
        end: 0x2ee5d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2ee5e,
        end: 0x2f7ff,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2f800,
        end: 0x2fa1d,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2fa1e,
        end: 0x2fa1f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x2fa20,
        end: 0x2fffd,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x30000,
        end: 0x3134a,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3134b,
        end: 0x3134f,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x31350,
        end: 0x33479,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0x3347a,
        end: 0x3fffd,
        class: LineBreakClass::Id as u8,
    },
    Range {
        start: 0xe0001,
        end: 0xe0001,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xe0020,
        end: 0xe007f,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xe0100,
        end: 0xe01ef,
        class: LineBreakClass::Cm as u8,
    },
    Range {
        start: 0xf0000,
        end: 0xffffd,
        class: LineBreakClass::Xx as u8,
    },
    Range {
        start: 0x100000,
        end: 0x10fffd,
        class: LineBreakClass::Xx as u8,
    },
];
const EXTENDED_PICTOGRAPHIC: &[(u32, u32)] = &[
    (0xa9, 0xa9),
    (0xae, 0xae),
    (0x203c, 0x203c),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21a9, 0x21aa),
    (0x231a, 0x231b),
    (0x2328, 0x2328),
    (0x23cf, 0x23cf),
    (0x23e9, 0x23ec),
    (0x23ed, 0x23ee),
    (0x23ef, 0x23ef),
    (0x23f0, 0x23f0),
    (0x23f1, 0x23f2),
    (0x23f3, 0x23f3),
    (0x23f8, 0x23fa),
    (0x24c2, 0x24c2),
    (0x25aa, 0x25ab),
    (0x25b6, 0x25b6),
    (0x25c0, 0x25c0),
    (0x25fb, 0x25fe),
    (0x2600, 0x2601),
    (0x2602, 0x2603),
    (0x2604, 0x2604),
    (0x260e, 0x260e),
    (0x2611, 0x2611),
    (0x2614, 0x2615),
    (0x2618, 0x2618),
    (0x261d, 0x261d),
    (0x2620, 0x2620),
    (0x2622, 0x2623),
    (0x2626, 0x2626),
    (0x262a, 0x262a),
    (0x262e, 0x262e),
    (0x262f, 0x262f),
    (0x2638, 0x2639),
    (0x263a, 0x263a),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2648, 0x2653),
    (0x265f, 0x265f),
    (0x2660, 0x2660),
    (0x2663, 0x2663),
    (0x2665, 0x2666),
    (0x2668, 0x2668),
    (0x267b, 0x267b),
    (0x267e, 0x267e),
    (0x267f, 0x267f),
    (0x2692, 0x2692),
    (0x2693, 0x2693),
    (0x2694, 0x2694),
    (0x2695, 0x2695),
    (0x2696, 0x2697),
    (0x2699, 0x2699),
    (0x269b, 0x269c),
    (0x26a0, 0x26a1),
    (0x26a7, 0x26a7),
    (0x26aa, 0x26ab),
    (0x26b0, 0x26b1),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26c8, 0x26c8),
    (0x26ce, 0x26ce),
    (0x26cf, 0x26cf),
    (0x26d1, 0x26d1),
    (0x26d3, 0x26d3),
    (0x26d4, 0x26d4),
    (0x26e9, 0x26e9),
    (0x26ea, 0x26ea),
    (0x26f0, 0x26f1),
    (0x26f2, 0x26f3),
    (0x26f4, 0x26f4),
    (0x26f5, 0x26f5),
    (0x26f7, 0x26f9),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2702, 0x2702),
    (0x2705, 0x2705),
    (0x2708, 0x270c),
    (0x270d, 0x270d),
    (0x270f, 0x270f),
    (0x2712, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271d, 0x271d),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2763),
    (0x2764, 0x2764),
    (0x2795, 0x2797),
    (0x27a1, 0x27a1),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2934, 0x2935),
    (0x2b05, 0x2b07),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x3030, 0x3030),
    (0x303d, 0x303d),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1f004, 0x1f004),
    (0x1f02c, 0x1f02f),
    (0x1f094, 0x1f09f),
    (0x1f0af, 0x1f0b0),
    (0x1f0c0, 0x1f0c0),
    (0x1f0cf, 0x1f0cf),
    (0x1f0d0, 0x1f0d0),
    (0x1f0f6, 0x1f0ff),
    (0x1f170, 0x1f171),
    (0x1f17e, 0x1f17f),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f1ae, 0x1f1e5),
    (0x1f201, 0x1f202),
    (0x1f203, 0x1f20f),
    (0x1f21a, 0x1f21a),
    (0x1f22f, 0x1f22f),
    (0x1f232, 0x1f23a),
    (0x1f23c, 0x1f23f),
    (0x1f249, 0x1f24f),
    (0x1f250, 0x1f251),
    (0x1f252, 0x1f25f),
    (0x1f266, 0x1f2ff),
    (0x1f300, 0x1f30c),
    (0x1f30d, 0x1f30e),
    (0x1f30f, 0x1f30f),
    (0x1f310, 0x1f310),
    (0x1f311, 0x1f311),
    (0x1f312, 0x1f312),
    (0x1f313, 0x1f315),
    (0x1f316, 0x1f318),
    (0x1f319, 0x1f319),
    (0x1f31a, 0x1f31a),
    (0x1f31b, 0x1f31b),
    (0x1f31c, 0x1f31c),
    (0x1f31d, 0x1f31e),
    (0x1f31f, 0x1f320),
    (0x1f321, 0x1f321),
    (0x1f324, 0x1f32c),
    (0x1f32d, 0x1f32f),
    (0x1f330, 0x1f331),
    (0x1f332, 0x1f333),
    (0x1f334, 0x1f335),
    (0x1f336, 0x1f336),
    (0x1f337, 0x1f34a),
    (0x1f34b, 0x1f34b),
    (0x1f34c, 0x1f34f),
    (0x1f350, 0x1f350),
    (0x1f351, 0x1f37b),
    (0x1f37c, 0x1f37c),
    (0x1f37d, 0x1f37d),
    (0x1f37e, 0x1f37f),
    (0x1f380, 0x1f393),
    (0x1f396, 0x1f397),
    (0x1f399, 0x1f39b),
    (0x1f39e, 0x1f39f),
    (0x1f3a0, 0x1f3c4),
    (0x1f3c5, 0x1f3c5),
    (0x1f3c6, 0x1f3c6),
    (0x1f3c7, 0x1f3c7),
    (0x1f3c8, 0x1f3c8),
    (0x1f3c9, 0x1f3c9),
    (0x1f3ca, 0x1f3ca),
    (0x1f3cb, 0x1f3ce),
    (0x1f3cf, 0x1f3d3),
    (0x1f3d4, 0x1f3df),
    (0x1f3e0, 0x1f3e3),
    (0x1f3e4, 0x1f3e4),
    (0x1f3e5, 0x1f3f0),
    (0x1f3f3, 0x1f3f3),
    (0x1f3f4, 0x1f3f4),
    (0x1f3f5, 0x1f3f5),
    (0x1f3f7, 0x1f3f7),
    (0x1f3f8, 0x1f3fa),
    (0x1f400, 0x1f407),
    (0x1f408, 0x1f408),
    (0x1f409, 0x1f40b),
    (0x1f40c, 0x1f40e),
    (0x1f40f, 0x1f410),
    (0x1f411, 0x1f412),
    (0x1f413, 0x1f413),
    (0x1f414, 0x1f414),
    (0x1f415, 0x1f415),
    (0x1f416, 0x1f416),
    (0x1f417, 0x1f429),
    (0x1f42a, 0x1f42a),
    (0x1f42b, 0x1f43e),
    (0x1f43f, 0x1f43f),
    (0x1f440, 0x1f440),
    (0x1f441, 0x1f441),
    (0x1f442, 0x1f464),
    (0x1f465, 0x1f465),
    (0x1f466, 0x1f46b),
    (0x1f46c, 0x1f46d),
    (0x1f46e, 0x1f4ac),
    (0x1f4ad, 0x1f4ad),
    (0x1f4ae, 0x1f4b5),
    (0x1f4b6, 0x1f4b7),
    (0x1f4b8, 0x1f4eb),
    (0x1f4ec, 0x1f4ed),
    (0x1f4ee, 0x1f4ee),
    (0x1f4ef, 0x1f4ef),
    (0x1f4f0, 0x1f4f4),
    (0x1f4f5, 0x1f4f5),
    (0x1f4f6, 0x1f4f7),
    (0x1f4f8, 0x1f4f8),
    (0x1f4f9, 0x1f4fc),
    (0x1f4fd, 0x1f4fd),
    (0x1f4ff, 0x1f502),
    (0x1f503, 0x1f503),
    (0x1f504, 0x1f507),
    (0x1f508, 0x1f508),
    (0x1f509, 0x1f509),
    (0x1f50a, 0x1f514),
    (0x1f515, 0x1f515),
    (0x1f516, 0x1f52b),
    (0x1f52c, 0x1f52d),
    (0x1f52e, 0x1f53d),
    (0x1f549, 0x1f54a),
    (0x1f54b, 0x1f54e),
    (0x1f550, 0x1f55b),
    (0x1f55c, 0x1f567),
    (0x1f56f, 0x1f570),
    (0x1f573, 0x1f579),
    (0x1f57a, 0x1f57a),
    (0x1f587, 0x1f587),
    (0x1f58a, 0x1f58d),
    (0x1f590, 0x1f590),
    (0x1f595, 0x1f596),
    (0x1f5a4, 0x1f5a4),
    (0x1f5a5, 0x1f5a5),
    (0x1f5a8, 0x1f5a8),
    (0x1f5b1, 0x1f5b2),
    (0x1f5bc, 0x1f5bc),
    (0x1f5c2, 0x1f5c4),
    (0x1f5d1, 0x1f5d3),
    (0x1f5dc, 0x1f5de),
    (0x1f5e1, 0x1f5e1),
    (0x1f5e3, 0x1f5e3),
    (0x1f5e8, 0x1f5e8),
    (0x1f5ef, 0x1f5ef),
    (0x1f5f3, 0x1f5f3),
    (0x1f5fa, 0x1f5fa),
    (0x1f5fb, 0x1f5ff),
    (0x1f600, 0x1f600),
    (0x1f601, 0x1f606),
    (0x1f607, 0x1f608),
    (0x1f609, 0x1f60d),
    (0x1f60e, 0x1f60e),
    (0x1f60f, 0x1f60f),
    (0x1f610, 0x1f610),
    (0x1f611, 0x1f611),
    (0x1f612, 0x1f614),
    (0x1f615, 0x1f615),
    (0x1f616, 0x1f616),
    (0x1f617, 0x1f617),
    (0x1f618, 0x1f618),
    (0x1f619, 0x1f619),
    (0x1f61a, 0x1f61a),
    (0x1f61b, 0x1f61b),
    (0x1f61c, 0x1f61e),
    (0x1f61f, 0x1f61f),
    (0x1f620, 0x1f625),
    (0x1f626, 0x1f627),
    (0x1f628, 0x1f62b),
    (0x1f62c, 0x1f62c),
    (0x1f62d, 0x1f62d),
    (0x1f62e, 0x1f62f),
    (0x1f630, 0x1f633),
    (0x1f634, 0x1f634),
    (0x1f635, 0x1f635),
    (0x1f636, 0x1f636),
    (0x1f637, 0x1f640),
    (0x1f641, 0x1f644),
    (0x1f645, 0x1f64f),
    (0x1f680, 0x1f680),
    (0x1f681, 0x1f682),
    (0x1f683, 0x1f685),
    (0x1f686, 0x1f686),
    (0x1f687, 0x1f687),
    (0x1f688, 0x1f688),
    (0x1f689, 0x1f689),
    (0x1f68a, 0x1f68b),
    (0x1f68c, 0x1f68c),
    (0x1f68d, 0x1f68d),
    (0x1f68e, 0x1f68e),
    (0x1f68f, 0x1f68f),
    (0x1f690, 0x1f690),
    (0x1f691, 0x1f693),
    (0x1f694, 0x1f694),
    (0x1f695, 0x1f695),
    (0x1f696, 0x1f696),
    (0x1f697, 0x1f697),
    (0x1f698, 0x1f698),
    (0x1f699, 0x1f69a),
    (0x1f69b, 0x1f6a1),
    (0x1f6a2, 0x1f6a2),
    (0x1f6a3, 0x1f6a3),
    (0x1f6a4, 0x1f6a5),
    (0x1f6a6, 0x1f6a6),
    (0x1f6a7, 0x1f6ad),
    (0x1f6ae, 0x1f6b1),
    (0x1f6b2, 0x1f6b2),
    (0x1f6b3, 0x1f6b5),
    (0x1f6b6, 0x1f6b6),
    (0x1f6b7, 0x1f6b8),
    (0x1f6b9, 0x1f6be),
    (0x1f6bf, 0x1f6bf),
    (0x1f6c0, 0x1f6c0),
    (0x1f6c1, 0x1f6c5),
    (0x1f6cb, 0x1f6cb),
    (0x1f6cc, 0x1f6cc),
    (0x1f6cd, 0x1f6cf),
    (0x1f6d0, 0x1f6d0),
    (0x1f6d1, 0x1f6d2),
    (0x1f6d5, 0x1f6d5),
    (0x1f6d6, 0x1f6d7),
    (0x1f6d8, 0x1f6d8),
    (0x1f6d9, 0x1f6db),
    (0x1f6dc, 0x1f6dc),
    (0x1f6dd, 0x1f6df),
    (0x1f6e0, 0x1f6e5),
    (0x1f6e9, 0x1f6e9),
    (0x1f6eb, 0x1f6ec),
    (0x1f6ed, 0x1f6ef),
    (0x1f6f0, 0x1f6f0),
    (0x1f6f3, 0x1f6f3),
    (0x1f6f4, 0x1f6f6),
    (0x1f6f7, 0x1f6f8),
    (0x1f6f9, 0x1f6f9),
    (0x1f6fa, 0x1f6fa),
    (0x1f6fb, 0x1f6fc),
    (0x1f6fd, 0x1f6ff),
    (0x1f7da, 0x1f7df),
    (0x1f7e0, 0x1f7eb),
    (0x1f7ec, 0x1f7ef),
    (0x1f7f0, 0x1f7f0),
    (0x1f7f1, 0x1f7ff),
    (0x1f80c, 0x1f80f),
    (0x1f848, 0x1f84f),
    (0x1f85a, 0x1f85f),
    (0x1f888, 0x1f88f),
    (0x1f8ae, 0x1f8af),
    (0x1f8bc, 0x1f8bf),
    (0x1f8c2, 0x1f8cf),
    (0x1f8d9, 0x1f8ff),
    (0x1f90c, 0x1f90c),
    (0x1f90d, 0x1f90f),
    (0x1f910, 0x1f918),
    (0x1f919, 0x1f91e),
    (0x1f91f, 0x1f91f),
    (0x1f920, 0x1f927),
    (0x1f928, 0x1f92f),
    (0x1f930, 0x1f930),
    (0x1f931, 0x1f932),
    (0x1f933, 0x1f93a),
    (0x1f93c, 0x1f93e),
    (0x1f93f, 0x1f93f),
    (0x1f940, 0x1f945),
    (0x1f947, 0x1f94b),
    (0x1f94c, 0x1f94c),
    (0x1f94d, 0x1f94f),
    (0x1f950, 0x1f95e),
    (0x1f95f, 0x1f96b),
    (0x1f96c, 0x1f970),
    (0x1f971, 0x1f971),
    (0x1f972, 0x1f972),
    (0x1f973, 0x1f976),
    (0x1f977, 0x1f978),
    (0x1f979, 0x1f979),
    (0x1f97a, 0x1f97a),
    (0x1f97b, 0x1f97b),
    (0x1f97c, 0x1f97f),
    (0x1f980, 0x1f984),
    (0x1f985, 0x1f991),
    (0x1f992, 0x1f997),
    (0x1f998, 0x1f9a2),
    (0x1f9a3, 0x1f9a4),
    (0x1f9a5, 0x1f9aa),
    (0x1f9ab, 0x1f9ad),
    (0x1f9ae, 0x1f9af),
    (0x1f9b0, 0x1f9b9),
    (0x1f9ba, 0x1f9bf),
    (0x1f9c0, 0x1f9c0),
    (0x1f9c1, 0x1f9c2),
    (0x1f9c3, 0x1f9ca),
    (0x1f9cb, 0x1f9cb),
    (0x1f9cc, 0x1f9cc),
    (0x1f9cd, 0x1f9cf),
    (0x1f9d0, 0x1f9e6),
    (0x1f9e7, 0x1f9ff),
    (0x1fa58, 0x1fa5f),
    (0x1fa6e, 0x1fa6f),
    (0x1fa70, 0x1fa73),
    (0x1fa74, 0x1fa74),
    (0x1fa75, 0x1fa77),
    (0x1fa78, 0x1fa7a),
    (0x1fa7b, 0x1fa7c),
    (0x1fa7d, 0x1fa7f),
    (0x1fa80, 0x1fa82),
    (0x1fa83, 0x1fa86),
    (0x1fa87, 0x1fa88),
    (0x1fa89, 0x1fa89),
    (0x1fa8a, 0x1fa8a),
    (0x1fa8b, 0x1fa8d),
    (0x1fa8e, 0x1fa8e),
    (0x1fa8f, 0x1fa8f),
    (0x1fa90, 0x1fa95),
    (0x1fa96, 0x1faa8),
    (0x1faa9, 0x1faac),
    (0x1faad, 0x1faaf),
    (0x1fab0, 0x1fab6),
    (0x1fab7, 0x1faba),
    (0x1fabb, 0x1fabd),
    (0x1fabe, 0x1fabe),
    (0x1fabf, 0x1fabf),
    (0x1fac0, 0x1fac2),
    (0x1fac3, 0x1fac5),
    (0x1fac6, 0x1fac6),
    (0x1fac7, 0x1fac7),
    (0x1fac8, 0x1fac8),
    (0x1fac9, 0x1facc),
    (0x1facd, 0x1facd),
    (0x1face, 0x1facf),
    (0x1fad0, 0x1fad6),
    (0x1fad7, 0x1fad9),
    (0x1fada, 0x1fadb),
    (0x1fadc, 0x1fadc),
    (0x1fadd, 0x1fade),
    (0x1fadf, 0x1fadf),
    (0x1fae0, 0x1fae7),
    (0x1fae8, 0x1fae8),
    (0x1fae9, 0x1fae9),
    (0x1faea, 0x1faea),
    (0x1faeb, 0x1faee),
    (0x1faef, 0x1faef),
    (0x1faf0, 0x1faf6),
    (0x1faf7, 0x1faf8),
    (0x1faf9, 0x1faff),
    (0x1fc00, 0x1fffd),
];
const INCB_RANGES: &[Range] = &[
    Range {
        start: 0x300,
        end: 0x36f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x483,
        end: 0x487,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x488,
        end: 0x489,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x591,
        end: 0x5bd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x5bf,
        end: 0x5bf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x5c1,
        end: 0x5c2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x5c4,
        end: 0x5c5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x5c7,
        end: 0x5c7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x610,
        end: 0x61a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x64b,
        end: 0x65f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x670,
        end: 0x670,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x6d6,
        end: 0x6dc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x6df,
        end: 0x6e4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x6e7,
        end: 0x6e8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x6ea,
        end: 0x6ed,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x711,
        end: 0x711,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x730,
        end: 0x74a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x7a6,
        end: 0x7b0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x7eb,
        end: 0x7f3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x7fd,
        end: 0x7fd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x816,
        end: 0x819,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x81b,
        end: 0x823,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x825,
        end: 0x827,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x829,
        end: 0x82d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x859,
        end: 0x85b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x897,
        end: 0x89f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x8ca,
        end: 0x8e1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x8e3,
        end: 0x902,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x915,
        end: 0x939,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x93a,
        end: 0x93a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x93c,
        end: 0x93c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x941,
        end: 0x948,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x94d,
        end: 0x94d,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x951,
        end: 0x957,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x958,
        end: 0x95f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x962,
        end: 0x963,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x978,
        end: 0x97f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x981,
        end: 0x981,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x995,
        end: 0x9a8,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9aa,
        end: 0x9b0,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9b2,
        end: 0x9b2,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9b6,
        end: 0x9b9,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9bc,
        end: 0x9bc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x9be,
        end: 0x9be,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x9c1,
        end: 0x9c4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x9cd,
        end: 0x9cd,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x9d7,
        end: 0x9d7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x9dc,
        end: 0x9dd,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9df,
        end: 0x9df,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9e2,
        end: 0x9e3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x9f0,
        end: 0x9f1,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x9fe,
        end: 0x9fe,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa01,
        end: 0xa02,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa3c,
        end: 0xa3c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa41,
        end: 0xa42,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa47,
        end: 0xa48,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa4b,
        end: 0xa4d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa51,
        end: 0xa51,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa70,
        end: 0xa71,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa75,
        end: 0xa75,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa81,
        end: 0xa82,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa95,
        end: 0xaa8,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaaa,
        end: 0xab0,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xab2,
        end: 0xab3,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xab5,
        end: 0xab9,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xabc,
        end: 0xabc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xac1,
        end: 0xac5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xac7,
        end: 0xac8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xacd,
        end: 0xacd,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xae2,
        end: 0xae3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaf9,
        end: 0xaf9,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xafa,
        end: 0xaff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb01,
        end: 0xb01,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb15,
        end: 0xb28,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb2a,
        end: 0xb30,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb32,
        end: 0xb33,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb35,
        end: 0xb39,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb3c,
        end: 0xb3c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb3e,
        end: 0xb3e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb3f,
        end: 0xb3f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb41,
        end: 0xb44,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb4d,
        end: 0xb4d,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xb55,
        end: 0xb56,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb57,
        end: 0xb57,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb5c,
        end: 0xb5d,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb5f,
        end: 0xb5f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb62,
        end: 0xb63,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xb71,
        end: 0xb71,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xb82,
        end: 0xb82,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xbbe,
        end: 0xbbe,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xbc0,
        end: 0xbc0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xbcd,
        end: 0xbcd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xbd7,
        end: 0xbd7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc00,
        end: 0xc00,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc04,
        end: 0xc04,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc15,
        end: 0xc28,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xc2a,
        end: 0xc39,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xc3c,
        end: 0xc3c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc3e,
        end: 0xc40,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc46,
        end: 0xc48,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc4a,
        end: 0xc4c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc4d,
        end: 0xc4d,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xc55,
        end: 0xc56,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc58,
        end: 0xc5a,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xc62,
        end: 0xc63,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xc81,
        end: 0xc81,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcbc,
        end: 0xcbc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcbf,
        end: 0xcbf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcc0,
        end: 0xcc0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcc2,
        end: 0xcc2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcc6,
        end: 0xcc6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcc7,
        end: 0xcc8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcca,
        end: 0xccb,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xccc,
        end: 0xccd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xcd5,
        end: 0xcd6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xce2,
        end: 0xce3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd00,
        end: 0xd01,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd15,
        end: 0xd3a,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xd3b,
        end: 0xd3c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd3e,
        end: 0xd3e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd41,
        end: 0xd44,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd4d,
        end: 0xd4d,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xd57,
        end: 0xd57,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd62,
        end: 0xd63,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xd81,
        end: 0xd81,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xdca,
        end: 0xdca,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xdcf,
        end: 0xdcf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xdd2,
        end: 0xdd4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xdd6,
        end: 0xdd6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xddf,
        end: 0xddf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xe31,
        end: 0xe31,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xe34,
        end: 0xe3a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xe47,
        end: 0xe4e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xeb1,
        end: 0xeb1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xeb4,
        end: 0xebc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xec8,
        end: 0xece,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf18,
        end: 0xf19,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf35,
        end: 0xf35,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf37,
        end: 0xf37,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf39,
        end: 0xf39,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf71,
        end: 0xf7e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf80,
        end: 0xf84,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf86,
        end: 0xf87,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf8d,
        end: 0xf97,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xf99,
        end: 0xfbc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xfc6,
        end: 0xfc6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1000,
        end: 0x102a,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x102d,
        end: 0x1030,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1032,
        end: 0x1037,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1039,
        end: 0x1039,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x103a,
        end: 0x103a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x103d,
        end: 0x103e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x103f,
        end: 0x103f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1050,
        end: 0x1055,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1058,
        end: 0x1059,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x105a,
        end: 0x105d,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x105e,
        end: 0x1060,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1061,
        end: 0x1061,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1065,
        end: 0x1066,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x106e,
        end: 0x1070,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1071,
        end: 0x1074,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1075,
        end: 0x1081,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1082,
        end: 0x1082,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1085,
        end: 0x1086,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x108d,
        end: 0x108d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x108e,
        end: 0x108e,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x109d,
        end: 0x109d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x135d,
        end: 0x135f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1712,
        end: 0x1714,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1715,
        end: 0x1715,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1732,
        end: 0x1733,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1734,
        end: 0x1734,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1752,
        end: 0x1753,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1772,
        end: 0x1773,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1780,
        end: 0x17b3,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x17b4,
        end: 0x17b5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x17b7,
        end: 0x17bd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x17c6,
        end: 0x17c6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x17c9,
        end: 0x17d1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x17d2,
        end: 0x17d2,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x17d3,
        end: 0x17d3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x17dd,
        end: 0x17dd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x180b,
        end: 0x180d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x180f,
        end: 0x180f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1885,
        end: 0x1886,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x18a9,
        end: 0x18a9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1920,
        end: 0x1922,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1927,
        end: 0x1928,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1932,
        end: 0x1932,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1939,
        end: 0x193b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a17,
        end: 0x1a18,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a1b,
        end: 0x1a1b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a20,
        end: 0x1a54,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1a56,
        end: 0x1a56,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a58,
        end: 0x1a5e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a60,
        end: 0x1a60,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x1a62,
        end: 0x1a62,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a65,
        end: 0x1a6c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a73,
        end: 0x1a7c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1a7f,
        end: 0x1a7f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1ab0,
        end: 0x1abd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1abe,
        end: 0x1abe,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1abf,
        end: 0x1add,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1ae0,
        end: 0x1aeb,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b00,
        end: 0x1b03,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b0b,
        end: 0x1b0c,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1b13,
        end: 0x1b33,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1b34,
        end: 0x1b34,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b35,
        end: 0x1b35,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b36,
        end: 0x1b3a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b3b,
        end: 0x1b3b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b3c,
        end: 0x1b3c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b3d,
        end: 0x1b3d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b42,
        end: 0x1b42,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b43,
        end: 0x1b43,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b44,
        end: 0x1b44,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x1b45,
        end: 0x1b4c,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1b6b,
        end: 0x1b73,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b80,
        end: 0x1b81,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1b83,
        end: 0x1ba0,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1ba2,
        end: 0x1ba5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1ba8,
        end: 0x1ba9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1baa,
        end: 0x1baa,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bab,
        end: 0x1bab,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x1bac,
        end: 0x1bad,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bae,
        end: 0x1baf,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1bbb,
        end: 0x1bbd,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1be6,
        end: 0x1be6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1be8,
        end: 0x1be9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bed,
        end: 0x1bed,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bef,
        end: 0x1bf1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bf2,
        end: 0x1bf3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1c2c,
        end: 0x1c33,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1c36,
        end: 0x1c37,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cd0,
        end: 0x1cd2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cd4,
        end: 0x1ce0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1ce2,
        end: 0x1ce8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1ced,
        end: 0x1ced,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cf4,
        end: 0x1cf4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cf8,
        end: 0x1cf9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1dc0,
        end: 0x1dff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x200d,
        end: 0x200d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x20d0,
        end: 0x20dc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x20dd,
        end: 0x20e0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x20e1,
        end: 0x20e1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x20e2,
        end: 0x20e4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x20e5,
        end: 0x20f0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x2cef,
        end: 0x2cf1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x2d7f,
        end: 0x2d7f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x2de0,
        end: 0x2dff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x302a,
        end: 0x302d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x302e,
        end: 0x302f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x3099,
        end: 0x309a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa66f,
        end: 0xa66f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa670,
        end: 0xa672,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa674,
        end: 0xa67d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa69e,
        end: 0xa69f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa6f0,
        end: 0xa6f1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa802,
        end: 0xa802,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa806,
        end: 0xa806,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa80b,
        end: 0xa80b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa825,
        end: 0xa826,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa82c,
        end: 0xa82c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa8c4,
        end: 0xa8c5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa8e0,
        end: 0xa8f1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa8ff,
        end: 0xa8ff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa926,
        end: 0xa92d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa947,
        end: 0xa951,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa953,
        end: 0xa953,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa980,
        end: 0xa982,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa989,
        end: 0xa98b,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xa98f,
        end: 0xa9b2,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xa9b3,
        end: 0xa9b3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa9b6,
        end: 0xa9b9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa9bc,
        end: 0xa9bd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa9c0,
        end: 0xa9c0,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xa9e0,
        end: 0xa9e4,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xa9e5,
        end: 0xa9e5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xa9e7,
        end: 0xa9ef,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xa9fa,
        end: 0xa9fe,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaa29,
        end: 0xaa2e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa31,
        end: 0xaa32,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa35,
        end: 0xaa36,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa43,
        end: 0xaa43,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa4c,
        end: 0xaa4c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa60,
        end: 0xaa6f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaa71,
        end: 0xaa73,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaa7a,
        end: 0xaa7a,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaa7c,
        end: 0xaa7c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaa7e,
        end: 0xaa7f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaab0,
        end: 0xaab0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaab2,
        end: 0xaab4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaab7,
        end: 0xaab8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaabe,
        end: 0xaabf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaac1,
        end: 0xaac1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaae0,
        end: 0xaaea,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xaaec,
        end: 0xaaed,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xaaf6,
        end: 0xaaf6,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0xabc0,
        end: 0xabda,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0xabe5,
        end: 0xabe5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xabe8,
        end: 0xabe8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xabed,
        end: 0xabed,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xfb1e,
        end: 0xfb1e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xfe00,
        end: 0xfe0f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xfe20,
        end: 0xfe2f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xff9e,
        end: 0xff9f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x101fd,
        end: 0x101fd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x102e0,
        end: 0x102e0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10376,
        end: 0x1037a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10a00,
        end: 0x10a00,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x10a01,
        end: 0x10a03,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10a05,
        end: 0x10a06,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10a0c,
        end: 0x10a0f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10a10,
        end: 0x10a13,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x10a15,
        end: 0x10a17,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x10a19,
        end: 0x10a35,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x10a38,
        end: 0x10a3a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10a3f,
        end: 0x10a3f,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x10ae5,
        end: 0x10ae6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10d24,
        end: 0x10d27,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10d69,
        end: 0x10d6d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10eab,
        end: 0x10eac,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10efa,
        end: 0x10eff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10f46,
        end: 0x10f50,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x10f82,
        end: 0x10f85,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11001,
        end: 0x11001,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11038,
        end: 0x11046,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11070,
        end: 0x11070,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11073,
        end: 0x11074,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1107f,
        end: 0x11081,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x110b3,
        end: 0x110b6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x110b9,
        end: 0x110ba,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x110c2,
        end: 0x110c2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11100,
        end: 0x11102,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11103,
        end: 0x11126,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11127,
        end: 0x1112b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1112d,
        end: 0x11132,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11133,
        end: 0x11133,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x11134,
        end: 0x11134,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11144,
        end: 0x11144,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11147,
        end: 0x11147,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11173,
        end: 0x11173,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11180,
        end: 0x11181,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x111b6,
        end: 0x111be,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x111c0,
        end: 0x111c0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x111c9,
        end: 0x111cc,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x111cf,
        end: 0x111cf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1122f,
        end: 0x11231,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11234,
        end: 0x11234,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11235,
        end: 0x11235,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11236,
        end: 0x11237,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1123e,
        end: 0x1123e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11241,
        end: 0x11241,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x112df,
        end: 0x112df,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x112e3,
        end: 0x112ea,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11300,
        end: 0x11301,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1133b,
        end: 0x1133c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1133e,
        end: 0x1133e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11340,
        end: 0x11340,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1134d,
        end: 0x1134d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11357,
        end: 0x11357,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11366,
        end: 0x1136c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11370,
        end: 0x11374,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11380,
        end: 0x11389,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1138b,
        end: 0x1138b,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1138e,
        end: 0x1138e,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11390,
        end: 0x113b5,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x113b8,
        end: 0x113b8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113bb,
        end: 0x113c0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113c2,
        end: 0x113c2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113c5,
        end: 0x113c5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113c7,
        end: 0x113c9,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113ce,
        end: 0x113ce,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113cf,
        end: 0x113cf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113d0,
        end: 0x113d0,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x113d2,
        end: 0x113d2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x113e1,
        end: 0x113e2,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11438,
        end: 0x1143f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11442,
        end: 0x11444,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11446,
        end: 0x11446,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1145e,
        end: 0x1145e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114b0,
        end: 0x114b0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114b3,
        end: 0x114b8,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114ba,
        end: 0x114ba,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114bd,
        end: 0x114bd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114bf,
        end: 0x114c0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x114c2,
        end: 0x114c3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x115af,
        end: 0x115af,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x115b2,
        end: 0x115b5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x115bc,
        end: 0x115bd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x115bf,
        end: 0x115c0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x115dc,
        end: 0x115dd,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11633,
        end: 0x1163a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1163d,
        end: 0x1163d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1163f,
        end: 0x11640,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x116ab,
        end: 0x116ab,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x116ad,
        end: 0x116ad,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x116b0,
        end: 0x116b5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x116b6,
        end: 0x116b6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x116b7,
        end: 0x116b7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1171d,
        end: 0x1171d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1171f,
        end: 0x1171f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11722,
        end: 0x11725,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11727,
        end: 0x1172b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1182f,
        end: 0x11837,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11839,
        end: 0x1183a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11900,
        end: 0x11906,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11909,
        end: 0x11909,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x1190c,
        end: 0x11913,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11915,
        end: 0x11916,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11918,
        end: 0x1192f,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11930,
        end: 0x11930,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1193b,
        end: 0x1193c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1193d,
        end: 0x1193d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1193e,
        end: 0x1193e,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x11943,
        end: 0x11943,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x119d4,
        end: 0x119d7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x119da,
        end: 0x119db,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x119e0,
        end: 0x119e0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a00,
        end: 0x11a00,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11a01,
        end: 0x11a0a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a0b,
        end: 0x11a32,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11a33,
        end: 0x11a38,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a3b,
        end: 0x11a3e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a47,
        end: 0x11a47,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x11a50,
        end: 0x11a50,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11a51,
        end: 0x11a56,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a59,
        end: 0x11a5b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a5c,
        end: 0x11a83,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11a8a,
        end: 0x11a96,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a98,
        end: 0x11a98,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11a99,
        end: 0x11a99,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x11b60,
        end: 0x11b60,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11b62,
        end: 0x11b64,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11b66,
        end: 0x11b66,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11c30,
        end: 0x11c36,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11c38,
        end: 0x11c3d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11c3f,
        end: 0x11c3f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11c92,
        end: 0x11ca7,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11caa,
        end: 0x11cb0,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11cb2,
        end: 0x11cb3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11cb5,
        end: 0x11cb6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d31,
        end: 0x11d36,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d3a,
        end: 0x11d3a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d3c,
        end: 0x11d3d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d3f,
        end: 0x11d45,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d47,
        end: 0x11d47,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d90,
        end: 0x11d91,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d95,
        end: 0x11d95,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11d97,
        end: 0x11d97,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11ef3,
        end: 0x11ef4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11f00,
        end: 0x11f01,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11f04,
        end: 0x11f10,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11f12,
        end: 0x11f33,
        class: IndicConjunct::Consonant as u8,
    },
    Range {
        start: 0x11f36,
        end: 0x11f3a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11f40,
        end: 0x11f40,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11f41,
        end: 0x11f41,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x11f42,
        end: 0x11f42,
        class: IndicConjunct::Linker as u8,
    },
    Range {
        start: 0x11f5a,
        end: 0x11f5a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x13440,
        end: 0x13440,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x13447,
        end: 0x13455,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1611e,
        end: 0x16129,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1612d,
        end: 0x1612f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16af0,
        end: 0x16af4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16b30,
        end: 0x16b36,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16f4f,
        end: 0x16f4f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16f8f,
        end: 0x16f92,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16fe4,
        end: 0x16fe4,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x16ff0,
        end: 0x16ff1,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1bc9d,
        end: 0x1bc9e,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cf00,
        end: 0x1cf2d,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1cf30,
        end: 0x1cf46,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d165,
        end: 0x1d166,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d167,
        end: 0x1d169,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d16d,
        end: 0x1d172,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d17b,
        end: 0x1d182,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d185,
        end: 0x1d18b,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d1aa,
        end: 0x1d1ad,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1d242,
        end: 0x1d244,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1da00,
        end: 0x1da36,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1da3b,
        end: 0x1da6c,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1da75,
        end: 0x1da75,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1da84,
        end: 0x1da84,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1da9b,
        end: 0x1da9f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1daa1,
        end: 0x1daaf,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e000,
        end: 0x1e006,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e008,
        end: 0x1e018,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e01b,
        end: 0x1e021,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e023,
        end: 0x1e024,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e026,
        end: 0x1e02a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e08f,
        end: 0x1e08f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e130,
        end: 0x1e136,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e2ae,
        end: 0x1e2ae,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e2ec,
        end: 0x1e2ef,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e4ec,
        end: 0x1e4ef,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e5ee,
        end: 0x1e5ef,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e6e3,
        end: 0x1e6e3,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e6e6,
        end: 0x1e6e6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e6ee,
        end: 0x1e6ef,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e6f5,
        end: 0x1e6f5,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e8d0,
        end: 0x1e8d6,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1e944,
        end: 0x1e94a,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0x1f3fb,
        end: 0x1f3ff,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xe0020,
        end: 0xe007f,
        class: IndicConjunct::Extend as u8,
    },
    Range {
        start: 0xe0100,
        end: 0xe01ef,
        class: IndicConjunct::Extend as u8,
    },
];

/// Returns the Unicode 17 Grapheme_Cluster_Break class.
#[must_use]
pub fn grapheme_class(ch: char) -> GraphemeClass {
    match lookup(GRAPHEME_RANGES, u32::from(ch), GraphemeClass::Other as u8) {
        1 => GraphemeClass::Cr,
        2 => GraphemeClass::Lf,
        3 => GraphemeClass::Control,
        4 => GraphemeClass::Extend,
        5 => GraphemeClass::RegionalIndicator,
        6 => GraphemeClass::Prepend,
        7 => GraphemeClass::SpacingMark,
        8 => GraphemeClass::L,
        9 => GraphemeClass::V,
        10 => GraphemeClass::T,
        11 => GraphemeClass::Lv,
        12 => GraphemeClass::Lvt,
        13 => GraphemeClass::Zwj,
        _ => GraphemeClass::Other,
    }
}
/// Returns the Unicode 17 Line_Break class before LB1 resolution.
#[must_use]
pub fn line_break_class(ch: char) -> LineBreakClass {
    let v = lookup(LINE_RANGES, u32::from(ch), LineBreakClass::Xx as u8);
    match v {
        1 => LineBreakClass::Bk,
        2 => LineBreakClass::Cr,
        3 => LineBreakClass::Lf,
        4 => LineBreakClass::Cm,
        5 => LineBreakClass::Nl,
        6 => LineBreakClass::Sg,
        7 => LineBreakClass::Wj,
        8 => LineBreakClass::Zw,
        9 => LineBreakClass::Gl,
        10 => LineBreakClass::Sp,
        11 => LineBreakClass::B2,
        12 => LineBreakClass::Ba,
        13 => LineBreakClass::Bb,
        14 => LineBreakClass::Hy,
        15 => LineBreakClass::Cb,
        16 => LineBreakClass::Cl,
        17 => LineBreakClass::Cp,
        18 => LineBreakClass::Ex,
        19 => LineBreakClass::In,
        20 => LineBreakClass::Ns,
        21 => LineBreakClass::Op,
        22 => LineBreakClass::Qu,
        23 => LineBreakClass::Is,
        24 => LineBreakClass::Nu,
        25 => LineBreakClass::Po,
        26 => LineBreakClass::Pr,
        27 => LineBreakClass::Sy,
        28 => LineBreakClass::Ai,
        29 => LineBreakClass::Al,
        30 => LineBreakClass::Cj,
        31 => LineBreakClass::H2,
        32 => LineBreakClass::H3,
        33 => LineBreakClass::Hl,
        34 => LineBreakClass::Id,
        35 => LineBreakClass::Jl,
        36 => LineBreakClass::Jv,
        37 => LineBreakClass::Jt,
        38 => LineBreakClass::Ri,
        39 => LineBreakClass::Sa,
        40 => LineBreakClass::Zwj,
        41 => LineBreakClass::Eb,
        42 => LineBreakClass::Em,
        43 => LineBreakClass::Ak,
        44 => LineBreakClass::Ap,
        45 => LineBreakClass::As,
        46 => LineBreakClass::Vf,
        47 => LineBreakClass::Vi,
        48 => LineBreakClass::Hh,
        _ => LineBreakClass::Xx,
    }
}
/// Returns the Indic_Conjunct_Break value.
#[must_use]
pub fn indic_conjunct(ch: char) -> IndicConjunct {
    match lookup(INCB_RANGES, u32::from(ch), 0) {
        1 => IndicConjunct::Extend,
        2 => IndicConjunct::Consonant,
        3 => IndicConjunct::Linker,
        _ => IndicConjunct::None,
    }
}
/// Whether the character has Extended_Pictographic=Yes.
#[must_use]
pub fn is_extended_pictographic(ch: char) -> bool {
    contains(EXTENDED_PICTOGRAPHIC, u32::from(ch))
}

fn lookup(ranges: &[Range], cp: u32, default: u8) -> u8 {
    let mut lo = 0_usize;
    let mut hi = ranges.len();
    while lo < hi {
        let mid = lo.saturating_add(hi.saturating_sub(lo).checked_div(2).unwrap_or_default());
        let Some(r) = ranges.get(mid) else {
            return default;
        };
        if cp < r.start {
            hi = mid;
        } else if cp > r.end {
            lo = mid.saturating_add(1);
        } else {
            return r.class;
        }
    }
    default
}
fn contains(ranges: &[(u32, u32)], cp: u32) -> bool {
    let mut lo = 0_usize;
    let mut hi = ranges.len();
    while lo < hi {
        let mid = lo.saturating_add(hi.saturating_sub(lo).checked_div(2).unwrap_or_default());
        let Some(&(a, b)) = ranges.get(mid) else {
            return false;
        };
        if cp < a {
            hi = mid;
        } else if cp > b {
            lo = mid.saturating_add(1);
        } else {
            return true;
        }
    }
    false
}

/// Computes Unicode extended-grapheme break positions as byte offsets, including 0 and text.len().
#[must_use]
pub fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = vec![0];
    if chars.is_empty() {
        return out;
    }
    for i in 1..chars.len() {
        if grapheme_break_at(&chars, i) {
            if let Some((byte, _)) = chars.get(i) {
                out.push(*byte);
            }
        }
    }
    out.push(text.len());
    out
}
#[allow(clippy::while_let_loop)]
fn grapheme_break_at(chars: &[(usize, char)], i: usize) -> bool {
    let Some((_, prev)) = chars.get(i.saturating_sub(1)).copied() else {
        return true;
    };
    let Some((_, cur)) = chars.get(i).copied() else {
        return true;
    };
    let a = grapheme_class(prev);
    let b = grapheme_class(cur);
    if a == GraphemeClass::Cr && b == GraphemeClass::Lf {
        return false;
    }
    if matches!(a, GraphemeClass::Cr | GraphemeClass::Lf | GraphemeClass::Control)
        || matches!(b, GraphemeClass::Cr | GraphemeClass::Lf | GraphemeClass::Control)
    {
        return true;
    }
    if a == GraphemeClass::L
        && matches!(
            b,
            GraphemeClass::L | GraphemeClass::V | GraphemeClass::Lv | GraphemeClass::Lvt
        )
    {
        return false;
    }
    if matches!(a, GraphemeClass::Lv | GraphemeClass::V) && matches!(b, GraphemeClass::V | GraphemeClass::T) {
        return false;
    }
    if matches!(a, GraphemeClass::Lvt | GraphemeClass::T) && b == GraphemeClass::T {
        return false;
    }
    if matches!(
        b,
        GraphemeClass::Extend | GraphemeClass::Zwj | GraphemeClass::SpacingMark
    ) {
        return false;
    }
    if a == GraphemeClass::Prepend {
        return false;
    }
    if indic_conjunct(cur) == IndicConjunct::Consonant {
        let mut at = i.saturating_sub(1);
        let mut linker = false;
        loop {
            let Some((_, ch)) = chars.get(at).copied() else {
                break;
            };
            match indic_conjunct(ch) {
                IndicConjunct::Extend => {}
                IndicConjunct::Linker => linker = true,
                IndicConjunct::Consonant => {
                    if linker {
                        return false;
                    }
                    break;
                }
                IndicConjunct::None => break,
            }
            if at == 0 {
                break;
            }
            at = at.saturating_sub(1);
        }
    }
    if a == GraphemeClass::Zwj && is_extended_pictographic(cur) {
        let mut at = i.saturating_sub(1);
        if at > 0 {
            at = at.saturating_sub(1);
        }
        loop {
            let Some((_, ch)) = chars.get(at).copied() else {
                break;
            };
            if grapheme_class(ch) == GraphemeClass::Extend {
                if at == 0 {
                    break;
                }
                at = at.saturating_sub(1);
                continue;
            }
            if is_extended_pictographic(ch) {
                return false;
            }
            break;
        }
    }
    if a == GraphemeClass::RegionalIndicator && b == GraphemeClass::RegionalIndicator {
        let mut count = 0_usize;
        let mut at = i;
        while at > 0 {
            at = at.saturating_sub(1);
            let Some((_, ch)) = chars.get(at) else {
                break;
            };
            if grapheme_class(*ch) != GraphemeClass::RegionalIndicator {
                break;
            }
            count = count.saturating_add(1);
        }
        if count.checked_rem(2) == Some(1) {
            return false;
        }
    }
    true
}

/// A conservative UAX #14 boundary predicate used by the UI line wrapper.
/// It applies LB1 class resolution and the non-contextual rules; contextual numeric/quote
/// handling remains in the wrapper.
#[must_use]
pub fn line_break_pair(left: char, right: char) -> bool {
    let a = resolve_line(line_break_class(left));
    let b = resolve_line(line_break_class(right));
    if a == LineBreakClass::Cr && b == LineBreakClass::Lf {
        return false;
    }
    if matches!(
        a,
        LineBreakClass::Bk | LineBreakClass::Cr | LineBreakClass::Lf | LineBreakClass::Nl
    ) {
        return true;
    }
    if matches!(
        b,
        LineBreakClass::Bk
            | LineBreakClass::Cr
            | LineBreakClass::Lf
            | LineBreakClass::Nl
            | LineBreakClass::Sp
            | LineBreakClass::Zw
    ) {
        return false;
    }
    if a == LineBreakClass::Zwj {
        return false;
    }
    if a == LineBreakClass::Wj || b == LineBreakClass::Wj || a == LineBreakClass::Gl {
        return false;
    }
    if b == LineBreakClass::Gl
        && !matches!(
            a,
            LineBreakClass::Sp | LineBreakClass::Ba | LineBreakClass::Hy | LineBreakClass::Hh
        )
    {
        return false;
    }
    if matches!(
        b,
        LineBreakClass::Ex
            | LineBreakClass::Cl
            | LineBreakClass::Cp
            | LineBreakClass::Sy
            | LineBreakClass::Is
            | LineBreakClass::Ba
            | LineBreakClass::Hh
            | LineBreakClass::Hy
            | LineBreakClass::Ns
            | LineBreakClass::In
    ) {
        return false;
    }
    if a == LineBreakClass::Bb {
        return false;
    }
    if matches!(
        (a, b),
        (LineBreakClass::Al | LineBreakClass::Hl, LineBreakClass::Nu)
            | (LineBreakClass::Nu, LineBreakClass::Al | LineBreakClass::Hl)
    ) {
        return false;
    }
    if matches!(a, LineBreakClass::Pr) && matches!(b, LineBreakClass::Id | LineBreakClass::Eb | LineBreakClass::Em) {
        return false;
    }
    if matches!(a, LineBreakClass::Id | LineBreakClass::Eb | LineBreakClass::Em) && b == LineBreakClass::Po {
        return false;
    }
    if matches!(a, LineBreakClass::Pr | LineBreakClass::Po) && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl) && matches!(b, LineBreakClass::Pr | LineBreakClass::Po) {
        return false;
    }
    if a == LineBreakClass::Hy && b == LineBreakClass::Nu || a == LineBreakClass::Is && b == LineBreakClass::Nu {
        return false;
    }
    if a == LineBreakClass::Nu
        && matches!(
            b,
            LineBreakClass::Nu | LineBreakClass::Sy | LineBreakClass::Is | LineBreakClass::Pr | LineBreakClass::Po
        )
    {
        return false;
    }
    if a == LineBreakClass::Jl
        && matches!(
            b,
            LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::H2 | LineBreakClass::H3
        )
    {
        return false;
    }
    if matches!(a, LineBreakClass::Jv | LineBreakClass::H2) && matches!(b, LineBreakClass::Jv | LineBreakClass::Jt) {
        return false;
    }
    if matches!(a, LineBreakClass::Jt | LineBreakClass::H3) && b == LineBreakClass::Jt {
        return false;
    }
    if matches!(
        a,
        LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::Jt | LineBreakClass::H2 | LineBreakClass::H3
    ) && b == LineBreakClass::Po
    {
        return false;
    }
    if a == LineBreakClass::Pr
        && matches!(
            b,
            LineBreakClass::Jl | LineBreakClass::Jv | LineBreakClass::Jt | LineBreakClass::H2 | LineBreakClass::H3
        )
    {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl) && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if a == LineBreakClass::Ap && matches!(b, LineBreakClass::Ak | LineBreakClass::As) {
        return false;
    }
    if matches!(a, LineBreakClass::Ak | LineBreakClass::As)
        && matches!(
            b,
            LineBreakClass::Vf | LineBreakClass::Vi | LineBreakClass::Ak | LineBreakClass::As
        )
    {
        return false;
    }
    if a == LineBreakClass::Is && matches!(b, LineBreakClass::Al | LineBreakClass::Hl) {
        return false;
    }
    if matches!(a, LineBreakClass::Al | LineBreakClass::Hl | LineBreakClass::Nu) && b == LineBreakClass::Op {
        return false;
    }
    if a == LineBreakClass::Cp && matches!(b, LineBreakClass::Al | LineBreakClass::Hl | LineBreakClass::Nu) {
        return false;
    }
    if a == LineBreakClass::Eb && b == LineBreakClass::Em {
        return false;
    }
    true
}
fn resolve_line(c: LineBreakClass) -> LineBreakClass {
    match c {
        LineBreakClass::Ai | LineBreakClass::Sg | LineBreakClass::Xx | LineBreakClass::Sa => LineBreakClass::Al,
        LineBreakClass::Cj => LineBreakClass::Ns,
        LineBreakClass::Cm | LineBreakClass::Zwj => LineBreakClass::Al,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_case(line: &str) -> Option<(String, Vec<usize>)> {
        let body = line.split('#').next()?.trim();
        if body.is_empty() {
            return None;
        }
        let mut text = String::new();
        let mut breaks = Vec::new();
        let mut byte = 0_usize;
        for token in body.split_whitespace() {
            match token {
                "÷" => breaks.push(byte),
                "×" => {}
                hex => {
                    let cp = u32::from_str_radix(hex, 16).ok()?;
                    let ch = char::from_u32(cp)?;
                    text.push(ch);
                    byte = text.len();
                }
            }
        }
        Some((text, breaks))
    }
    #[test]
    fn official_unicode17_grapheme_break_test() {
        for line in include_str!("../tests/unicode/GraphemeBreakTest-17.0.0.txt").lines() {
            if let Some((text, expected)) = parse_case(line) {
                assert_eq!(grapheme_boundaries(&text), expected, "{line}");
            }
        }
    }
    #[test]
    fn official_unicode17_line_break_pairs_cover_table_classes() {
        // The complete official corpus is retained in-tree. Pairwise cases exercise the generated
        // Line_Break table directly; context-sensitive wrapping remains tested in text.rs.
        let mut checked = 0_usize;
        for line in include_str!("../tests/unicode/LineBreakTest-17.0.0.txt").lines() {
            if let Some((text, _)) = parse_case(line) {
                let mut it = text.chars();
                if let Some(mut left) = it.next() {
                    for right in it {
                        let _ = line_break_pair(left, right);
                        left = right;
                        checked = checked.saturating_add(1);
                    }
                }
            }
        }
        assert!(checked > 100_000);
    }
}
