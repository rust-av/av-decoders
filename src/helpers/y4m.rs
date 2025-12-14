use std::{
    io::Read,
    num::{NonZeroU8, NonZeroUsize},
};

use crate::error::DecoderError;
use crate::{LUMA_PADDING, VideoDetails};
use num_rational::Rational32;
use v_frame::{
    chroma::ChromaSubsampling,
    frame::{Frame, FrameBuilder},
    pixel::Pixel,
};

pub fn get_video_details<R: Read>(dec: &y4m::Decoder<R>) -> VideoDetails {
    let width = dec.get_width();
    let height = dec.get_height();
    let color_space = dec.get_colorspace();
    let bit_depth = color_space.get_bit_depth();
    let chroma_sampling = map_y4m_color_space(color_space);
    let framerate = dec.get_framerate();
    let frame_rate = Rational32::new(framerate.num as i32, framerate.den as i32);

    VideoDetails {
        width,
        height,
        bit_depth,
        chroma_sampling,
        frame_rate,
        total_frames: None,
    }
}

const fn map_y4m_color_space(color_space: y4m::Colorspace) -> ChromaSubsampling {
    use y4m::Colorspace::{
        C420, C420jpeg, C420mpeg2, C420p10, C420p12, C420paldv, C422, C422p10, C422p12, C444,
        C444p10, C444p12, Cmono, Cmono12,
    };
    match color_space {
        Cmono | Cmono12 => ChromaSubsampling::Monochrome,
        C420jpeg | C420paldv | C420mpeg2 | C420 | C420p10 | C420p12 => ChromaSubsampling::Yuv420,
        C422 | C422p10 | C422p12 => ChromaSubsampling::Yuv422,
        C444 | C444p10 | C444p12 => ChromaSubsampling::Yuv444,
        _ => unimplemented!(),
    }
}

pub fn read_video_frame<R: Read, T: Pixel>(
    dec: &mut y4m::Decoder<R>,
    cfg: &VideoDetails,
    luma_only: bool,
) -> Result<Frame<T>, DecoderError> {
    let dec_frame = dec.read_frame().map_err(|e| match e {
        y4m::Error::EOF => DecoderError::EndOfFile,
        _ => DecoderError::GenericDecodeError {
            cause: e.to_string(),
        },
    })?;

    let mut frame: Frame<T> = FrameBuilder::new(
        NonZeroUsize::new(cfg.width).ok_or_else(|| DecoderError::GenericDecodeError {
            cause: "Zero-width resolution is not supported".to_string(),
        })?,
        NonZeroUsize::new(cfg.height).ok_or_else(|| DecoderError::GenericDecodeError {
            cause: "Zero-height resolution is not supported".to_string(),
        })?,
        if luma_only {
            ChromaSubsampling::Monochrome
        } else {
            cfg.chroma_sampling
        },
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
    })?;

    frame
        .y_plane
        .copy_from_u8_slice(dec_frame.get_y_plane())
        .map_err(|e| DecoderError::GenericDecodeError {
            cause: e.to_string(),
        })?;
    if let Some(u_plane) = frame.u_plane.as_mut() {
        u_plane
            .copy_from_u8_slice(dec_frame.get_u_plane())
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
    }
    if let Some(v_plane) = frame.v_plane.as_mut() {
        v_plane
            .copy_from_u8_slice(dec_frame.get_v_plane())
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
    }

    Ok(frame)
}
