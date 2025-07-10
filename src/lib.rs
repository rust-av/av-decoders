//! This crate providers ready-made decoders for use with the rust-av ecosystem.
//! Each decoder will output structs from the `v_frame` crate.
//!
//! Only the y4m decoder is enabled by default.
//! Others must be enabled via Cargo features, since they require external dependencies.

#[cfg(feature = "vapoursynth")]
use std::collections::HashMap;
use std::fs::File;
use std::io::{stdin, BufReader, Read};
use std::path::Path;
use v_frame::frame::Frame;
use v_frame::pixel::{ChromaSampling, Pixel};
#[cfg(feature = "vapoursynth")]
use vapoursynth::node::Node;
#[cfg(feature = "vapoursynth")]
use vapoursynth::prelude::Environment;

mod error;
mod helpers {
    #[cfg(feature = "ffmpeg")]
    pub(crate) mod ffmpeg;
    #[cfg(feature = "vapoursynth")]
    pub(crate) mod vapoursynth;
    pub(crate) mod y4m;
}

#[cfg(feature = "ffmpeg")]
pub use crate::helpers::ffmpeg::FfmpegDecoder;
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

/// Video metadata and configuration details.
///
/// This struct contains essential information about a video stream that is needed
/// for proper decoding and frame processing. All decoders will populate this
/// information when initialized.
#[derive(Debug, Clone, Copy)]
pub struct VideoDetails {
    /// The width of the video frame in pixels.
    pub width: usize,
    /// The height of the video frame in pixels.
    pub height: usize,
    /// The bit depth per color component (e.g., 8 for 8-bit, 10 for 10-bit video).
    pub bit_depth: usize,
    /// The chroma subsampling format used by the video.
    ///
    /// Common values include:
    /// - `ChromaSampling::Cs420` for 4:2:0 subsampling (most common)
    /// - `ChromaSampling::Cs422` for 4:2:2 subsampling  
    /// - `ChromaSampling::Cs444` for 4:4:4 subsampling (no chroma subsampling)
    pub chroma_sampling: ChromaSampling,
    /// The frame rate of the video as a rational number (frames per second).
    ///
    /// Examples:
    /// - `Rational32::new(30, 1)` for 30 fps
    /// - `Rational32::new(24000, 1001)` for 23.976 fps (24000/1001)
    /// - `Rational32::new(25, 1)` for 25 fps
    pub frame_rate: Rational32,
    /// The total number of frames in the video, if known.
    pub total_frames: Option<usize>,
}

#[cfg(test)]
impl Default for VideoDetails {
    #[inline]
    fn default() -> Self {
        VideoDetails {
            width: 640,
            height: 480,
            bit_depth: 8,
            chroma_sampling: ChromaSampling::Cs420,
            frame_rate: Rational32::new(30, 1),
            total_frames: None,
        }
    }
}

/// A unified video decoder that can handle multiple video formats and sources.
///
/// The `Decoder` provides a consistent interface for decoding video frames from various
/// sources and formats. It automatically selects the most appropriate decoding backend
/// based on the input format and available features.
///
/// ## Supported Formats
///
/// - **Y4M files** (always available): Raw Y4M format files with `.y4m` or `.yuv` extensions
/// - **General video files** (requires `ffmpeg` feature): Most common video formats via FFmpeg
/// - **Advanced video processing** (requires `vapoursynth` feature): Enhanced format support via VapourSynth
///
/// ## Backend Priority
///
/// The decoder automatically selects backends in this order of preference:
/// 1. **Y4M parser** - Used for Y4M files (fastest, lowest overhead)
/// 2. **FFmpeg** - Used when available for faster decoding of a variety of formats
/// 3. **VapourSynth** - Used as fallback when VapourSynth not available
///
/// ## Examples
///
/// ```no_run
/// use av_decoders::Decoder;
///
/// // Decode from a file
/// let mut decoder = Decoder::from_file("video.y4m")?;
/// let details = decoder.get_video_details();
/// println!("Video: {}x{} @ {} fps", details.width, details.height, details.frame_rate);
///
/// // Read frames
/// while let Ok(frame) = decoder.read_video_frame::<u8>() {
///     // Process the frame...
/// }
///
/// // Decode from stdin
/// let mut stdin_decoder = Decoder::from_stdin()?;
/// let frame = stdin_decoder.read_video_frame::<u8>()?;
/// # Ok::<(), av_decoders::DecoderError>(())
/// ```
pub struct Decoder {
    decoder: DecoderImpl,
    video_details: VideoDetails,
}

impl Decoder {
    /// Creates a new decoder from a file path.
    ///
    /// This method automatically detects the input format and selects the most appropriate
    /// decoder backend. It will prioritize Y4M files for performance, then FFmpeg for speed,
    /// and finally Vapoursynth.
    ///
    /// # Arguments
    ///
    /// * `input` - A path to the video file. Can be any type that implements `AsRef<Path>`,
    ///   such as `&str`, `String`, `Path`, or `PathBuf`.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Decoder<BufReader<File>>)` - A successfully initialized decoder
    /// - `Err(DecoderError)` - An error if the file cannot be read or decoded
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The file cannot be opened or read (`DecoderError::FileReadError`)
    /// - No suitable decoder backend is available (`DecoderError::NoDecoder`)
    /// - The file format is not supported or corrupted (`DecoderError::GenericDecodeError`)
    /// - Required features are not enabled for the file format
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    /// use std::path::Path;
    ///
    /// // From string path
    /// let decoder = Decoder::from_file("video.y4m")?;
    ///
    /// // From Path
    /// let path = Path::new("video.mp4");
    /// let decoder = Decoder::from_file(path)?;
    ///
    /// // From PathBuf
    /// let pathbuf = std::env::current_dir()?.join("video.mkv");
    /// let decoder = Decoder::from_file(pathbuf)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[inline]
    #[allow(unreachable_code)]
    #[allow(clippy::needless_return)]
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
                });
            }

            #[cfg(feature = "vapoursynth")]
            if ext == "vpy" {
                // Decode vapoursynth script file input
                let decoder = DecoderImpl::Vapoursynth(VapoursynthDecoder::new(input)?);
                let video_details = decoder.video_details()?;
                return Ok(Decoder {
                    decoder,
                    video_details,
                });
            }
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
            });
        }

        #[cfg(feature = "vapoursynth")]
        {
            // Build a vapoursynth script and use that
            let script = format!(
                r#"
import vapoursynth as vs
core = vs.core
clip = core.ffms2.Source("{}")
clip.set_output()
"#,
                std::path::absolute(input)
                    .map_err(|e| DecoderError::FileReadError {
                        cause: e.to_string()
                    })?
                    .to_string_lossy()
                    .replace('"', "\\\"")
            );
            let decoder = DecoderImpl::Vapoursynth(VapoursynthDecoder::from_script(&script)?);
            let video_details = decoder.video_details()?;
            return Ok(Decoder {
                decoder,
                video_details,
            });
        }

        Err(DecoderError::NoDecoder)
    }

    /// Creates a new decoder from a VapourSynth script.
    ///
    /// This method allows you to create a decoder by providing a VapourSynth script directly
    /// as a string, rather than reading from a file. This is useful for dynamic video processing
    /// pipelines, custom filtering operations, or when you need to apply VapourSynth's advanced
    /// video processing capabilities programmatically.
    ///
    /// VapourSynth scripts can include complex video processing operations, filters, and
    /// transformations that are not available through simple file-based decoders. This method
    /// provides access to the full power of the VapourSynth ecosystem.
    ///
    /// # Requirements
    ///
    /// This function is only available when the `vapoursynth` feature is enabled.
    ///
    /// # Arguments
    ///
    /// * `script` - A VapourSynth script as a string. The script should define a video node
    ///   that will be used as the source for decoding. The script must be valid VapourSynth
    ///   Python code that produces a video clip.
    ///
    /// * `variables` - Optional script variables as key-value pairs. These will be passed
    ///   to the VapourSynth environment and can be accessed within the script using
    ///   `vs.get_output()` or similar mechanisms. Pass `None` if no variables are needed.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Decoder<BufReader<File>>)` - A successfully initialized decoder using the script
    /// - `Err(DecoderError)` - An error if the script cannot be executed or produces invalid output
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The VapourSynth script contains syntax errors (`DecoderError::GenericDecodeError`)
    /// - The script fails to execute or raises exceptions
    /// - The script does not produce a valid video output
    /// - Required VapourSynth plugins are not available
    /// - The VapourSynth environment cannot be initialized
    /// - The arguments cannot be set (`DecoderError::GenericDecodeError`)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    /// use std::collections::HashMap;
    ///
    /// // Simple script that loads a video file
    /// let script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    /// clip = core.ffms2.Source("input.mkv")
    /// clip.set_output()
    /// "#;
    ///
    /// let decoder = Decoder::from_script(script, None)?;
    /// let details = decoder.get_video_details();
    /// println!("Video: {}x{} @ {} fps", details.width, details.height, details.frame_rate);
    ///
    /// // Script with variables for dynamic processing
    /// let script_with_args = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// # Get variables passed from Rust
    /// filename = vs.get_output().get("filename", "default.mkv")
    /// resize_width = int(vs.get_output().get("width", "1920"))
    ///
    /// clip = core.ffms2.Source(filename)
    /// clip = core.resize.Bicubic(clip, width=resize_width, height=clip.height * resize_width // clip.width)
    /// clip.set_output()
    /// "#;
    ///
    /// let mut variables = HashMap::new();
    /// variables.insert("filename".to_string(), "video.mp4".to_string());
    /// variables.insert("width".to_string(), "1280".to_string());
    ///
    /// let mut decoder = Decoder::from_script(script_with_args, Some(variables))?;
    ///
    /// // Read frames from the processed video
    /// while let Ok(frame) = decoder.read_video_frame::<u8>() {
    ///     // Process the filtered frame...
    /// }
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// ## Advanced Usage
    ///
    /// VapourSynth scripts can include complex filtering pipelines:
    ///
    /// ```no_run
    /// # use av_decoders::Decoder;
    /// let advanced_script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// # Load source
    /// clip = core.ffms2.Source("input.mkv")
    ///
    /// # Apply denoising
    /// clip = core.bm3d.BM3D(clip, sigma=3.0)
    ///
    /// # Upscale using AI
    /// clip = core.waifu2x.Waifu2x(clip, noise=1, scale=2)
    ///
    /// # Color correction
    /// clip = core.std.Levels(clip, min_in=16, max_in=235, min_out=0, max_out=255)
    ///
    /// clip.set_output()
    /// "#;
    ///
    /// let decoder = Decoder::from_script(advanced_script, None)?;
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn from_script(
        script: &str,
        variables: Option<HashMap<VariableName, VariableValue>>,
    ) -> Result<Decoder, DecoderError> {
        let mut dec = VapoursynthDecoder::from_script(script)?;
        if let Some(variables_map) = variables {
            dec.set_variables(variables_map)?;
        }
        let decoder = DecoderImpl::Vapoursynth(dec);
        let video_details = decoder.video_details()?;
        Ok(Decoder {
            decoder,
            video_details,
        })
    }

    /// Creates a new decoder that reads from standard input (stdin).
    ///
    /// This method is useful for processing video data in pipelines or when the video
    /// data is being streamed. Currently, only Y4M format is supported for stdin input.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Decoder<BufReader<Stdin>>)` - A successfully initialized decoder reading from stdin
    /// - `Err(DecoderError)` - An error if stdin cannot be read or the data is not valid Y4M
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The input stream is not in Y4M format
    /// - The Y4M header is malformed or missing (`DecoderError::GenericDecodeError`)
    /// - End of file is reached immediately (`DecoderError::EndOfFile`)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    ///
    /// // Read Y4M data from stdin
    /// let mut decoder = Decoder::from_stdin()?;
    ///
    /// // Process frames as they arrive
    /// loop {
    ///     match decoder.read_video_frame::<u8>() {
    ///         Ok(frame) => {
    ///             // Process the frame
    ///             println!("Received frame: {}x{}", frame.planes[0].cfg.width, frame.planes[0].cfg.height);
    ///         }
    ///         Err(av_decoders::DecoderError::EndOfFile) => break,
    ///         Err(e) => return Err(e),
    ///     }
    /// }
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// ## Command Line Usage
    ///
    /// This is commonly used with command-line pipelines:
    /// ```bash
    /// # Pipe Y4M data to your application
    /// ffmpeg -i input.mp4 -f yuv4mpegpipe - | your_app
    ///
    /// # Or directly from Y4M files
    /// cat video.y4m | your_app
    /// ```
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
        let video_details = decoder.video_details()?;
        Ok(Decoder {
            decoder,
            video_details,
        })
    }

    /// Creates a new decoder from an existing decoder implementation.
    ///
    /// This method provides a way to construct a `Decoder` from a specific `DecoderImpl`
    /// variant when you need direct control over the decoder backend selection. This is
    /// typically used for advanced use cases where you want to bypass the automatic
    /// format detection and backend selection logic of the other constructor methods.
    ///
    /// The method will extract the video metadata from the provided decoder implementation
    /// and create a fully initialized `Decoder` instance ready for frame reading.
    ///
    /// # Arguments
    ///
    /// * `decoder_impl` - A specific decoder implementation variant (`DecoderImpl`).
    ///   This can be one of:
    ///   - `DecoderImpl::Y4m` for Y4M format decoding
    ///   - `DecoderImpl::Vapoursynth` for VapourSynth-based decoding (requires `vapoursynth` feature)
    ///   - `DecoderImpl::Ffmpeg` for FFmpeg-based decoding (requires `ffmpeg` feature)
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Decoder)` - A successfully initialized decoder using the provided implementation
    /// - `Err(DecoderError)` - An error if video details cannot be extracted from the implementation
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The decoder implementation is not properly initialized
    /// - Video metadata cannot be extracted from the implementation (`DecoderError::GenericDecodeError`)
    /// - The implementation is in an invalid state
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::{Decoder, DecoderImpl, Y4mDecoder};
    /// use std::fs::File;
    /// use std::io::{BufReader, Read};
    ///
    /// // Create a Y4M decoder implementation directly
    /// let file = File::open("video.y4m")?;
    /// let reader = BufReader::new(file);
    /// let y4m_decoder = Y4mDecoder::new(Box::new(reader) as Box<dyn Read>)?;
    /// let decoder_impl = DecoderImpl::Y4m(y4m_decoder);
    ///
    /// // Create a Decoder from the implementation
    /// let decoder = Decoder::from_decoder_impl(decoder_impl)?;
    /// let details = decoder.get_video_details();
    /// println!("Video: {}x{}", details.width, details.height);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// ## Use Cases
    ///
    /// This method is particularly useful when:
    /// - You need to pre-configure a specific decoder backend
    /// - You want to bypass automatic format detection
    /// - You're implementing custom decoder initialization logic
    /// - You need to reuse or transfer decoder implementations between contexts
    ///
    /// ## Note
    ///
    /// This is an advanced method that exposes internal decoder implementation details.
    /// In most cases, you should prefer using `from_file()`, `from_script()`, or
    /// `from_stdin()` which provide safer, higher-level interfaces with automatic
    /// format detection and backend selection.
    #[inline]
    pub fn from_decoder_impl(decoder_impl: DecoderImpl) -> Result<Decoder, DecoderError> {
        let video_details = decoder_impl.video_details()?;
        Ok(Decoder {
            decoder: decoder_impl,
            video_details,
        })
    }

    /// Returns the video metadata and configuration details.
    ///
    /// This method provides access to the essential video properties that were detected
    /// during decoder initialization. The returned reference is valid for the lifetime
    /// of the decoder and the values will not change during decoding.
    ///
    /// # Returns
    ///
    /// Returns a reference to `VideoDetails` containing:
    /// - `width` and `height` - Frame dimensions in pixels
    /// - `bit_depth` - Bits per color component (8, 10, 12, etc.)
    /// - `chroma_sampling` - Color subsampling format (4:2:0, 4:2:2, 4:4:4)
    /// - `frame_rate` - Frames per second as a rational number
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    /// use v_frame::pixel::ChromaSampling;
    ///
    /// let decoder = Decoder::from_file("video.y4m").unwrap();
    /// let details = decoder.get_video_details();
    ///
    /// println!("Resolution: {}x{}", details.width, details.height);
    /// println!("Bit depth: {} bits", details.bit_depth);
    /// println!("Frame rate: {} fps", details.frame_rate);
    ///
    /// match details.chroma_sampling {
    ///     ChromaSampling::Cs420 => println!("4:2:0 chroma subsampling"),
    ///     ChromaSampling::Cs422 => println!("4:2:2 chroma subsampling"),
    ///     ChromaSampling::Cs444 => println!("4:4:4 chroma subsampling"),
    ///     _ => println!("Other chroma subsampling"),
    /// }
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    #[inline]
    pub fn get_video_details(&self) -> &VideoDetails {
        &self.video_details
    }

    /// Reads and decodes the next video frame from the input.
    ///
    /// This method advances the decoder to the next frame and returns it as a `Frame<T>`
    /// where `T` is the pixel type. The pixel type must be compatible with the video's
    /// bit depth and the decoder backend being used.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The pixel type to use for the decoded frame. Must implement the `Pixel` trait.
    ///   Types include:
    ///   - `u8` for 8-bit video
    ///   - `u16` for 10-bit to 16-bit video
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Frame<T>)` - The decoded video frame
    /// - `Err(DecoderError)` - An error if the frame cannot be read or decoded
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - End of file/stream is reached (`DecoderError::EndOfFile`)
    /// - The frame data is corrupted or invalid (`DecoderError::GenericDecodeError`)
    /// - There's an I/O error reading the input (`DecoderError::FileReadError`)
    /// - The pixel type is incompatible with the video format
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    ///
    /// let mut decoder = Decoder::from_file("video.y4m").unwrap();
    /// let details = decoder.get_video_details();
    ///
    /// // Read video frames, dynamically detecting the pixel type
    /// if details.bit_depth > 8 {
    ///     while let Ok(frame) = decoder.read_video_frame::<u16>() {
    ///         println!("Frame size: {}x{}",
    ///             frame.planes[0].cfg.width,
    ///             frame.planes[0].cfg.height
    ///         );
    ///         // Process frame data...
    ///     }
    /// } else {
    ///     while let Ok(frame) = decoder.read_video_frame::<u8>() {
    ///         println!("Frame size: {}x{}",
    ///             frame.planes[0].cfg.width,
    ///             frame.planes[0].cfg.height
    ///         );
    ///         // Process frame data...
    ///     }
    /// }
    /// ```
    ///
    /// ## Performance Notes
    ///
    /// - Frames are decoded sequentially; seeking may not be supported by all backends
    /// - Each frame contains uncompressed pixel values, which results in heavy memory usage;
    ///   avoid keeping frames in memory for longer than needed
    #[inline]
    pub fn read_video_frame<T: Pixel>(&mut self) -> Result<Frame<T>, DecoderError> {
        self.decoder.read_video_frame(&self.video_details)
    }

    /// Reads and decodes the specified video frame from the input.
    ///
    /// This method decodes the specified frame and returns it as a `Frame<T>`
    /// where `T` is the pixel type. The pixel type must be compatible with the video's
    /// bit depth and the decoder backend being used.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The pixel type to use for the decoded frame. Must implement the `Pixel` trait.
    ///   Types include:
    ///   - `u8` for 8-bit video
    ///   - `u16` for 10-bit to 16-bit video
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Frame<T>)` - The decoded video frame
    /// - `Err(DecoderError)` - An error if the frame cannot be read or decoded
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - End of file/stream is reached (`DecoderError::EndOfFile`)
    /// - The frame data is corrupted or invalid (`DecoderError::GenericDecodeError`)
    /// - There's an I/O error reading the input (`DecoderError::FileReadError`)
    /// - The pixel type is incompatible with the video format
    /// - The decoder does not support seeking (`DecoderError::UnsupportedDecoder`)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    ///
    /// let script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// clip = core.ffms2.Source('input.mp4')
    /// clip.set_output()
    /// "#;
    ///
    /// let mut decoder = Decoder::from_script().unwrap();
    /// let details = decoder.get_video_details();
    ///
    /// // Seek the 42nd video frame, dynamically detecting the pixel type
    /// if details.bit_depth > 8 {
    ///     while let Ok(frame) = decoder.seek_video_frame::<u16>(42) {
    ///         println!("Frame size: {}x{}",
    ///             frame.planes[0].cfg.width,
    ///             frame.planes[0].cfg.height
    ///         );
    ///         // Process frame data...
    ///     }
    /// } else {
    ///     while let Ok(frame) = decoder.seek_video_frame::<u8>(42) {
    ///         println!("Frame size: {}x{}",
    ///             frame.planes[0].cfg.width,
    ///             frame.planes[0].cfg.height
    ///         );
    ///         // Process frame data...
    ///     }
    /// }
    /// ```
    ///
    /// ## Performance Notes
    ///
    /// - Frames are decoded sequentially; seeking may not be supported by all backends
    /// - Each frame contains uncompressed pixel values, which results in heavy memory usage;
    ///   avoid keeping frames in memory for longer than needed
    #[inline]
    pub fn seek_video_frame<T: Pixel>(
        &mut self,
        frame_index: usize,
    ) -> Result<Frame<T>, DecoderError> {
        self.decoder
            .seek_video_frame(&self.video_details, frame_index)
    }

    /// Returns a mutable reference to the VapourSynth environment.
    ///
    /// This method provides direct access to the VapourSynth environment when using
    /// a VapourSynth-based decoder. The environment can be used for advanced
    /// VapourSynth operations, plugin loading, or creating additional video nodes
    /// and filters programmatically.
    ///
    /// This is particularly useful when you need to:
    /// - Load additional VapourSynth plugins
    /// - Create custom filtering pipelines
    /// - Access VapourSynth's core functionality directly
    /// - Integrate with existing VapourSynth workflows
    ///
    /// # Requirements
    ///
    /// This method is only available when:
    /// - The `vapoursynth` feature is enabled
    /// - The decoder is using the VapourSynth backend (created via `from_file()` with
    ///   VapourSynth available, or via `from_script()`)
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(&mut Environment)` - A mutable reference to the VapourSynth environment
    /// - `Err(DecoderError::UnsupportedDecoder)` - If the current decoder is not using VapourSynth
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The decoder was not initialized with VapourSynth (e.g., using Y4M or FFmpeg backend)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    ///
    /// let mut decoder = Decoder::from_file("video.mkv")?;
    ///
    /// // Access the VapourSynth environment for advanced operations
    /// if let Ok(env) = decoder.get_vapoursynth_env() {
    ///     // Load additional plugins
    ///     // Note: This is a simplified example - actual VapourSynth API usage
    ///     // would require more specific vapoursynth crate methods
    ///     println!("VapourSynth environment available");
    ///     
    ///     // You can now use the environment for advanced VapourSynth operations
    ///     // such as loading plugins, creating nodes, etc.
    /// }
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth_env(&mut self) -> Result<&mut Environment, DecoderError> {
        match self.decoder {
            DecoderImpl::Vapoursynth(ref mut dec) => Ok(dec.get_env()),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }

    /// Returns the VapourSynth video node representing the decoded video stream.
    ///
    /// This method provides access to the underlying VapourSynth `Node` that represents
    /// the video source. The node can be used for advanced VapourSynth operations,
    /// creating additional processing pipelines, or integrating with other VapourSynth
    /// workflows and tools.
    ///
    /// VapourSynth nodes are the fundamental building blocks of video processing
    /// pipelines and can be used to:
    /// - Apply additional filters and transformations
    /// - Create branched processing pipelines
    /// - Extract frame metadata and properties
    /// - Implement custom frame processing logic
    /// - Interface with other VapourSynth-based applications
    ///
    /// # Requirements
    ///
    /// This method is only available when:
    /// - The `vapoursynth` feature is enabled
    /// - The decoder is using the VapourSynth backend (created via `from_file()` with
    ///   VapourSynth available, or via `from_script()`)
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing:
    /// - `Ok(Node)` - The VapourSynth node representing the video stream
    /// - `Err(DecoderError::UnsupportedDecoder)` - If the current decoder is not using VapourSynth
    ///
    /// # Errors
    ///
    /// This method will return an error if:
    /// - The decoder was not initialized with VapourSynth (e.g., using Y4M or FFmpeg backend)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use av_decoders::Decoder;
    ///
    /// let decoder = Decoder::from_file("video.mkv")?;
    ///
    /// // Get the VapourSynth node for advanced processing
    /// if let Ok(node) = decoder.get_vapoursynth_node() {
    ///     // You can now use this node for additional VapourSynth operations
    ///     // Note: This example shows the concept - actual usage would depend
    ///     // on specific VapourSynth operations you want to perform
    ///     
    ///     println!("Got VapourSynth node");
    ///     
    ///     // Example: You could apply additional filters to this node
    ///     // let filtered_node = apply_custom_filter(node);
    ///     
    ///     // Or use it to create a new processing pipeline
    ///     // let output_node = create_processing_pipeline(node);
    /// }
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// ## Advanced Usage
    ///
    /// ```no_run
    /// # use av_decoders::Decoder;
    /// # use std::collections::HashMap;
    /// // Create a decoder from a script
    /// let script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    /// clip = core.ffms2.Source("input.mkv")
    /// clip.set_output()
    /// "#;
    ///
    /// let decoder = Decoder::from_script(script, None)?;
    ///
    /// // Get the node and use it for further processing
    /// let node = decoder.get_vapoursynth_node()?;
    ///
    /// // Now you can integrate this node into larger VapourSynth workflows
    /// // or apply additional processing that wasn't included in the original script
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    #[inline]
    #[cfg(feature = "vapoursynth")]
    pub fn get_vapoursynth_node(&self) -> Result<Node, DecoderError> {
        match self.decoder {
            DecoderImpl::Vapoursynth(ref dec) => Ok(dec.get_output_node()),
            _ => Err(DecoderError::UnsupportedDecoder),
        }
    }
}

/// Internal enum representing different decoder backend implementations.
///
/// This enum is used internally by the `Decoder` struct to store the specific
/// decoder implementation being used. The appropriate variant is selected automatically
/// based on the input format and available features during decoder initialization.
///
/// Each variant wraps a different decoder backend, allowing the unified `Decoder`
/// interface to support multiple video formats and processing libraries.
pub enum DecoderImpl {
    /// Y4M format decoder using the built-in y4m parser.
    ///
    /// This variant provides fast, low-overhead decoding of Y4M (YUV4MPEG2) format files.
    /// It's always available and is preferred for Y4M files due to its performance characteristics.
    /// The decoder reads from any source implementing the `Read` trait.
    Y4m(Y4mDecoder<Box<dyn Read>>),

    /// VapourSynth-based decoder for advanced video processing.
    ///
    /// This variant uses the VapourSynth framework for video decoding and processing.
    /// It provides the most accurate metadata extraction and supports complex video
    /// processing pipelines. Only available when the `vapoursynth` feature is enabled.
    #[cfg(feature = "vapoursynth")]
    Vapoursynth(VapoursynthDecoder),

    /// FFmpeg-based decoder for general video format support.
    ///
    /// This variant uses FFmpeg for broad video format compatibility, serving as a
    /// fallback decoder for formats not handled by other backends. Only available
    /// when the `ffmpeg` feature is enabled.
    #[cfg(feature = "ffmpeg")]
    Ffmpeg(FfmpegDecoder),
}

impl DecoderImpl {
    pub(crate) fn video_details(&self) -> Result<VideoDetails, DecoderError> {
        match self {
            Self::Y4m(dec) => Ok(helpers::y4m::get_video_details(dec)),
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.get_video_details(),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(dec) => Ok(dec.video_details),
        }
    }

    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
    ) -> Result<Frame<T>, DecoderError> {
        match self {
            Self::Y4m(dec) => helpers::y4m::read_video_frame::<Box<dyn Read>, T>(dec, cfg),
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.read_video_frame::<T>(cfg),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(dec) => dec.read_video_frame::<T>(),
        }
    }

    pub(crate) fn seek_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
        frame_index: usize,
    ) -> Result<Frame<T>, DecoderError> {
        match self {
            Self::Y4m(_) => {
                // Seeking to a specific frame in Y4M is not supported
                Err(DecoderError::UnsupportedDecoder)
            }
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.seek_video_frame::<T>(cfg, frame_index),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(_) => Err(DecoderError::UnsupportedDecoder),
        }
    }
}
