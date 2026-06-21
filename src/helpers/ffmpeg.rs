extern crate ffmpeg_the_third as ffmpeg;

use std::{
    num::{NonZeroU8, NonZeroUsize},
    path::Path,
};

use ffmpeg::{
    codec::{decoder, packet},
    format,
    format::context,
    frame,
    media::Type,
};
use ffmpeg_the_third::threading;
use num_rational::Rational32;
use v_frame::{
    chroma::ChromaSubsampling,
    frame::{Frame, FrameBuilder},
    pixel::Pixel,
};

use crate::{LUMA_PADDING, VideoDetails, error::DecoderError};

/// An interface that is used for decoding a video stream using ffmpeg
///
/// There have been desync issue reported with this decoder
/// on some video files. Use at your own risk!
pub struct FfmpegDecoder {
    #[allow(missing_docs)]
    pub input_ctx: context::Input,
    #[allow(missing_docs)]
    pub decoder: decoder::Video,
    #[allow(missing_docs)]
    pub video_details: VideoDetails,
    stream_index: usize,
    end_of_stream: bool,
    eof_sent: bool,
}

impl FfmpegDecoder {
    /// Creates a new FFmpeg decoder for the specified video file.
    ///
    /// Opens the file, selects the best video stream, and configures frame-threaded decoding.
    ///
    /// # Supported pixel formats
    ///
    /// YUV 4:2:0/4:2:2/4:4:4 at 8, 10, or 12-bit (including JPEG colorspace variants).
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::NoVideoStream`] if no video stream is found,
    /// [`DecoderError::UnsupportedFormat`] for unsupported pixel formats,
    /// [`DecoderError::FfmpegInternalError`] on codec setup failure.
    ///
    /// # Warning
    ///
    /// Desync issues have been reported with this decoder on some files. Use at your own risk.
    #[inline]
    pub fn new<P: AsRef<Path>>(input: P) -> Result<Self, DecoderError> {
        ffmpeg::init().map_err(|e| DecoderError::FfmpegInternalError {
            cause: e.to_string(),
        })?;

        let input_ctx = format::input(input.as_ref()).map_err(|e| DecoderError::FileReadError {
            cause: e.to_string(),
        })?;
        let input = input_ctx
            .streams()
            .best(Type::Video)
            .ok_or(DecoderError::NoVideoStream)?;
        let stream_index = input.index();
        let mut context = ffmpeg::codec::context::Context::from_parameters(input.parameters())
            .map_err(|e| DecoderError::FfmpegInternalError {
                cause: e.to_string(),
            })?;
        context.set_threading(threading::Config::kind(threading::Type::Frame));
        let mut decoder = context
            .decoder()
            .video()
            .map_err(|_| DecoderError::NoVideoStream)?;
        decoder.set_parameters(input.parameters()).map_err(|e| {
            DecoderError::FfmpegInternalError {
                cause: e.to_string(),
            }
        })?;

        let total_frames = input.frames();
        let frame_rate = input.rate();
        Ok(Self {
            video_details: VideoDetails {
                width: decoder.width() as usize,
                height: decoder.height() as usize,
                bit_depth: match decoder.format() {
                    format::pixel::Pixel::YUV420P
                    | format::pixel::Pixel::YUV422P
                    | format::pixel::Pixel::YUV444P
                    | format::pixel::Pixel::YUVJ420P
                    | format::pixel::Pixel::YUVJ422P
                    | format::pixel::Pixel::YUVJ444P => 8,
                    format::pixel::Pixel::YUV420P10LE
                    | format::pixel::Pixel::YUV422P10LE
                    | format::pixel::Pixel::YUV444P10LE => 10,
                    format::pixel::Pixel::YUV420P12LE
                    | format::pixel::Pixel::YUV422P12LE
                    | format::pixel::Pixel::YUV444P12LE => 12,
                    fmt => {
                        return Err(DecoderError::UnsupportedFormat {
                            fmt: format!("{fmt:?}"),
                        });
                    }
                },
                chroma_sampling: match decoder.format() {
                    format::pixel::Pixel::YUV420P
                    | format::pixel::Pixel::YUVJ420P
                    | format::pixel::Pixel::YUV420P10LE
                    | format::pixel::Pixel::YUV420P12LE => ChromaSubsampling::Yuv420,
                    format::pixel::Pixel::YUV422P
                    | format::pixel::Pixel::YUVJ422P
                    | format::pixel::Pixel::YUV422P10LE
                    | format::pixel::Pixel::YUV422P12LE => ChromaSubsampling::Yuv422,
                    format::pixel::Pixel::YUV444P
                    | format::pixel::Pixel::YUVJ444P
                    | format::pixel::Pixel::YUV444P10LE
                    | format::pixel::Pixel::YUV444P12LE => ChromaSubsampling::Yuv444,
                    fmt => {
                        return Err(DecoderError::UnsupportedFormat {
                            fmt: format!("{fmt:?}"),
                        });
                    }
                },
                frame_rate: Rational32::new(frame_rate.numerator(), frame_rate.denominator()),
                total_frames: total_frames.try_into().ok(),
            },
            decoder,
            input_ctx,
            stream_index,
            end_of_stream: false,
            eof_sent: false,
        })
    }

    fn decode_frame<T: Pixel>(
        &self,
        decoded: &frame::Video,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        let width = self.video_details.width;
        let height = self.video_details.height;
        let bit_depth = self.video_details.bit_depth;
        let chroma_sampling = self.video_details.chroma_sampling;
        let mut frame: Frame<T> = FrameBuilder::new(
            NonZeroUsize::new(width).ok_or_else(|| DecoderError::GenericDecodeError {
                cause: "Zero-width resolution is not supported".to_string(),
            })?,
            NonZeroUsize::new(height).ok_or_else(|| DecoderError::GenericDecodeError {
                cause: "Zero-height resolution is not supported".to_string(),
            })?,
            if luma_only {
                ChromaSubsampling::Monochrome
            } else {
                chroma_sampling
            },
            NonZeroU8::new(bit_depth as u8).ok_or_else(|| DecoderError::GenericDecodeError {
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
            .copy_from_u8_slice(decoded.data(0))
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
        if let Some(u_plane) = frame.u_plane.as_mut() {
            u_plane.copy_from_u8_slice(decoded.data(1)).map_err(|e| {
                DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                }
            })?;
        }
        if let Some(v_plane) = frame.v_plane.as_mut() {
            v_plane.copy_from_u8_slice(decoded.data(2)).map_err(|e| {
                DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                }
            })?;
        }

        Ok(frame)
    }

    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        frame_index: usize,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        // For some reason there's a crap ton of work needed to get ffmpeg to do
        // something simple, because each codec has it's own stupid way of doing
        // things and they don't all decode the same way.
        //
        // Maybe ffmpeg could have made a simple, singular interface that does this for
        // us, but noooooo.
        //
        // Reference: https://ffmpeg.org/doxygen/trunk/api-h264-test_8c_source.html#l00110
        loop {
            // This iterator is actually really stupid... it doesn't reset itself after each
            // `new`. But that solves our lifetime hell issues, ironically.
            let packet = self
                .input_ctx
                .packets()
                .next()
                .and_then(Result::ok)
                .map(|(_, packet)| packet);

            let mut packet = if let Some(packet) = packet {
                packet
            } else {
                self.end_of_stream = true;
                packet::Packet::empty()
            };

            if self.end_of_stream && !self.eof_sent {
                let _ = self.decoder.send_eof();
                self.eof_sent = true;
            }

            if self.end_of_stream || packet.stream() == self.stream_index {
                let mut decoded = frame::Video::new(
                    self.decoder.format(),
                    self.video_details.width as u32,
                    self.video_details.height as u32,
                );
                packet.set_pts(Some(frame_index as i64));
                packet.set_dts(Some(frame_index as i64));

                if !self.end_of_stream {
                    let _ = self.decoder.send_packet(&packet);
                }

                if self.decoder.receive_frame(&mut decoded).is_ok() {
                    let f = self.decode_frame(&decoded, luma_only);
                    return f;
                } else if self.end_of_stream {
                    return Err(DecoderError::EndOfFile);
                }
            }
        }
    }
}
