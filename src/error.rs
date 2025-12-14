use thiserror::Error;

/// Errors that can occur during video decoding operations.
///
/// This enum represents all possible error conditions that may arise when
/// decoding video files using the av-decoders library. Errors range from
/// file I/O issues to unsupported video formats and decoder-specific problems.
#[derive(Debug, Clone, Error)]
pub enum DecoderError {
    /// Indicates that the end of the video file has been reached.
    ///
    /// This is returned when attempting to read frames beyond the end of the video stream.
    /// This is typically not an error condition but rather a normal end-of-stream indicator.
    #[error("end of video file reached")]
    EndOfFile,

    /// Failed to read or open the input video file.
    ///
    /// This error occurs when the decoder cannot access the specified file,
    /// either due to file permissions, file not found, or other I/O related issues.
    #[error("failed to open input file ({cause})")]
    FileReadError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Error in the `VapourSynth` script execution.
    ///
    /// This error is returned when there's a problem with the `VapourSynth` script itself,
    /// such as syntax errors, invalid filter chains, or script logic issues.
    /// Only available when the `vapoursynth` feature is enabled.
    #[cfg(feature = "vapoursynth")]
    #[error("Vapoursynth script error ({cause})")]
    VapoursynthScriptError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Internal `VapourSynth` error.
    ///
    /// This represents errors that occur within the `VapourSynth` core or plugins,
    /// typically indicating issues with the `VapourSynth` installation or environment.
    /// Only available when the `vapoursynth` feature is enabled.
    #[cfg(feature = "vapoursynth")]
    #[error("Vapoursynth internal error ({cause})")]
    VapoursynthInternalError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Failure to set Vapoursynth script arguments.
    ///
    /// Only available when the `vapoursynth` feature is enabled.
    #[cfg(feature = "vapoursynth")]
    #[error("error setting Vapoursynth script args ({cause})")]
    VapoursynthArgsError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Internal FFmpeg error.
    ///
    /// This error occurs when FFmpeg encounters an internal problem during
    /// decoding operations, such as codec issues or corrupted video data.
    /// Only available when the `ffmpeg` feature is enabled.
    #[cfg(feature = "ffmpeg")]
    #[error("FFMpeg internal error ({cause})")]
    FfmpegInternalError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Internal FFMS2 error.
    ///
    /// This error occurs when FFMS2 encounters an internal problem during
    /// decoding operations, such as codec issues or corrupted video data.
    /// Only available when the `ffms2` feature is enabled.
    #[cfg(feature = "ffms2")]
    #[error("FFMS2 internal error ({cause})")]
    Ffms2InternalError {
        /// The underlying cause of the error
        cause: String,
    },

    /// Generic decoder error for issues not covered by specific error types.
    ///
    /// This is a catch-all error for various decoding problems that don't fit
    /// into other categories, providing additional context through the cause string.
    #[error("internal decoder error ({cause})")]
    GenericDecodeError {
        /// The underlying cause of the error
        cause: String,
    },

    /// No video stream found in the input file.
    ///
    /// This error is returned when the input file doesn't contain any decodeable
    /// video streams, or when all video streams are in unsupported formats.
    #[error("no decodeable video stream found in file")]
    NoVideoStream,

    /// No suitable decoder available for the input file.
    ///
    /// This occurs when none of the available decoders can handle the input format.
    /// Consider enabling additional features like `ffmpeg` or `vapoursynth` to
    /// support more video formats.
    #[error(
        "no decoder found which can decode this file--perhaps you need to enable the ffmpeg or vapoursynth feature"
    )]
    NoDecoder,

    /// The current decoder does not support the function which is being called.
    #[error("this function is not supported by the decoder in use")]
    UnsupportedDecoder,

    /// Variable format video clips are not supported.
    ///
    /// This error is returned when the video file contains streams with changing
    /// pixel formats throughout the video, which is currently not supported by
    /// this library.
    #[error("variable format clips are not currently supported")]
    VariableFormat,

    /// Variable resolution video clips are not supported.
    ///
    /// This error occurs when the video resolution changes during playback,
    /// which is not currently supported. All frames must have the same dimensions.
    #[error("variable resolution clips are not currently supported")]
    VariableResolution,

    /// Variable framerate video clips are not supported.
    ///
    /// This error is returned when the video has a variable framerate (VFR),
    /// where the time between frames is not constant. Only constant framerate
    /// videos are currently supported.
    #[error("variable framerate clips are not currently supported")]
    VariableFramerate,

    /// Unsupported chroma subsampling format.
    ///
    /// This error occurs when the video uses a chroma subsampling scheme that
    /// is not supported by the decoder. The `x` and `y` values indicate the
    /// horizontal and vertical subsampling factors respectively.
    #[error("unsupported chroma subsampling ({x}, {y})")]
    UnsupportedChromaSubsampling {
        /// The horizontal chroma subsampling which triggered the error
        x: usize,
        /// The vertical chroma subsampling which triggered the error
        y: usize,
    },

    /// Unsupported video format.
    ///
    /// This error is returned when the video uses a pixel format or codec
    /// that is not supported by the current decoder configuration.
    #[error("unsupported video format {fmt}")]
    UnsupportedFormat {
        /// The video format which triggered the error
        fmt: String,
    },
}
