//! Color metadata types.
//!
//! These mirror the FFmpeg/libavutil color enumerations, which FFMS2 also uses
//! for its raw values. VapourSynth frame properties follow the same numbering
//! for the standard color keys.

/// The pixel value range (limited vs. full) of the video.
///
/// Numbering matches `AVColorRange`/FFMS2 `FFMS_ColorRanges`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRange {
    /// Range is unspecified.
    Unspecified,
    /// Limited range (MPEG), e.g. Y in [16, 235] for 8-bit.
    Limited,
    /// Full range (JPEG), e.g. Y in [0, 255] for 8-bit.
    Full,
}

/// The color primaries (gamut) of the video.
///
/// Numbering matches `AVColorPrimaries`/FFMS2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorPrimaries {
    /// Reserved value 0.
    Reserved0,
    /// ITU-R BT.709 (sRGB-derived, HD standard).
    BT709,
    /// Primaries are unspecified.
    Unspecified,
    /// Reserved value 3.
    Reserved,
    /// ITU-R BT.470 System M (NTSC).
    BT470M,
    /// ITU-R BT.470 System B/G (PAL).
    BT470BG,
    /// SMPTE 170M (NTSC).
    SMPTE170M,
    /// SMPTE 240M.
    SMPTE240M,
    /// Generic film gamut.
    Film,
    /// ITU-R BT.2020 (UHD standard).
    BT2020,
    /// SMPTE ST 428-1 (XYZ, digital cinema).
    SMPTE428,
    /// SMPTE RP 431-2 (DCI-P3).
    SMPTE431,
    /// SMPTE EG 432-1 (Display-P3).
    SMPTE432,
    /// EBU Tech. 3213 (JEDEC P22).
    EBU3213,
    /// Versatile Gamut (FFmpeg extension, value 256).
    VGamut,
}

/// The transfer characteristics of the video.
///
/// Numbering matches `AVColorTransferCharacteristic`/FFMS2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(
    non_camel_case_types,
    reason = "variant names intentionally mirror the FFmpeg/libavutil spelling"
)]
pub enum TransferCharacteristic {
    /// Reserved value 0.
    Reserved0,
    /// ITU-R BT.709 (about 2.2 power gamma).
    BT709,
    /// Transfer characteristics are unspecified.
    Unspecified,
    /// Reserved value 3.
    Reserved,
    /// Gamma 2.2.
    GAMMA22,
    /// Gamma 2.8.
    GAMMA28,
    /// SMPTE 170M (BT.601-6 525-line).
    SMPTE170M,
    /// SMPTE 240M.
    SMPTE240M,
    /// Linear transfer.
    Linear,
    /// Logarithmic (100:1).
    Log,
    /// Logarithmic (100 * sqrt(10):1).
    LogSqrt,
    /// IEC 61966-2-4.
    IEC61966_2_4,
    /// ITU-R BT.1361 extended color gamut.
    BT1361_ECG,
    /// IEC 61966-2-1 (sRGB).
    IEC61966_2_1,
    /// ITU-R BT.2020 for 10-bit systems.
    BT2020_10,
    /// ITU-R BT.2020 for 12-bit systems.
    BT2020_12,
    /// SMPTE ST 2084 (PQ, HDR10).
    SMPTE2084,
    /// SMPTE ST 428-1.
    SMPTE428,
    /// ARIB STD-B67 (HLG).
    ARIB_STD_B67,
    /// Versatile Log (FFmpeg extension, value 256).
    VLog,
}

/// The color space/matrix coefficients of the video.
///
/// Numbering matches `AVColorSpace`/FFMS2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// RGB (no matrix).
    RGB,
    /// ITU-R BT.709.
    BT709,
    /// Color space is unspecified.
    Unspecified,
    /// Reserved value 3.
    Reserved,
    /// FCC Title 47 CFR 73.682 (a)(10).
    FCC,
    /// ITU-R BT.470 System B/G (same as SMPTE 170M).
    BT470BG,
    /// SMPTE 170M (BT.601 NTSC).
    SMPTE170M,
    /// SMPTE 240M.
    SMPTE240M,
    /// `YCgCo`.
    YCGCO,
    /// ITU-R BT.2020 non-constant luminance.
    BT2020NCL,
    /// ITU-R BT.2020 constant luminance.
    BT2020CL,
    /// SMPTE ST 2085.
    SMPTE2085,
    /// Chroma-derived non-constant luminance.
    ChromaDerivedNCL,
    /// Chroma-derived constant luminance.
    ChromaDerivedCL,
    /// `ICtCp`.
    ICTCP,
    /// SMPTE ST 2128 IPT-C2.
    IPTC2,
    /// `YCgCo-R`, even addition of bits.
    YCGCORE,
    /// `YCgCo-R`, odd addition of bits.
    YCGCORO,
}

/// The chroma sample location of the video.
///
/// Numbering matches `AVChromaLocation`/FFMS2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChromaLocation {
    /// Location is unspecified.
    Unspecified,
    /// Left (MPEG-2 style, chroma at even luma positions).
    Left,
    /// Center (MPEG-1 style).
    Center,
    /// Top-left.
    TopLeft,
    /// Top.
    Top,
    /// Bottom-left.
    BottomLeft,
    /// Bottom.
    Bottom,
}

/// CIE 1931 xy chromaticity coordinates for the red, green, and blue
/// primaries plus the white point.
pub type PrimariesXY = ([(f64, f64); 3], (f64, f64));

/// SMPTE ST 2086 mastering display metadata.
///
/// Values are only present when the source flags them as such.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MasteringDisplayMetadata {
    /// CIE 1931 xy chromaticity coordinates of the red, green, and blue
    /// primaries plus the white point, present only if flagged by the source.
    pub primaries: Option<PrimariesXY>,
    /// Minimum and maximum display luminance in cd/m²,
    /// present only if flagged by the source.
    pub luminance: Option<(f64, f64)>,
}

/// CTA-861.3 content light level information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentLightLevel {
    /// Maximum content light level (`MaxCLL`) in cd/m².
    pub max_cll: u32,
    /// Maximum frame-average light level (`MaxFALL`) in cd/m².
    pub max_fall: u32,
}

macro_rules! raw_color_enum {
    ($ty:ty, $($variant:ident = $value:expr),+ $(,)?) => {
        impl $ty {
            /// Converts a raw `FFmpeg`/`FFMS2` integer value into this enum.
            ///
            /// Returns `None` for values not known to this library
            /// (e.g. values introduced by a newer `FFmpeg`).
            #[inline]
            #[must_use]
            pub const fn from_raw(value: i32) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

raw_color_enum!(ColorRange, Unspecified = 0, Limited = 1, Full = 2,);

raw_color_enum!(
    ColorPrimaries,
    Reserved0 = 0,
    BT709 = 1,
    Unspecified = 2,
    Reserved = 3,
    BT470M = 4,
    BT470BG = 5,
    SMPTE170M = 6,
    SMPTE240M = 7,
    Film = 8,
    BT2020 = 9,
    SMPTE428 = 10,
    SMPTE431 = 11,
    SMPTE432 = 12,
    EBU3213 = 22,
    VGamut = 256,
);

raw_color_enum!(
    TransferCharacteristic,
    Reserved0 = 0,
    BT709 = 1,
    Unspecified = 2,
    Reserved = 3,
    GAMMA22 = 4,
    GAMMA28 = 5,
    SMPTE170M = 6,
    SMPTE240M = 7,
    Linear = 8,
    Log = 9,
    LogSqrt = 10,
    IEC61966_2_4 = 11,
    BT1361_ECG = 12,
    IEC61966_2_1 = 13,
    BT2020_10 = 14,
    BT2020_12 = 15,
    SMPTE2084 = 16,
    SMPTE428 = 17,
    ARIB_STD_B67 = 18,
    VLog = 256,
);

raw_color_enum!(
    ColorSpace,
    RGB = 0,
    BT709 = 1,
    Unspecified = 2,
    Reserved = 3,
    FCC = 4,
    BT470BG = 5,
    SMPTE170M = 6,
    SMPTE240M = 7,
    YCGCO = 8,
    BT2020NCL = 9,
    BT2020CL = 10,
    SMPTE2085 = 11,
    ChromaDerivedNCL = 12,
    ChromaDerivedCL = 13,
    ICTCP = 14,
    IPTC2 = 15,
    YCGCORE = 16,
    YCGCORO = 17,
);

raw_color_enum!(
    ChromaLocation,
    Unspecified = 0,
    Left = 1,
    Center = 2,
    TopLeft = 3,
    Top = 4,
    BottomLeft = 5,
    Bottom = 6,
);

#[cfg(feature = "ffmpeg")]
mod ffmpeg_convert {
    use super::{ChromaLocation, ColorPrimaries, ColorRange, ColorSpace, TransferCharacteristic};

    // The FFmpeg wrapper enums are version-gated (their variant sets depend on the
    // linked FFmpeg), so converting through the raw libavutil value keeps this
    // crate compiling against any supported FFmpeg version.
    impl From<ffmpeg_the_third::color::Range> for ColorRange {
        #[inline]
        fn from(value: ffmpeg_the_third::color::Range) -> Self {
            let raw: ffmpeg_the_third::ffi::AVColorRange = value.into();
            Self::from_raw(raw.0).unwrap_or(Self::Unspecified)
        }
    }

    impl From<ffmpeg_the_third::color::Primaries> for ColorPrimaries {
        #[inline]
        fn from(value: ffmpeg_the_third::color::Primaries) -> Self {
            let raw: ffmpeg_the_third::ffi::AVColorPrimaries = value.into();
            Self::from_raw(raw.0).unwrap_or(Self::Unspecified)
        }
    }

    impl From<ffmpeg_the_third::color::TransferCharacteristic> for TransferCharacteristic {
        #[inline]
        fn from(value: ffmpeg_the_third::color::TransferCharacteristic) -> Self {
            let raw: ffmpeg_the_third::ffi::AVColorTransferCharacteristic = value.into();
            Self::from_raw(raw.0).unwrap_or(Self::Unspecified)
        }
    }

    impl From<ffmpeg_the_third::color::Space> for ColorSpace {
        #[inline]
        fn from(value: ffmpeg_the_third::color::Space) -> Self {
            let raw: ffmpeg_the_third::ffi::AVColorSpace = value.into();
            Self::from_raw(raw.0).unwrap_or(Self::Unspecified)
        }
    }

    impl From<ffmpeg_the_third::chroma::Location> for ChromaLocation {
        #[inline]
        fn from(value: ffmpeg_the_third::chroma::Location) -> Self {
            let raw: ffmpeg_the_third::ffi::AVChromaLocation = value.into();
            Self::from_raw(raw.0).unwrap_or(Self::Unspecified)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_match_documented_numbering() {
        assert_eq!(ColorRange::from_raw(0), Some(ColorRange::Unspecified));
        assert_eq!(ColorRange::from_raw(1), Some(ColorRange::Limited));
        assert_eq!(ColorRange::from_raw(2), Some(ColorRange::Full));
        assert_eq!(ColorRange::from_raw(3), None);

        assert_eq!(ColorPrimaries::from_raw(1), Some(ColorPrimaries::BT709));
        assert_eq!(ColorPrimaries::from_raw(9), Some(ColorPrimaries::BT2020));
        assert_eq!(ColorPrimaries::from_raw(22), Some(ColorPrimaries::EBU3213));
        assert_eq!(ColorPrimaries::from_raw(256), Some(ColorPrimaries::VGamut));
        assert_eq!(ColorPrimaries::from_raw(23), None);

        assert_eq!(
            TransferCharacteristic::from_raw(16),
            Some(TransferCharacteristic::SMPTE2084)
        );
        assert_eq!(
            TransferCharacteristic::from_raw(18),
            Some(TransferCharacteristic::ARIB_STD_B67)
        );
        assert_eq!(
            TransferCharacteristic::from_raw(256),
            Some(TransferCharacteristic::VLog)
        );
        assert_eq!(TransferCharacteristic::from_raw(19), None);

        assert_eq!(ColorSpace::from_raw(9), Some(ColorSpace::BT2020NCL));
        assert_eq!(ColorSpace::from_raw(14), Some(ColorSpace::ICTCP));
        assert_eq!(ColorSpace::from_raw(15), Some(ColorSpace::IPTC2));
        assert_eq!(ColorSpace::from_raw(17), Some(ColorSpace::YCGCORO));
        assert_eq!(ColorSpace::from_raw(18), None);

        assert_eq!(
            ChromaLocation::from_raw(0),
            Some(ChromaLocation::Unspecified)
        );
        assert_eq!(ChromaLocation::from_raw(1), Some(ChromaLocation::Left));
        assert_eq!(ChromaLocation::from_raw(6), Some(ChromaLocation::Bottom));
        assert_eq!(ChromaLocation::from_raw(7), None);
    }

    #[test]
    fn negative_and_unknown_values_return_none() {
        assert_eq!(ColorRange::from_raw(-1), None);
        assert_eq!(ColorSpace::from_raw(-5), None);
        assert_eq!(TransferCharacteristic::from_raw(1000), None);
    }
}
