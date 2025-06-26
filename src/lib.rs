//! This crate providers ready-made decoders for use with the rust-av ecosystem.
//! Each decoder will output structs from the `v_frame` crate.
//!
//! Only the y4m decoder is enabled by default.
//! Others must be enabled via Cargo features, since they require external dependencies.

#[cfg(feature = "ffmpeg")]
use crate::helpers::ffmpeg::FfmpegDecoder;
#[cfg(feature = "vapoursynth")]
use crate::helpers::vapoursynth::VapoursynthDecoder;
use std::fs::File;
use std::io::{stdin, BufReader, Read, Stdin};
use std::path::Path;
use v_frame::frame::Frame;
use v_frame::pixel::{ChromaSampling, Pixel};
use y4m::Decoder as Y4mDecoder;

mod error;
mod helpers {
    #[cfg(feature = "ffmpeg")]
    pub(crate) mod ffmpeg;
    #[cfg(feature = "vapoursynth")]
    pub(crate) mod vapoursynth;
    pub(crate) mod y4m;
}

pub use error::DecoderError;
pub use num_rational::Rational32;
pub use v_frame;

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
/// 2. **VapourSynth** - Used when available (best metadata accuracy and seeking)
/// 3. **FFmpeg** - Used as fallback for general video files
///
/// ## Examples
///
/// ```no_run
/// use av_decoders::{from_file, from_stdin};
///
/// // Decode from a file
/// let mut decoder = from_file("video.y4m")?;
/// let details = decoder.get_video_details();
/// println!("Video: {}x{} @ {} fps", details.width, details.height, details.frame_rate);
///
/// // Read frames
/// while let Ok(frame) = decoder.read_video_frame::<u8>() {
///     // Process the frame...
/// }
///
/// // Decode from stdin
/// let mut stdin_decoder = from_stdin()?;
/// let frame = stdin_decoder.read_video_frame::<u8>()?;
/// # Ok::<(), av_decoders::DecoderError>(())
/// ```
pub struct Decoder<R: Read> {
    decoder: DecoderImpl<R>,
    video_details: VideoDetails,
}

/// Creates a new decoder from a file path.
///
/// This method automatically detects the input format and selects the most appropriate
/// decoder backend. It will prioritize Y4M files for performance, then VapourSynth
/// for accuracy, and finally FFmpeg for broad format support.
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
/// use av_decoders::from_file;
/// use std::path::Path;
///
/// // From string path
/// let decoder = from_file("video.y4m")?;
///
/// // From Path
/// let path = Path::new("video.mp4");
/// let decoder = from_file(path)?;
///
/// // From PathBuf
/// let pathbuf = std::env::current_dir()?.join("video.mkv");
/// let decoder = from_file(pathbuf)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[inline]
#[allow(unreachable_code)]
#[allow(clippy::needless_return)]
pub fn from_file<P: AsRef<Path>>(input: P) -> Result<Decoder<BufReader<File>>, DecoderError> {
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
            let decoder = DecoderImpl::Y4m(y4m::decode(reader).map_err(|e| match e {
                y4m::Error::EOF => DecoderError::EndOfFile,
                _ => DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                },
            })?);
            let video_details = decoder.video_details()?;
            return Ok(Decoder {
                decoder,
                video_details,
            });
        }
    }

    // Vapoursynth tends to give the most video metadata and have the best frame accuracy when seeking,
    // so we should prioritize it over ffmpeg.
    #[cfg(feature = "vapoursynth")]
    {
        let decoder = DecoderImpl::Vapoursynth(VapoursynthDecoder::new(input)?);
        let video_details = decoder.video_details()?;
        return Ok(Decoder {
            decoder,
            video_details,
        });
    }

    #[cfg(feature = "ffmpeg")]
    {
        let decoder = DecoderImpl::Ffmpeg(FfmpegDecoder::new(input)?);
        let video_details = decoder.video_details()?;
        return Ok(Decoder {
            decoder,
            video_details,
        });
    }

    Err(DecoderError::NoDecoder)
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
/// use av_decoders::from_stdin;
///
/// // Read Y4M data from stdin
/// let mut decoder = from_stdin()?;
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
pub fn from_stdin() -> Result<Decoder<BufReader<Stdin>>, DecoderError> {
    // We can only support y4m for this
    let reader = BufReader::new(stdin());
    let decoder = DecoderImpl::Y4m(y4m::decode(reader).map_err(|e| match e {
        y4m::Error::EOF => DecoderError::EndOfFile,
        _ => DecoderError::GenericDecodeError {
            cause: e.to_string(),
        },
    })?);
    let video_details = decoder.video_details()?;
    Ok(Decoder {
        decoder,
        video_details,
    })
}

impl<R: Read> Decoder<R> {
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
    /// use av_decoders::from_file;
    /// use v_frame::pixel::ChromaSampling;
    ///
    /// let decoder = from_file("video.y4m").unwrap();
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
    /// use av_decoders::from_file;
    ///
    /// let mut decoder = from_file("video.y4m").unwrap();
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
}

enum DecoderImpl<R: Read> {
    Y4m(Y4mDecoder<R>),
    #[cfg(feature = "vapoursynth")]
    Vapoursynth(VapoursynthDecoder),
    #[cfg(feature = "ffmpeg")]
    Ffmpeg(FfmpegDecoder),
}

impl<R: Read> DecoderImpl<R> {
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
            Self::Y4m(dec) => helpers::y4m::read_video_frame::<R, T>(dec, cfg),
            #[cfg(feature = "vapoursynth")]
            Self::Vapoursynth(dec) => dec.read_video_frame::<T>(cfg),
            #[cfg(feature = "ffmpeg")]
            Self::Ffmpeg(dec) => dec.read_video_frame::<T>(),
        }
    }
}
