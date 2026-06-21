//! Ready-made decoders for the rust-av ecosystem.
//!
//! Each decoder outputs [`v_frame`] structs. Only the y4m decoder is enabled by default;
//! others require Cargo features for their external dependencies.
//!
//! # Backend priority
//!
//! 1. **Y4M** — `.y4m`/`.yuv` files (always available, lowest overhead)
//! 2. **FFMS2** — when the `ffms2` feature is enabled
//! 3. **FFmpeg** — when the `ffmpeg` feature is enabled
//! 4. **VapourSynth** — when the `vapoursynth` feature is enabled
//!
//! # Example
//!
//! ```no_run
//! use av_decoders::Decoder;
//!
//! let mut decoder = Decoder::from_file("video.y4m")?;
//! let details = decoder.get_video_details();
//! println!("{}x{} @ {} fps", details.width, details.height, details.frame_rate);
//!
//! while let Ok(frame) = decoder.read_video_frame::<u8>() {
//!     // process frame
//! }
//! # Ok::<(), av_decoders::DecoderError>(())
//! ```

#[cfg(feature = "vapoursynth")]
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, stdin};
use std::path::Path;
use v_frame::chroma::ChromaSubsampling;
use v_frame::frame::Frame;
use v_frame::pixel::Pixel;
#[cfg(feature = "vapoursynth")]
use vapoursynth::node::Node;
#[cfg(feature = "vapoursynth")]
use vapoursynth::prelude::Environment;

mod error;
mod helpers {
    #[cfg(feature = "ffmpeg")]
    pub(crate) mod ffmpeg;
    #[cfg(feature = "ffms2")]
    pub(crate) mod ffms2;
    mod frame_builder;
    #[cfg(feature = "vapoursynth")]
    pub(crate) mod vapoursynth;
    pub(crate) mod y4m;
}
mod util;

#[cfg(feature = "ffmpeg")]
pub use crate::helpers::ffmpeg::FfmpegDecoder;
#[cfg(feature = "ffms2")]
pub use crate::helpers::ffms2::Ffms2Decoder;
#[cfg(feature = "vapoursynth")]
pub use crate::helpers::vapoursynth::ModifyNode;
#[cfg(feature = "vapoursynth")]
pub use crate::helpers::vapoursynth::VapoursynthDecoder;
#[cfg(feature = "vapoursynth")]
use crate::helpers::vapoursynth::{VariableName, VariableValue};
pub use error::DecoderError;
pub use num_rational::Rational32;
pub use v_frame;
pub use y4m::Decoder as Y4mDecoder;

const Y4M_EXTENSIONS: &[&str] = &["y4m", "yuv"];

// TODO: Get rid of these and make padding an optional parameter
const SB_SIZE_LOG2: usize = 6;
const SB_SIZE: usize = 1 << SB_SIZE_LOG2;
const SUBPEL_FILTER_SIZE: usize = 8;
const FRAME_MARGIN: usize = 16 + SUBPEL_FILTER_SIZE;
const LUMA_PADDING: usize = SB_SIZE + FRAME_MARGIN;

/// Video metadata and configuration details, populated by every decoder on init.
#[derive(Debug, Clone, Copy)]
pub struct VideoDetails {
    /// The width of the video frame in pixels.
    pub width: usize,
    /// The height of the video frame in pixels.
    pub height: usize,
    /// Bits per color component (e.g. 8, 10, 12).
    pub bit_depth: usize,
    /// Chroma subsampling format.
    pub chroma_sampling: ChromaSubsampling,
    /// Frame rate as a rational number (frames per second).
    pub frame_rate: Rational32,
    /// Total number of frames, if known.
    pub total_frames: Option<usize>,
}

/// A set of possible configuration flags that are generic across all decoders.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecoderConfig {
    /// If `true`, the decoder will only fetch the luma planes from the video.
    pub luma_only: bool,
}

#[cfg(test)]
impl Default for VideoDetails {
    #[inline]
    fn default() -> Self {
        VideoDetails {
            width: 640,
            height: 480,
            bit_depth: 8,
            chroma_sampling: ChromaSubsampling::Yuv420,
            frame_rate: Rational32::new(30, 1),
            total_frames: None,
        }
    }
}

/// Unified video decoder that auto-selects the best available backend.
///
/// See the [crate-level example](self#example) for typical usage.
pub struct Decoder {
    decoder: DecoderImpl,
    video_details: VideoDetails,
    frames_read: usize,
    config: DecoderConfig,
}

impl Decoder {
    /// Creates a new decoder from a file path, auto-selecting the backend.
    ///
    /// Priority: Y4M → FFMS2 → FFmpeg → VapourSynth.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::FileReadError`] if the file cannot be opened,
    /// [`DecoderError::NoDecoder`] if no backend is available for the format.
    #[inline]
    #[expect(clippy::allow_attributes)]
    #[allow(
        unreachable_code,
        reason = "some branches are unreachable with some combinations of features"
    )]
    pub fn from_file<P: AsRef<Path>>(input: P) -> Result<Decoder, DecoderError> {
        // A raw y4m parser is going to be the fastest with the least overhead,
        // so we should use it if we have a y4m file.
        let ext = input
            .as_ref()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        if let Some(ext) = ext.as_deref() {
            if Y4M_EXTENSIONS.contains(&ext) {
                let reader =
                    BufReader::new(File::open(input).map_err(|e| DecoderError::FileReadError {
                        cause: e.to_string(),
                    })?);
                let decoder = DecoderImpl::Y4m(
                    y4m::decode(Box::new(reader) as Box<dyn Read>).map_err(|e| match e {
                        y4m::Error::EOF => DecoderError::EndOfFile,
                        _ => DecoderError::GenericDecodeError {
                            cause: e.to_string(),
                        },
                    })?,
                );
                let video_details = decoder.video_details()?;
                return Ok(Decoder {
                    decoder,
                    video_details,
                    frames_read: 0,
                    config: DecoderConfig::default(),
                });
            }

            #[cfg(feature = "vapoursynth")]
            if ext == "vpy" {
                // Decode vapoursynth script file input
                let decoder = DecoderImpl::Vapoursynth(VapoursynthDecoder::from_file(
                    input,
                    HashMap::new(),
                    None,
                )?);
                let video_details = decoder.video_details()?;
                return Ok(Decoder {
                    decoder,
                    video_details,
                    frames_read: 0,
                    config: DecoderConfig::default(),
                });
            }
        }

        // Ffms2 is the fastest and most reliable, use it if available.
        #[cfg(feature = "ffms2")]
        {
            let decoder = DecoderImpl::Ffms2(Ffms2Decoder::new(input, None)?);
            let video_details = decoder.video_details()?;
            return Ok(Decoder {
                decoder,
                video_details,
                frames_read: 0,
                config: DecoderConfig::default(),
            });
        }

        // Ffmpeg is considerably faster at decoding, so we should prefer it over Vapoursynth
        // for general use cases.
        #[cfg(feature = "ffmpeg")]
        {
            let decoder = DecoderImpl::Ffmpeg(FfmpegDecoder::new(input)?);
            let video_details = decoder.video_details()?;
            return Ok(Decoder {
                decoder,
                video_details,
                frames_read: 0,
                config: DecoderConfig::default(),
            });
        }

        #[cfg(feature = "vapoursynth")]
        {
            // Build a vapoursynth script and use that
            use crate::util::escape_python_string;

            let script = format!(
                r#"
import vapoursynth as vs
core = vs.core
clip = core.ffms2.Source("{}")
clip.set_output()
"#,
                escape_python_string(
                    &std::path::absolute(input)
                        .map_err(|e| DecoderError::FileReadError {
                            cause: e.to_string()
                        })?
                        .to_string_lossy()
                )
            );
            let decoder = DecoderImpl::Vapoursynth(VapoursynthDecoder::from_script(
                &script,
                HashMap::new(),
                None,
            )?);
            let video_details = decoder.video_details()?;
            return Ok(Decoder {
                decoder,
                video_details,
                frames_read: 0,
                config: DecoderConfig::default(),
            });
        }

        Err(DecoderError::NoDecoder)
    }

    /// Creates a new decoder from a VapourSynth script string.
    ///
    /// The script must produce a video clip via `clip.set_output()`.
    /// Pass `HashMap::new()` for `variables` if none are needed.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError`] if the script fails to execute, produces no valid
    /// output, or required VapourSynth plugins are unavailable.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn from_script(
        script: &str,
        variables: HashMap<VariableName, VariableValue>,
    ) -> Result<Decoder, DecoderError> {
        let dec = VapoursynthDecoder::from_script(script, variables, None)?;
        let decoder = DecoderImpl::Vapoursynth(dec);
        let video_details = decoder.video_details()?;
        Ok(Decoder {
            decoder,
            video_details,
            frames_read: 0,
            config: DecoderConfig::default(),
        })
    }

    /// Creates a decoder that reads Y4M data from stdin.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::EndOfFile`] if stdin is empty,
    /// [`DecoderError::GenericDecodeError`] if the Y4M header is invalid.
    #[inline]
    pub fn from_stdin() -> Result<Decoder, DecoderError> {
        // We can only support y4m for this
        let reader = BufReader::new(stdin());
        let decoder = DecoderImpl::Y4m(y4m::decode(Box::new(reader) as Box<dyn Read>).map_err(
            |e| match e {
                y4m::Error::EOF => DecoderError::EndOfFile,
                _ => DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                },
            },
        )?);
        let video_details: VideoDetails = decoder.video_details()?;
        Ok(Decoder {
            decoder,
            video_details,
            frames_read: 0,
            config: DecoderConfig::default(),
        })
    }

    /// Creates a decoder from a specific [`DecoderImpl`] variant, bypassing auto-detection.
    ///
    /// Prefer [`from_file`](Self::from_file), `from_script`, or
    /// [`from_stdin`](Self::from_stdin) unless you need direct backend control.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError`] if video metadata cannot be extracted from the implementation.
    #[inline]
    pub fn from_decoder_impl(decoder_impl: DecoderImpl) -> Result<Decoder, DecoderError> {
        let video_details = decoder_impl.video_details()?;
        Ok(Decoder {
            decoder: decoder_impl,
            video_details,
            frames_read: 0,
            config: DecoderConfig::default(),
        })
    }

    /// Returns the video metadata detected during initialization.
    #[inline]
    #[must_use]
    pub fn get_video_details(&self) -> &VideoDetails {
        &self.video_details
    }

    /// Sets the decoder to only fetch the luma planes from the video.
    /// This may improve performance for applications that do not need chroma data.
    #[inline]
    pub fn set_luma_only(&mut self, enabled: bool) {
        self.config.luma_only = enabled;
    }

    /// Decodes and returns the next video frame.
    ///
    /// `T` must match the video's bit depth: `u8` for 8-bit, `u16` for 10–16 bit.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::EndOfFile`] at end of stream,
    /// [`DecoderError::GenericDecodeError`] on corrupted data.
    ///
    /// Each frame contains uncompressed pixel data; avoid holding frames longer than needed.
    #[inline]
    pub fn read_video_frame<T: Pixel>(&mut self) -> Result<Frame<T>, DecoderError> {
        let result = self.decoder.read_video_frame(
            &self.video_details,
            #[cfg(any(feature = "ffmpeg", feature = "vapoursynth", feature = "ffms2"))]
            self.frames_read,
            self.config.luma_only,
        );
        if result.is_ok() {
            self.frames_read += 1;
        }
        result
    }

    /// Decodes and returns a specific frame by index.
    ///
    /// Not all backends support seeking. `T` must match the video's bit depth.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::UnsupportedDecoder`] if the backend cannot seek,
    /// [`DecoderError::EndOfFile`] past the last frame.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_video_frame<T: Pixel>(
        &mut self,
        frame_index: usize,
    ) -> Result<Frame<T>, DecoderError> {
        self.decoder.get_video_frame(
            #[cfg(feature = "vapoursynth")]
            &self.video_details,
            #[cfg(feature = "vapoursynth")]
            frame_index,
            self.config.luma_only,
        )
    }

    /// Seeks to the given frame index, skipping intermediate frames.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::UnsupportedDecoder`] if the backend does not support seeking,
    /// [`DecoderError::EndOfFile`] if the index is past the last frame.
    #[inline]
    #[cfg(any(feature = "vapoursynth", feature = "ffms2"))]
    pub fn seek_to_frame(&mut self, frame_index: usize) -> Result<(), DecoderError> {
        match &self.decoder {
            #[cfg(feature = "vapoursynth")]
            DecoderImpl::Vapoursynth(_) => {
                if self
                    .video_details
                    .total_frames
                    .is_some_and(|total_frames| frame_index >= total_frames)
                {
                    return Err(DecoderError::EndOfFile);
                }
                self.frames_read = frame_index;
                Ok(())
            }
            #[cfg(feature = "ffms2")]
            DecoderImpl::Ffms2(_) => {
                if self
                    .video_details
                    .total_frames
                    .is_some_and(|total_frames| frame_index >= total_frames)
                {
                    return Err(DecoderError::EndOfFile);
                }
                self.frames_read = frame_index;
                Ok(())
            }
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }

    /// Returns a mutable reference to the VapourSynth environment.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::UnsupportedDecoder`] if the active backend is not VapourSynth.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth_env(&mut self) -> Result<&mut Environment, DecoderError> {
        match self.decoder {
            DecoderImpl::Vapoursynth(ref mut dec) => Ok(dec.get_env()),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }

    /// Returns the VapourSynth output node for the decoded video stream.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::UnsupportedDecoder`] if the active backend is not VapourSynth.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth_node(&self) -> Result<Node<'_>, DecoderError> {
        match self.decoder {
            DecoderImpl::Vapoursynth(ref dec) => Ok(dec.get_output_node()),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }

    /// Returns both the VapourSynth environment and output node.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::UnsupportedDecoder`] if the active backend is not VapourSynth.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth(&self) -> Result<(&Environment, Node<'_>), DecoderError> {
        match self.decoder {
            DecoderImpl::Vapoursynth(ref dec) => Ok((dec.get_environment(), dec.get_output_node())),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }

    /// Returns a mutable reference to the underlying FFmpeg decoder, or `None` otherwise.
    #[inline]
    #[cfg(feature = "ffmpeg")]
    pub fn get_ffmpeg_impl(&mut self) -> Option<&mut FfmpegDecoder> {
        match &mut self.decoder {
            DecoderImpl::Ffmpeg(dec) => Some(dec),
            _ => None,
        }
    }

    /// Returns a mutable reference to the underlying FFMS2 decoder, or `None` otherwise.
    #[inline]
    #[cfg(feature = "ffms2")]
    pub fn get_ffms2_impl(&mut self) -> Option<&mut Ffms2Decoder> {
        match &mut self.decoder {
            DecoderImpl::Ffms2(dec) => Some(dec),
            _ => None,
        }
    }

    /// Returns a mutable reference to the underlying VapourSynth decoder, or `None` otherwise.
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth_impl(&mut self) -> Option<&mut VapoursynthDecoder> {
        match &mut self.decoder {
            DecoderImpl::Vapoursynth(dec) => Some(dec),
            _ => None,
        }
    }
}

/// Internal enum representing the active decoder backend.
///
/// The variant is selected automatically during [`Decoder`] initialization.
pub enum DecoderImpl {
    /// Y4M format parser (always available).
    Y4m(Y4mDecoder<Box<dyn Read>>),

    /// VapourSynth-based decoder (requires `vapoursynth` feature).
    #[cfg(feature = "vapoursynth")]
    Vapoursynth(VapoursynthDecoder),

    /// FFmpeg-based decoder (requires `ffmpeg` feature).
    #[cfg(feature = "ffmpeg")]
    Ffmpeg(FfmpegDecoder),

    /// FFMS2-based decoder (requires `ffms2` feature).
    #[cfg(feature = "ffms2")]
    Ffms2(Ffms2Decoder),
}

impl DecoderImpl {
    pub(crate) fn video_details(&self) -> Result<VideoDetails, DecoderError> {
        match self {
            Self::Y4m(dec) => Ok(helpers::y4m::get_video_details(dec)),
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.get_video_details(),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(dec) => Ok(dec.video_details),
            #[cfg(feature = "ffms2")]
            Self::Ffms2(dec) => Ok(dec.video_details),
        }
    }

    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
        #[cfg(any(feature = "ffmpeg", feature = "vapoursynth", feature = "ffms2"))]
        frame_index: usize,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        match self {
            Self::Y4m(dec) => {
                helpers::y4m::read_video_frame::<Box<dyn Read>, T>(dec, cfg, luma_only)
            }
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.read_video_frame::<T>(cfg, frame_index, luma_only),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(dec) => dec.read_video_frame::<T>(frame_index, luma_only),
            #[cfg(feature = "ffms2")]
            Self::Ffms2(dec) => dec.read_video_frame::<T>(frame_index, luma_only),
        }
    }

    #[cfg(feature = "vapoursynth")]
    pub(crate) fn get_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
        frame_index: usize,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        match self {
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.read_video_frame::<T>(cfg, frame_index, luma_only),
            #[cfg(feature = "ffms2")]
            Self::Ffms2(dec) => dec.read_video_frame::<T>(frame_index, luma_only),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }
}
