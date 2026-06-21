use thiserror::Error;

/// Errors that can occur during video decoding operations.
#[derive(Debug, Clone, Error)]
pub enum DecoderError {
    /// End of video stream reached (not necessarily an error).
    #[error("end of video file reached")]
    EndOfFile,

    /// Failed to open or read the input file.
    #[error("failed to open input file ({cause})")]
    FileReadError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// VapourSynth script execution error (requires `vapoursynth` feature).
    #[cfg(feature = "vapoursynth")]
    #[error("Vapoursynth script error ({cause})")]
    VapoursynthScriptError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// VapourSynth internal/core error (requires `vapoursynth` feature).
    #[cfg(feature = "vapoursynth")]
    #[error("Vapoursynth internal error ({cause})")]
    VapoursynthInternalError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// Failed to set VapourSynth script arguments (requires `vapoursynth` feature).
    #[cfg(feature = "vapoursynth")]
    #[error("error setting Vapoursynth script args ({cause})")]
    VapoursynthArgsError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// FFmpeg internal error (requires `ffmpeg` feature).
    #[cfg(feature = "ffmpeg")]
    #[error("FFMpeg internal error ({cause})")]
    FfmpegInternalError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// FFMS2 internal error (requires `ffms2` feature).
    #[cfg(feature = "ffms2")]
    #[error("FFMS2 internal error ({cause})")]
    Ffms2InternalError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// Catch-all for decoding problems not covered by other variants.
    #[error("internal decoder error ({cause})")]
    GenericDecodeError {
        /// The underlying cause of the error.
        cause: String,
    },

    /// No decodeable video stream found in the input file.
    #[error("no decodeable video stream found in file")]
    NoVideoStream,

    /// No suitable decoder available; consider enabling `ffmpeg` or `vapoursynth`.
    #[error(
        "no decoder found which can decode this file--perhaps you need to enable the ffmpeg or vapoursynth feature"
    )]
    NoDecoder,

    /// The active decoder backend does not support the called function.
    #[error("this function is not supported by the decoder in use")]
    UnsupportedDecoder,

    /// Variable-format streams are not supported.
    #[error("variable format clips are not currently supported")]
    VariableFormat,

    /// Variable-resolution streams are not supported.
    #[error("variable resolution clips are not currently supported")]
    VariableResolution,

    /// Variable-framerate streams are not supported.
    #[error("variable framerate clips are not currently supported")]
    VariableFramerate,

    /// Unsupported chroma subsampling (`x`, `y` are horizontal/vertical factors).
    #[error("unsupported chroma subsampling ({x}, {y})")]
    UnsupportedChromaSubsampling {
        /// Horizontal chroma subsampling factor.
        x: usize,
        /// Vertical chroma subsampling factor.
        y: usize,
    },

    /// Unsupported pixel format or codec.
    #[error("unsupported video format {fmt}")]
    UnsupportedFormat {
        /// The format identifier that triggered the error.
        fmt: String,
    },
}
