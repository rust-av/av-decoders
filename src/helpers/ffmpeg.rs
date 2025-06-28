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
use v_frame::{
    frame::Frame,
    pixel::{ChromaSampling, Pixel},
};

use crate::{error::DecoderError, VideoDetails};

/// An interface that is used for decoding a video stream using ffmpeg
///
/// There have been desync issue reported with this decoder
/// on some video files. Use at your own risk!
pub struct FfmpegDecoder {
    input_ctx: context::Input,
    decoder: decoder::Video,
    pub(crate) video_details: VideoDetails,
    frameno: usize,
    stream_index: usize,
    end_of_stream: bool,
    eof_sent: bool,
}

impl FfmpegDecoder {
    /// Creates a new FFmpeg decoder for the specified video file.
    ///
    /// This function initializes the FFmpeg library, opens the input video file,
    /// finds the best video stream, and sets up a video decoder with frame-level
    /// threading for optimal performance.
    ///
    /// # Arguments
    ///
    /// * `input` - A path to the video file to decode. Can be any type that implements
    ///   `AsRef<Path>`, such as `&str`, `String`, `PathBuf`, or `&Path`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(FfmpegDecoder)` on success, containing a configured decoder ready
    /// to read video frames. The decoder will have its video details populated with
    /// information extracted from the input file.
    ///
    /// # Errors
    ///
    /// This function can return several types of errors:
    ///
    /// * `DecoderError::FfmpegInternalError` - If FFmpeg initialization fails or
    ///   if there's an internal FFmpeg error during codec context setup
    /// * `DecoderError::FileReadError` - If the input file cannot be opened or read
    /// * `DecoderError::NoVideoStream` - If no video stream is found in the input file
    /// * `DecoderError::UnsupportedFormat` - If the video format is not supported.
    ///   Currently supports YUV 4:2:0, 4:2:2, and 4:4:4 with 8, 10, or 12-bit depth
    ///
    /// # Supported Formats
    ///
    /// The decoder currently supports the following pixel formats:
    /// - YUV420P, YUV422P, YUV444P (8-bit)
    /// - YUVJ420P, YUVJ422P, YUVJ444P (8-bit JPEG colorspace)
    /// - YUV420P10LE, YUV422P10LE, YUV444P10LE (10-bit)
    /// - YUV420P12LE, YUV422P12LE, YUV444P12LE (12-bit)
    ///
    /// # Threading
    ///
    /// The decoder is configured with frame-level threading for improved performance
    /// on multi-core systems.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use av_decoders::FfmpegDecoder;
    ///
    /// // Create decoder from string path
    /// let decoder = FfmpegDecoder::new("input.mp4")?;
    ///
    /// // Create decoder from PathBuf
    /// use std::path::PathBuf;
    /// let path = PathBuf::from("input.mkv");
    /// let decoder = FfmpegDecoder::new(&path)?;
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// # Warning
    ///
    /// There have been desync issues reported with this decoder on some video files.
    /// Use at your own risk for critical applications.
    #[inline]
    pub fn new<P: AsRef<Path>>(input: P) -> Result<Self, DecoderError> {
        ffmpeg::init().map_err(|e| DecoderError::FfmpegInternalError {
            cause: e.to_string(),
        })?;

        let input_ctx = format::input(&input).map_err(|e| DecoderError::FileReadError {
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
                    | format::pixel::Pixel::YUV420P12LE => ChromaSampling::Cs420,
                    format::pixel::Pixel::YUV422P
                    | format::pixel::Pixel::YUVJ422P
                    | format::pixel::Pixel::YUV422P10LE
                    | format::pixel::Pixel::YUV422P12LE => ChromaSampling::Cs422,
                    format::pixel::Pixel::YUV444P
                    | format::pixel::Pixel::YUVJ444P
                    | format::pixel::Pixel::YUV444P10LE
                    | format::pixel::Pixel::YUV444P12LE => ChromaSampling::Cs444,
                    fmt => {
                        return Err(DecoderError::UnsupportedFormat {
                            fmt: format!("{fmt:?}"),
                        });
                    }
                },
                frame_rate: Rational32::new(frame_rate.numerator(), frame_rate.denominator()),
            },
            decoder,
            input_ctx,
            frameno: 0,
            stream_index,
            end_of_stream: false,
            eof_sent: false,
        })
    }

    fn decode_frame<T: Pixel>(&self, decoded: &frame::Video) -> Frame<T> {
        const SB_SIZE_LOG2: usize = 6;
        const SB_SIZE: usize = 1 << SB_SIZE_LOG2;
        const SUBPEL_FILTER_SIZE: usize = 8;
        const FRAME_MARGIN: usize = 16 + SUBPEL_FILTER_SIZE;
        const LUMA_PADDING: usize = SB_SIZE + FRAME_MARGIN;

        let mut f: Frame<T> = Frame::new_with_padding(
            self.video_details.width,
            self.video_details.height,
            self.video_details.chroma_sampling,
            LUMA_PADDING,
        );
        let width = self.video_details.width;
        let height = self.video_details.height;
        let bit_depth = self.video_details.bit_depth;
        let bytes = if bit_depth > 8 { 2 } else { 1 };
        let (chroma_width, _) = self
            .video_details
            .chroma_sampling
            .get_chroma_dimensions(width, height);
        f.planes[0].copy_from_raw_u8(decoded.data(0), width * bytes, bytes);
        f.planes[1].copy_from_raw_u8(decoded.data(1), chroma_width * bytes, bytes);
        f.planes[2].copy_from_raw_u8(decoded.data(2), chroma_width * bytes, bytes);
        f
    }

    pub(crate) fn read_video_frame<T: Pixel>(&mut self) -> Result<Frame<T>, DecoderError> {
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
                packet.set_pts(Some(self.frameno as i64));
                packet.set_dts(Some(self.frameno as i64));

                if !self.end_of_stream {
                    let _ = self.decoder.send_packet(&packet);
                }

                if self.decoder.receive_frame(&mut decoded).is_ok() {
                    let f = self.decode_frame(&decoded);
                    self.frameno += 1;
                    return Ok(f);
                } else if self.end_of_stream {
                    return Err(DecoderError::EndOfFile);
                }
            }
        }
    }
}
