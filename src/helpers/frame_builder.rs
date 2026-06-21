use std::num::{NonZeroU8, NonZeroUsize};

use v_frame::{
    chroma::ChromaSubsampling,
    frame::{Frame, FrameBuilder},
    pixel::Pixel,
};

use crate::{DecoderError, LUMA_PADDING, VideoDetails};

pub(super) fn new_padded_frame<T: Pixel>(
    cfg: &VideoDetails,
    luma_only: bool,
) -> Result<Frame<T>, DecoderError> {
    let chroma_sampling = if luma_only {
        ChromaSubsampling::Monochrome
    } else {
        cfg.chroma_sampling
    };

    FrameBuilder::new(
        NonZeroUsize::new(cfg.width).ok_or_else(|| DecoderError::GenericDecodeError {
            cause: "Zero-width resolution is not supported".to_string(),
        })?,
        NonZeroUsize::new(cfg.height).ok_or_else(|| DecoderError::GenericDecodeError {
            cause: "Zero-height resolution is not supported".to_string(),
        })?,
        chroma_sampling,
        NonZeroU8::new(cfg.bit_depth as u8).ok_or_else(|| DecoderError::GenericDecodeError {
            cause: "Zero-bit-depth is not supported".to_string(),
        })?,
    )
    .luma_padding_bottom(LUMA_PADDING)
    .luma_padding_top(LUMA_PADDING)
    .luma_padding_left(LUMA_PADDING)
    .luma_padding_right(LUMA_PADDING)
    .build()
    .map_err(|e| DecoderError::GenericDecodeError {
        cause: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_monochrome_chroma_when_luma_only() {
        let frame = match new_padded_frame::<u8>(&VideoDetails::default(), true) {
            Ok(frame) => frame,
            Err(err) => panic!("valid default details should build: {err}"),
        };

        assert!(frame.u_plane.is_none());
        assert!(frame.v_plane.is_none());
    }

    #[test]
    fn rejects_zero_width() {
        let cfg = VideoDetails {
            width: 0,
            ..VideoDetails::default()
        };

        match new_padded_frame::<u8>(&cfg, false) {
            Err(DecoderError::GenericDecodeError { cause }) => {
                assert_eq!(cause, "Zero-width resolution is not supported");
            }
            Err(err) => panic!("unexpected error: {err}"),
            Ok(_) => panic!("zero width should fail"),
        }
    }
}
