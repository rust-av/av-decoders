extern crate ffmpeg_the_third as ffmpeg;

use std::path::Path;

use ffmpeg::{
    codec::{decoder, packet},
    format,
    format::context,
    frame,
    media::Type,
};
use ffmpeg_the_third::threading;
use num_rational::Rational32;
use v_frame::{chroma::ChromaSubsampling, frame::Frame, pixel::Pixel};

use crate::color::{
    ChromaLocation, ColorPrimaries, ColorRange, ColorSpace, ContentLightLevel,
    MasteringDisplayMetadata, TransferCharacteristic,
};
use crate::{VideoDetails, error::DecoderError};

use super::frame_builder::new_padded_frame;

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

        let parameters = input.parameters();
        let color_range: ColorRange = parameters.color_range().into();
        let color_primaries: ColorPrimaries = parameters.color_primaries().into();
        let color_trc: TransferCharacteristic = parameters.color_transfer_characteristic().into();
        let color_space: ColorSpace = parameters.color_space().into();
        let chroma_location: ChromaLocation = parameters.chroma_location().into();
        let (mastering_display, content_light_level) = parse_hdr_side_data(&parameters);

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
                color_range: Some(color_range),
                color_primaries: Some(color_primaries),
                color_transfer_characteristic: Some(color_trc),
                color_space: Some(color_space),
                chroma_location: Some(chroma_location),
                mastering_display,
                content_light_level,
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
        let mut frame: Frame<T> = new_padded_frame(&self.video_details, luma_only)?;

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

/// Mirror of FFmpeg's `AVMasteringDisplayMetadata` for reading raw side data.
///
/// The FFmpeg sys bindings do not expose this struct, so it is re-declared here
/// with the exact upstream layout and validated by size before each read.
#[repr(C)]
struct RawMasteringDisplayMetadata {
    display_primaries: [[ffmpeg::ffi::AVRational; 2]; 3],
    white_point: [ffmpeg::ffi::AVRational; 2],
    min_luminance: ffmpeg::ffi::AVRational,
    max_luminance: ffmpeg::ffi::AVRational,
    has_primaries: std::ffi::c_int,
    has_luminance: std::ffi::c_int,
}

/// Mirror of FFmpeg's `AVContentLightMetadata` for reading raw side data.
#[repr(C)]
struct RawContentLightMetadata {
    max_cll: std::ffi::c_uint,
    max_fall: std::ffi::c_uint,
}

fn rational_to_f64(value: ffmpeg::ffi::AVRational) -> f64 {
    if value.den == 0 {
        0.0
    } else {
        f64::from(value.num) / f64::from(value.den)
    }
}

/// Extracts HDR metadata from the stream's coded side data, if present.
fn parse_hdr_side_data(
    parameters: &ffmpeg::codec::ParametersRef<'_>,
) -> (Option<MasteringDisplayMetadata>, Option<ContentLightLevel>) {
    let mut mastering_display = None;
    let mut content_light_level = None;

    // SAFETY: `parameters.as_ptr()` is a valid `AVCodecParameters` pointer owned
    // by the input context, and the side-data array is valid for `nb_coded_side_data`
    // entries for as long as the context is alive.
    let side_data = unsafe {
        let ptr = (*parameters.as_ptr()).coded_side_data;
        let count = (*parameters.as_ptr()).nb_coded_side_data;
        if ptr.is_null() || count <= 0 {
            return (mastering_display, content_light_level);
        }
        std::slice::from_raw_parts(ptr, count as usize)
    };

    for entry in side_data {
        // SAFETY: `entry.data` was checked for null and `entry.size` is the
        // buffer length as reported by FFmpeg for this side-data entry.
        let data = unsafe {
            if entry.data.is_null() {
                continue;
            }
            std::slice::from_raw_parts(entry.data, entry.size)
        };
        match packet::side_data::Type::from(entry.type_) {
            packet::side_data::Type::MasteringDisplayMetadata
                if data.len() >= std::mem::size_of::<RawMasteringDisplayMetadata>() =>
            {
                // SAFETY: the buffer length was checked against the struct size above;
                // `read_unaligned` handles any alignment.
                let raw = unsafe {
                    std::ptr::read_unaligned(data.as_ptr() as *const RawMasteringDisplayMetadata)
                };
                mastering_display = Some(MasteringDisplayMetadata {
                    primaries: (raw.has_primaries != 0).then(|| {
                        let primaries = raw.display_primaries.map(|primary| {
                            (rational_to_f64(primary[0]), rational_to_f64(primary[1]))
                        });
                        let white_point = (
                            rational_to_f64(raw.white_point[0]),
                            rational_to_f64(raw.white_point[1]),
                        );
                        (primaries, white_point)
                    }),
                    luminance: (raw.has_luminance != 0).then(|| {
                        (
                            rational_to_f64(raw.min_luminance),
                            rational_to_f64(raw.max_luminance),
                        )
                    }),
                });
            }
            packet::side_data::Type::ContentLightLevel
                if data.len() >= std::mem::size_of::<RawContentLightMetadata>() =>
            {
                // SAFETY: the buffer length was checked against the struct size above;
                // `read_unaligned` handles any alignment.
                let raw = unsafe {
                    std::ptr::read_unaligned(data.as_ptr() as *const RawContentLightMetadata)
                };
                content_light_level = Some(ContentLightLevel {
                    max_cll: raw.max_cll,
                    max_fall: raw.max_fall,
                });
            }
            _ => {}
        }
    }

    (mastering_display, content_light_level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mastering_display_metadata_bytes() {
        // AVMasteringDisplayMetadata with primaries and luminance set
        let mut bytes = Vec::new();
        for value in [
            (17i32, 25i32), // r x
            (8, 25),        // r y
            (7, 25),        // g x
            (1, 7),         // g y
            (3, 25),        // b x
            (1, 15),        // b y
            (1, 5),         // white x
            (1, 3),         // white y
            (1, 10000),     // min luminance
            (1000, 1),      // max luminance
        ] {
            bytes.extend_from_slice(&value.0.to_le_bytes());
            bytes.extend_from_slice(&value.1.to_le_bytes());
        }
        bytes.extend_from_slice(&1i32.to_le_bytes()); // has_primaries
        bytes.extend_from_slice(&1i32.to_le_bytes()); // has_luminance

        // SAFETY: `bytes` holds exactly the layout of `RawMasteringDisplayMetadata`
        // as constructed above; `read_unaligned` handles any alignment.
        let raw = unsafe {
            std::ptr::read_unaligned(bytes.as_ptr() as *const RawMasteringDisplayMetadata)
        };
        let metadata = MasteringDisplayMetadata {
            primaries: (raw.has_primaries != 0).then(|| {
                let primaries = raw
                    .display_primaries
                    .map(|primary| (rational_to_f64(primary[0]), rational_to_f64(primary[1])));
                let white_point = (
                    rational_to_f64(raw.white_point[0]),
                    rational_to_f64(raw.white_point[1]),
                );
                (primaries, white_point)
            }),
            luminance: (raw.has_luminance != 0).then(|| {
                (
                    rational_to_f64(raw.min_luminance),
                    rational_to_f64(raw.max_luminance),
                )
            }),
        };

        let (primaries, white_point) = metadata.primaries.expect("primaries flagged");
        assert_eq!(primaries[0], (0.68, 0.32));
        assert_eq!(primaries[1], (0.28, 0.14285714285714285));
        assert_eq!(primaries[2], (0.12, 0.06666666666666667));
        assert_eq!(white_point, (0.2, 0.3333333333333333));
        assert_eq!(metadata.luminance, Some((0.0001, 1000.0)));
    }

    #[test]
    fn parses_content_light_level_bytes() {
        let bytes = [1000u32.to_le_bytes(), 400u32.to_le_bytes()].concat();

        // SAFETY: `bytes` holds exactly the layout of `RawContentLightMetadata`
        // as constructed above; `read_unaligned` handles any alignment.
        let raw =
            unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const RawContentLightMetadata) };
        assert_eq!(raw.max_cll, 1000);
        assert_eq!(raw.max_fall, 400);
    }

    #[test]
    fn mirror_structs_match_ffmpeg_layout() {
        // The mastering display struct is 10 rationals (6 primaries + 2 white
        // point + 2 luminance) plus two c_int flags.
        assert_eq!(
            std::mem::size_of::<RawMasteringDisplayMetadata>(),
            10 * std::mem::size_of::<ffmpeg::ffi::AVRational>()
                + 2 * std::mem::size_of::<std::ffi::c_int>()
        );
        assert_eq!(
            std::mem::size_of::<RawContentLightMetadata>(),
            2 * std::mem::size_of::<std::ffi::c_uint>()
        );
    }
}
