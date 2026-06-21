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
    if cfg.width == 0 || cfg.height == 0 || cfg.bit_depth == 0 {
        return Err(DecoderError::GenericDecodeError {
            cause: "Zero resolution is not supported".to_string(),
        });
    }

    let chroma_sampling = if luma_only {
        ChromaSubsampling::Monochrome
    } else {
        cfg.chroma_sampling
    };

    FrameBuilder::new(cfg.width, cfg.height, chroma_sampling, cfg.bit_depth as u8)
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
                assert_eq!(cause, "Zero resolution is not supported");
            }
            Err(err) => panic!("unexpected error: {err}"),
            Ok(_) => panic!("zero width should fail"),
        }
    }
}
