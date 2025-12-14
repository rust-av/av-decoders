use std::{
    ffi::CString,
    num::{NonZeroU8, NonZeroUsize},
    path::Path,
    slice,
    str::FromStr,
    sync::{LazyLock, Once},
};

use ffms2_sys::{
    FFMS_CreateIndexer, FFMS_CreateVideoSource, FFMS_DestroyIndex, FFMS_DestroyVideoSource,
    FFMS_DoIndexing2, FFMS_ErrorInfo, FFMS_GetFirstIndexedTrackOfType, FFMS_GetFrame,
    FFMS_GetPixFmt, FFMS_GetVideoProperties, FFMS_Index, FFMS_IndexBelongsToFile, FFMS_Init,
    FFMS_ReadIndex, FFMS_Resizers, FFMS_SetOutputFormatV2, FFMS_TrackType,
    FFMS_TrackTypeIndexSettings, FFMS_VideoSource, FFMS_WriteIndex,
};
use num_rational::Rational32;
use v_frame::{
    chroma::ChromaSubsampling,
    frame::{Frame, FrameBuilder},
    pixel::Pixel,
};

use crate::{DecoderError, LUMA_PADDING, VideoDetails};

/// Ensures FFMS2 is initialized only once per process
static FFMS2_INIT: Once = Once::new();

/// A decoder for video files using the FFMS2 library.
///
/// This struct represents a video decoder that uses the FFMS2 library to decode video files.
/// It holds video details, a video source handle, and an index handle for efficient frame access.
///
/// # Fields
/// * `video_details` - Contains information about the video such as width, height, frame rate, etc.
/// * `video_source` - A pointer to the FFMS2 video source.
/// * `index_handle` - A handle to the index used for efficient frame access.
///
/// # Safety
/// This struct contains raw pointers and should be used with care. The `Drop` implementation
/// ensures proper cleanup of resources.
pub struct Ffms2Decoder {
    /// Contains the resolution and bit depth of the loaded video stream
    pub video_details: VideoDetails,
    video_source: *mut FFMS_VideoSource,
    #[expect(dead_code, reason = "Keep alive until drop")]
    index_handle: FfmsIndex,
}

impl Drop for Ffms2Decoder {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: we validate that the handle exists before freeing it
        unsafe {
            FFMS_DestroyVideoSource(self.video_source);
        }
    }
}

pub struct FfmsIndex {
    pub path: String,
    pub track: i32,
    pub idx_handle: *mut FFMS_Index,
}

impl Drop for FfmsIndex {
    fn drop(&mut self) {
        // SAFETY: we validate that the handle exists before freeing it
        unsafe {
            if !self.idx_handle.is_null() {
                FFMS_DestroyIndex(self.idx_handle);
            }
        }
    }
}

impl Ffms2Decoder {
    /// Creates a new `Ffms2Decoder` instance for the given input file.
    ///
    /// This function initializes the FFMS2 library, creates an index for the input file,
    /// and sets up a video source for decoding. It returns a new `Ffms2Decoder` instance
    /// if successful, or an error if any step fails.
    ///
    /// # Arguments
    ///
    /// * `input` - A path to the input video file.
    ///
    /// # Returns
    ///
    /// * `Result<Self, DecoderError>` - A new `Ffms2Decoder` instance on success, or a `DecoderError` on failure.
    ///
    /// # Errors
    ///
    /// This function can return the following errors:
    /// * `DecoderError::FileReadError` - If there's an error converting the input path to a `CString`.
    /// * `DecoderError::GenericDecodeError` - If there's an error creating the video source, indexer, or indexing the input file.
    /// * `DecoderError::UnsupportedFormat` - If the pixel format of the video is not supported.
    ///
    /// # Safety
    ///
    /// This function performs unsafe operations to interact with the FFMS2 library.
    /// It ensures proper error handling and resource cleanup.
    #[inline]
    pub fn new<P: AsRef<Path>>(input: P) -> Result<Self, DecoderError> {
        FFMS2_INIT.call_once(|| {
            // SAFETY: FFI call with infallible parameters
            unsafe {
                FFMS_Init(0, 0);
            }
        });

        let index_handle = Self::get_index(input.as_ref())?;

        let threads = std::thread::available_parallelism().map_or(8, std::num::NonZero::get) as i32;

        let source =
            CString::new(index_handle.path.as_str()).map_err(|e| DecoderError::FileReadError {
                cause: e.to_string(),
            })?;
        // SAFETY: we free this on all branches below
        let mut err = unsafe { empty_error_info() };
        // SAFETY: `source` is not null since we just created it
        let video_source = unsafe {
            FFMS_CreateVideoSource(
                source.as_ptr(),
                index_handle.track,
                index_handle.idx_handle,
                threads,
                0,
                std::ptr::addr_of_mut!(err),
            )
        };

        if video_source.is_null() {
            let error_msg = get_error_message(err);
            free_error_info(&mut err);
            return Err(DecoderError::GenericDecodeError {
                cause: format!("Failed to create video source: {}", error_msg),
            });
        }

        free_error_info(&mut err);

        // SAFETY: verified that `video_source` is not null
        let video_details = unsafe { Self::get_video_details(video_source)? };

        Ok(Self {
            video_details,
            video_source,
            index_handle,
        })
    }

    /// Sets the FFMS2 video source output characteristics, allowing for fast resizing and bit depth conversion.
    ///
    /// This forwards the requested resolution, bit depth, and chroma layout through `FFMS_SetOutputFormatV2` before
    /// decoding, making the resizing transparent to the consumer. Currently supports converting to YUV420, 422, or 444
    /// in 8-bit, 10-bit, or 12-bit. Any input formats are supported.
    ///
    /// If the resolution is equivalent to the input, no resizing is performed. If the bit depth and subsampling
    /// are equivalent to the input, no colorspace resampling is performed.
    ///
    /// # Parameters
    /// * `width` - Desired output width in pixels.
    /// * `height` - Desired output height in pixels.
    /// * `bit_depth` - Desired per-plane bit depth (e.g., 10 for 10-bit output).
    /// * `chroma_subsampling` - Tuple matching the FFMS2 chroma layout (horizontal, vertical).
    ///
    /// # Errors
    /// * `DecoderError::UnsupportedFormat` - The bit depth / chroma combination is not currently supported by this library.
    #[inline]
    pub fn set_output_format(
        &mut self,
        width: usize,
        height: usize,
        bit_depth: u8,
        chroma_subsampling: (u8, u8),
    ) -> Result<(), DecoderError> {
        // SAFETY: we free this on all branches below
        let mut err = unsafe { empty_error_info() };
        // SAFETY: `self.video_source` cannot be null here
        unsafe {
            FFMS_SetOutputFormatV2(
                self.video_source,
                // I HATE C
                [
                    video_info_to_pixel_format(bit_depth, chroma_subsampling)?,
                    -1,
                ]
                .as_ptr(),
                width as i32,
                height as i32,
                FFMS_Resizers::FFMS_RESIZER_BICUBIC as i32,
                std::ptr::addr_of_mut!(err),
            );
        }
        if err.ErrorType != 0 {
            let msg = get_error_message(err);
            free_error_info(&mut err);
            return Err(DecoderError::Ffms2InternalError { cause: msg });
        }
        free_error_info(&mut err);

        // SAFETY: `self.video_source` cannot be null here
        self.video_details = unsafe { Self::get_video_details(self.video_source)? };

        Ok(())
    }

    fn get_index(input: &Path) -> Result<FfmsIndex, DecoderError> {
        // SAFETY: we free this on all branches below
        let mut err = unsafe { empty_error_info() };

        let input_cstr = CString::from_str(&input.to_string_lossy()).map_err(|e| {
            DecoderError::FileReadError {
                cause: e.to_string(),
            }
        })?;

        let idx_path = format!("{}.ffindex", input.to_string_lossy());
        let idx_cstr =
            CString::new(idx_path.as_str()).map_err(|e| DecoderError::FileReadError {
                cause: e.to_string(),
            })?;

        let mut idx = if std::path::Path::new(&idx_path).exists() {
            // SAFETY: `idx_cstr` is not null since we just created it
            unsafe { FFMS_ReadIndex(idx_cstr.as_ptr(), std::ptr::addr_of_mut!(err)) }
        } else {
            std::ptr::null_mut()
        };

        if !idx.is_null()
            &&
            // SAFETY: verified `idx` is not null
            unsafe {
                FFMS_IndexBelongsToFile(idx, input_cstr.as_ptr(), std::ptr::addr_of_mut!(err)) != 0
            }
        {
            // Found an existing index file but it's not valid for this video file
            // SAFETY: verified `idx` is not null
            unsafe { FFMS_DestroyIndex(idx) };
            idx = std::ptr::null_mut();
        }

        let idx = if idx.is_null() {
            // SAFETY: `input_cstr` is not null since we created it
            let idxer =
                unsafe { FFMS_CreateIndexer(input_cstr.as_ptr(), std::ptr::addr_of_mut!(err)) };
            if idxer.is_null() {
                let error_msg = get_error_message(err);
                free_error_info(&mut err);
                return Err(DecoderError::GenericDecodeError {
                    cause: format!("Failed to create indexer: {}", error_msg),
                });
            }

            // SAFETY: verified `idxer` is not null
            let idx = unsafe {
                // Disable indexing for non-video tracks
                FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_AUDIO as i32, 0, 0);
                FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_DATA as i32, 0, 0);
                FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_SUBTITLE as i32, 0, 0);
                FFMS_TrackTypeIndexSettings(
                    idxer,
                    FFMS_TrackType::FFMS_TYPE_ATTACHMENT as i32,
                    0,
                    0,
                );

                FFMS_DoIndexing2(idxer, 0, std::ptr::addr_of_mut!(err))
            };

            if idx.is_null() {
                let error_msg = get_error_message(err);
                free_error_info(&mut err);
                return Err(DecoderError::GenericDecodeError {
                    cause: format!("Failed to index input file: {}", error_msg),
                });
            }

            // SAFETY: verified `idx` is not null
            unsafe { FFMS_WriteIndex(idx_cstr.as_ptr(), idx, std::ptr::addr_of_mut!(err)) };
            idx
        } else {
            idx
        };

        // SAFETY: verified `idx` is not null
        let track = unsafe { FFMS_GetFirstIndexedTrackOfType(idx, 0, std::ptr::addr_of_mut!(err)) };

        free_error_info(&mut err);

        Ok(FfmsIndex {
            path: input.to_string_lossy().to_string(),
            track,

            idx_handle: idx,
        })
    }

    unsafe fn get_video_details(
        video: *mut FFMS_VideoSource,
    ) -> Result<VideoDetails, DecoderError> {
        // SAFETY: caller must verify that `video` is not null
        unsafe {
            let mut err = std::mem::zeroed::<FFMS_ErrorInfo>();

            let props = FFMS_GetVideoProperties(video);
            let frame = FFMS_GetFrame(video, 0, std::ptr::addr_of_mut!(err));

            let width = if (*frame).ScaledWidth > 0 {
                (*frame).ScaledWidth
            } else {
                (*frame).EncodedWidth
            } as usize;
            let height = if (*frame).ScaledHeight > 0 {
                (*frame).ScaledHeight
            } else {
                (*frame).EncodedHeight
            } as usize;
            let frame_rate =
                Rational32::new((*props).FPSNumerator as i32, (*props).FPSDenominator as i32);
            let total_frames = Some((*props).NumFrames as usize);

            // Extract bit depth and chroma sampling from pixel format
            let pix_fmt = (*frame).ConvertedPixelFormat;
            let (bit_depth, chroma_sampling) = pixel_format_to_video_info(pix_fmt)?;

            let inf = VideoDetails {
                width,
                height,
                bit_depth,
                chroma_sampling,
                frame_rate,
                total_frames,
            };

            Ok(inf)
        }
    }

    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        frame_index: usize,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        if frame_index
            >= self
                .video_details
                .total_frames
                .expect("ffms2 decoder knows frame count")
        {
            return Err(DecoderError::EndOfFile);
        }
        // SAFETY: we free `err` on all branches below
        let mut err = unsafe { empty_error_info() };
        // SAFETY: `self.video_source` cannot be null
        let raw_frame = unsafe {
            FFMS_GetFrame(
                self.video_source,
                i32::try_from(frame_index).unwrap_or(0),
                std::ptr::addr_of_mut!(err),
            )
        };
        if raw_frame.is_null() {
            let error_msg = get_error_message(err);
            free_error_info(&mut err);
            return Err(DecoderError::Ffms2InternalError {
                cause: format!("Failed to read frame: {error_msg}"),
            });
        }
        free_error_info(&mut err);

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

        // SAFETY: we assume that the values provided by VapourSynth are correct
        unsafe {
            frame.y_plane.copy_from_u8_slice_with_stride(
                slice::from_raw_parts(
                    (*raw_frame).Data[0],
                    (*raw_frame).Linesize[0] as usize * self.video_details.height,
                ),
                NonZeroUsize::new((*raw_frame).Linesize[0] as usize)
                    .expect("zero stride should be impossible"),
            )
        }
        .map_err(|e| DecoderError::GenericDecodeError {
            cause: e.to_string(),
        })?;
        if let Some(u_plane) = frame.u_plane.as_mut() {
            // SAFETY: we assume that the values provided by VapourSynth are correct
            unsafe {
                u_plane.copy_from_u8_slice_with_stride(
                    slice::from_raw_parts(
                        (*raw_frame).Data[1],
                        (*raw_frame).Linesize[1] as usize * self.video_details.height,
                    ),
                    NonZeroUsize::new((*raw_frame).Linesize[1] as usize)
                        .expect("zero stride should be impossible"),
                )
            }
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
        }
        if let Some(v_plane) = frame.v_plane.as_mut() {
            // SAFETY: we assume that the values provided by VapourSynth are correct
            unsafe {
                v_plane.copy_from_u8_slice_with_stride(
                    slice::from_raw_parts(
                        (*raw_frame).Data[2],
                        (*raw_frame).Linesize[2] as usize * self.video_details.height,
                    ),
                    NonZeroUsize::new((*raw_frame).Linesize[2] as usize)
                        .expect("zero stride should be impossible"),
                )
            }
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
        }

        Ok(frame)
    }
}

// FFmpeg pixel format constants (from libavutil/pixfmt.h)
// These are used to interpret FFMS_Frame::ConvertedPixelFormat values
// Using `FFMS_GetPixFmt` ensures we have the correct value regardless
// of the ffmpeg version we are linked against
static AV_PIX_FMT_YUV420P: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv420p".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV422P: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv422p".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV444P: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv444p".as_ptr().cast()) }
});
static AV_PIX_FMT_GRAY8: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"gray8".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV420P10BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv420p10be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV420P10LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv420p10le".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV422P10BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv422p10be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV422P10LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv422p10le".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV444P10BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv444p10be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV444P10LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv444p10le".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV420P12BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv420p12be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV420P12LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv420p12le".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV422P12BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv422p12be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV422P12LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv422p12le".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV444P12BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv444p12be".as_ptr().cast()) }
});
static AV_PIX_FMT_YUV444P12LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"yuv444p12le".as_ptr().cast()) }
});
static AV_PIX_FMT_GRAY12BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"gray12be".as_ptr().cast()) }
});
static AV_PIX_FMT_GRAY12LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"gray12le".as_ptr().cast()) }
});
static AV_PIX_FMT_GRAY10BE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"gray10be".as_ptr().cast()) }
});
static AV_PIX_FMT_GRAY10LE: LazyLock<i32> = LazyLock::new(|| {
    // SAFETY: FFI call with a const C string
    unsafe { FFMS_GetPixFmt(c"gray10le".as_ptr().cast()) }
});

/// Maps FFmpeg pixel format to bit depth and chroma sampling
fn pixel_format_to_video_info(pix_fmt: i32) -> Result<(usize, ChromaSubsampling), DecoderError> {
    match pix_fmt {
        // 8-bit formats
        x if x == *AV_PIX_FMT_YUV420P => Ok((8, ChromaSubsampling::Yuv420)),
        x if x == *AV_PIX_FMT_YUV422P => Ok((8, ChromaSubsampling::Yuv422)),
        x if x == *AV_PIX_FMT_YUV444P => Ok((8, ChromaSubsampling::Yuv444)),
        x if x == *AV_PIX_FMT_GRAY8 => Ok((8, ChromaSubsampling::Monochrome)),

        // 10-bit formats
        x if x == *AV_PIX_FMT_YUV420P10LE || x == *AV_PIX_FMT_YUV420P10BE => {
            Ok((10, ChromaSubsampling::Yuv420))
        }
        x if x == *AV_PIX_FMT_YUV422P10LE || x == *AV_PIX_FMT_YUV422P10BE => {
            Ok((10, ChromaSubsampling::Yuv422))
        }
        x if x == *AV_PIX_FMT_YUV444P10LE || x == *AV_PIX_FMT_YUV444P10BE => {
            Ok((10, ChromaSubsampling::Yuv444))
        }
        x if x == *AV_PIX_FMT_GRAY10LE || x == *AV_PIX_FMT_GRAY10BE => {
            Ok((10, ChromaSubsampling::Monochrome))
        }

        // 12-bit formats
        x if x == *AV_PIX_FMT_YUV420P12LE || x == *AV_PIX_FMT_YUV420P12BE => {
            Ok((12, ChromaSubsampling::Yuv420))
        }
        x if x == *AV_PIX_FMT_YUV422P12LE || x == *AV_PIX_FMT_YUV422P12BE => {
            Ok((12, ChromaSubsampling::Yuv422))
        }
        x if x == *AV_PIX_FMT_YUV444P12LE || x == *AV_PIX_FMT_YUV444P12BE => {
            Ok((12, ChromaSubsampling::Yuv444))
        }
        x if x == *AV_PIX_FMT_GRAY12LE || x == *AV_PIX_FMT_GRAY12BE => {
            Ok((12, ChromaSubsampling::Monochrome))
        }

        _ => Err(DecoderError::UnsupportedFormat {
            fmt: format!("Unsupported pixel format: {}", pix_fmt),
        }),
    }
}

fn video_info_to_pixel_format(
    bit_depth: u8,
    chroma_subsampling: (u8, u8),
) -> Result<i32, DecoderError> {
    Ok(
        match (bit_depth, chroma_subsampling.0 + chroma_subsampling.1) {
            // 8-bit formats
            (8, 2) => *AV_PIX_FMT_YUV420P,
            (8, 1) => *AV_PIX_FMT_YUV422P,
            (8, 0) => *AV_PIX_FMT_YUV444P,

            // 10-bit formats
            (10, 2) => *AV_PIX_FMT_YUV420P10LE,
            (10, 1) => *AV_PIX_FMT_YUV422P10LE,
            (10, 0) => *AV_PIX_FMT_YUV444P10LE,

            // 12-bit formats
            (12, 2) => *AV_PIX_FMT_YUV420P12LE,
            (12, 1) => *AV_PIX_FMT_YUV422P12LE,
            (12, 0) => *AV_PIX_FMT_YUV444P12LE,

            _ => {
                return Err(DecoderError::UnsupportedFormat {
                    fmt: "Unsupported bit depth and subsampling combination".to_string(),
                });
            }
        },
    )
}

const ERR_BUFFER_SIZE: usize = 1024;

/// Creates a new `FFMS_ErrorInfo` struct with allocated buffer
///
/// # Returns
/// A new `FFMS_ErrorInfo` struct with a 1024-byte buffer allocated
///
/// # Safety
/// The caller is responsible for freeing the allocated buffer when done
unsafe fn empty_error_info() -> FFMS_ErrorInfo {
    // SAFETY: we fill the required buffer before returning
    let mut err: FFMS_ErrorInfo = unsafe { std::mem::zeroed() };
    // Allocate 1024 bytes for the error buffer
    let buffer = vec![0u8; ERR_BUFFER_SIZE];
    let buffer_ptr = buffer.as_ptr() as *mut i8;
    #[expect(
        clippy::mem_forget,
        reason = "intentionally avoid drop here, must be freed by caller"
    )]
    std::mem::forget(buffer);
    err.Buffer = buffer_ptr;
    err.BufferSize = ERR_BUFFER_SIZE as i32;
    err
}

/// Extracts error message from `FFMS_ErrorInfo` struct
///
/// # Safety
/// The `FFMS_ErrorInfo` struct must be properly initialized by an FFMS2 function call
fn get_error_message(err: FFMS_ErrorInfo) -> String {
    if err.Buffer.is_null() {
        return "Unknown error".to_string();
    }

    // SAFETY: we validated that buffer is not null
    unsafe { std::ffi::CStr::from_ptr(err.Buffer) }
        .to_string_lossy()
        .into_owned()
}

/// Frees the buffer allocated by `empty_error_info`
///
/// # Safety
/// The buffer must be a valid pointer returned by `empty_error_info`
fn free_error_info(err: &mut FFMS_ErrorInfo) {
    if !err.Buffer.is_null() {
        // SAFETY: we validated that buffer is not null
        let _ = unsafe { Box::from_raw(err.Buffer as *mut [u8; ERR_BUFFER_SIZE]) };
        err.Buffer = std::ptr::null_mut();
    }
}
