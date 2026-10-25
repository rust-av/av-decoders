use std::{
    ffi::CString,
    ffi::{c_char, c_void},
    path::{Path, PathBuf},
    slice,
    str::FromStr,
    sync::{LazyLock, Once},
};

use ffms2_sys::{
    FFMS_CreateIndexer, FFMS_CreateVideoSource, FFMS_DestroyIndex, FFMS_DestroyVideoSource,
    FFMS_DoIndexing2, FFMS_ErrorInfo, FFMS_Errors, FFMS_GetFirstIndexedTrackOfType, FFMS_GetFrame,
    FFMS_GetNumTracks, FFMS_GetPixFmt, FFMS_GetTrackFromIndex, FFMS_GetTrackType,
    FFMS_GetVideoProperties, FFMS_Index, FFMS_IndexBelongsToFile, FFMS_Init, FFMS_ReadIndex,
    FFMS_Resizers, FFMS_SetOutputFormatV2, FFMS_SetProgressCallback, FFMS_TrackType,
    FFMS_TrackTypeIndexSettings, FFMS_VideoSource, FFMS_WriteIndex,
};
use num_rational::Rational32;
use v_frame::{chroma::ChromaSubsampling, frame::Frame, pixel::Pixel};

use crate::color::{
    ChromaLocation, ColorPrimaries, ColorRange, ColorSpace, ContentLightLevel,
    MasteringDisplayMetadata, TransferCharacteristic,
};
use crate::{DecoderError, VideoDetails};

use super::frame_builder::new_padded_frame;

/// Ensures FFMS2 is initialized only once per process
static FFMS2_INIT: Once = Once::new();

/// Reports FFMS2 indexing progress as a whole percentage, `0..=100`.
///
/// `0` is emitted once before indexing starts and `100` once indexing has
/// finished, even when a valid index already exists.
pub type IndexProgress<'a> = &'a mut dyn FnMut(u8);

/// Returns `true` when in-flight indexing should be aborted.
pub type IndexCancelled<'a> = &'a dyn Fn() -> bool;

/// Video decoder using the FFMS2 library.
///
/// Contains raw pointers; cleaned up via `Drop`.
pub struct Ffms2Decoder {
    /// Contains the resolution and bit depth of the loaded video stream
    pub video_details: VideoDetails,
    #[allow(missing_docs)]
    pub video_source: *mut FFMS_VideoSource,
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
    /// The video file the index was built from.
    pub path: String,
    /// The `.ffindex` file the index was read from or written to.
    pub index_path: PathBuf,
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

impl FfmsIndex {
    /// Selects the track to decode: `track_index` if given, else the first
    /// indexed video track.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::NoVideoStream`] when the index holds no video
    /// track, or when `track_index` is out of range or names a non-video track.
    fn resolve_track(idx: *mut FFMS_Index, track_index: Option<u8>) -> Result<i32, DecoderError> {
        let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
        let mut err = empty_error_info(&mut err_buffer);

        // SAFETY: `idx` is a live index handle for the whole call.
        let track = unsafe {
            let track_index = track_index.map_or_else(
                || FFMS_GetFirstIndexedTrackOfType(idx, 0, std::ptr::addr_of_mut!(err)),
                |index| index as i32,
            );

            // FFMS_Index is a std::vector subclass, so an out-of-range index is
            // undefined behavior rather than a null return. A missing track
            // type reports -1, which this also rejects.
            if track_index < 0 || track_index >= FFMS_GetNumTracks(idx) {
                return Err(DecoderError::NoVideoStream);
            }

            let track = FFMS_GetTrackFromIndex(idx, track_index);
            if FFMS_GetTrackType(track) != (FFMS_TrackType::FFMS_TYPE_VIDEO as i32) {
                return Err(DecoderError::NoVideoStream);
            }

            track_index
        };

        Ok(track)
    }
}

impl Ffms2Decoder {
    /// Creates a new FFMS2 decoder for the given video file.
    ///
    /// Initializes FFMS2, creates/reads an index, and sets up a video source.
    /// `track_index` defaults to the first video track if `None`.
    ///
    /// The index is read from, or written to, FFMS2's default location next to
    /// the file: `{input}.ffindex`. Use [`Ffms2Decoder::with_index`] to choose a
    /// different location or to observe indexing progress.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::NoVideoStream`] if no video track is found,
    /// [`DecoderError::UnsupportedFormat`] for unsupported pixel formats,
    /// [`DecoderError::GenericDecodeError`] on indexing or source creation failure.
    #[inline]
    pub fn new<P: AsRef<Path>>(input: P, track_index: Option<u8>) -> Result<Self, DecoderError> {
        let index_path = default_index_path(input.as_ref());
        Self::with_index(input, &index_path, track_index, None)
    }

    /// Creates a new FFMS2 decoder using the index at `index_path`.
    ///
    /// A missing or stale index is built first, reporting progress through
    /// `progress`. An index that already belongs to `input` is reused as-is.
    /// `track_index` defaults to the first video track if `None`.
    ///
    /// Use [`Ffms2Decoder::build_index`] instead when indexing may need to be
    /// aborted part-way through; the indexing work itself is identical.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::NoVideoStream`] if no video track is found,
    /// [`DecoderError::FileReadError`] for unusable paths,
    /// [`DecoderError::UnsupportedFormat`] for unsupported pixel formats,
    /// [`DecoderError::GenericDecodeError`] on indexing or source creation failure.
    #[inline]
    pub fn with_index<P: AsRef<Path>>(
        input: P,
        index_path: &Path,
        track_index: Option<u8>,
        progress: Option<IndexProgress<'_>>,
    ) -> Result<Self, DecoderError> {
        let input = input.as_ref();
        ensure_ffms2_init();

        let (idx_handle, _built) = Self::open_index(
            input,
            index_path,
            progress,
            None,
            IndexWriteFailure::Tolerated,
        )?;

        let track = match FfmsIndex::resolve_track(idx_handle, track_index) {
            Ok(track) => track,
            Err(e) => {
                // SAFETY: `idx_handle` came from `open_index` and is not owned
                // by anything else yet.
                unsafe { FFMS_DestroyIndex(idx_handle) };
                return Err(e);
            }
        };

        let index_handle = FfmsIndex {
            path: input.to_string_lossy().to_string(),
            index_path: index_path.to_path_buf(),
            track,
            idx_handle,
        };

        let source_cstr =
            CString::new(index_handle.path.as_str()).map_err(|e| DecoderError::FileReadError {
                cause: e.to_string(),
            })?;

        let threads = std::thread::available_parallelism().map_or(8, std::num::NonZero::get) as i32;

        let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
        let mut err = empty_error_info(&mut err_buffer);
        // SAFETY: `source_cstr` is not null since we just created it, and
        // `idx_handle` is a live index handed to the source below.
        let video_source = unsafe {
            FFMS_CreateVideoSource(
                source_cstr.as_ptr(),
                index_handle.track,
                index_handle.idx_handle,
                threads,
                1,
                std::ptr::addr_of_mut!(err),
            )
        };

        if video_source.is_null() {
            let error_msg = get_error_message(err);
            return Err(DecoderError::GenericDecodeError {
                cause: format!("Failed to create video source: {}", error_msg),
            });
        }

        // SAFETY: verified that `video_source` is not null
        let video_details = match unsafe { Self::get_video_details(video_source) } {
            Ok(details) => details,
            Err(e) => {
                // SAFETY: `video_source` was just created and not stored
                // anywhere, so ownership is still ours.
                unsafe { FFMS_DestroyVideoSource(video_source) };
                return Err(e);
            }
        };

        Ok(Self {
            video_details,
            video_source,
            index_handle,
        })
    }

    /// Ensures the index at `index_path` exists and belongs to `input`,
    /// building it if needed.
    ///
    /// `index_path` of `None` selects FFMS2's default location next to the
    /// source file: `{input}.ffindex`.
    ///
    /// Returns `true` when an index was built, `false` when an existing valid
    /// index was reused. Both cases report `0` then `100` to `progress`, so
    /// callers always see a complete bracket.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::IndexingCancelled`] if `cancelled` returns `true`,
    /// [`DecoderError::FileReadError`] for unusable paths, and
    /// [`DecoderError::GenericDecodeError`] if FFMS2 fails to index or write.
    #[inline]
    pub fn build_index<P: AsRef<Path>>(
        input: P,
        index_path: Option<&Path>,
        progress: Option<IndexProgress<'_>>,
        cancelled: Option<IndexCancelled<'_>>,
    ) -> Result<bool, DecoderError> {
        let input = input.as_ref();
        ensure_ffms2_init();

        let index_path = index_path.map_or_else(|| default_index_path(input), Path::to_path_buf);

        let (idx_handle, built) = Self::open_index(
            input,
            &index_path,
            progress,
            cancelled,
            IndexWriteFailure::Fatal,
        )?;

        // SAFETY: `idx_handle` was created solely to validate or build the
        // cache so this handle is no longer needed.
        unsafe { FFMS_DestroyIndex(idx_handle) };

        Ok(built)
    }

    /// Reads the index at `index_path`, or builds and writes it when it is
    /// missing or does not belong to `input`.
    ///
    /// Returns the live index handle plus whether an index was built; the caller
    /// owns the handle and must destroy it.
    fn open_index(
        input: &Path,
        index_path: &Path,
        mut progress: Option<IndexProgress<'_>>,
        cancelled: Option<IndexCancelled<'_>>,
        on_write_failure: IndexWriteFailure,
    ) -> Result<(*mut FFMS_Index, bool), DecoderError> {
        let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
        let mut err = empty_error_info(&mut err_buffer);

        let input_cstr = CString::from_str(&input.to_string_lossy()).map_err(|e| {
            DecoderError::FileReadError {
                cause: e.to_string(),
            }
        })?;

        let idx_path = index_path.to_string_lossy().to_string();
        let idx_cstr =
            CString::new(idx_path.as_str()).map_err(|e| DecoderError::FileReadError {
                cause: e.to_string(),
            })?;

        let mut idx = if index_path.exists() {
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

        if !idx.is_null() {
            // Cache hit: emit both 0 and 100 to signal completion.
            if let Some(progress) = progress.as_mut() {
                (**progress)(0);
                (**progress)(100);
            }

            return Ok((idx, false));
        }

        if let Some(progress) = progress.as_mut() {
            (**progress)(0);
        }

        // SAFETY: `input_cstr` is not null since we created it
        let idxer = unsafe { FFMS_CreateIndexer(input_cstr.as_ptr(), std::ptr::addr_of_mut!(err)) };
        if idxer.is_null() {
            let error_msg = get_error_message(err);
            return Err(DecoderError::GenericDecodeError {
                cause: format!("Failed to create indexer: {}", error_msg),
            });
        }

        let mut context = ProgressContext {
            progress,
            cancelled,
        };

        // SAFETY: `idxer` is live. `context` outlives the synchronous
        // `FFMS_DoIndexing2` call that owns the trampoline, and that call
        // consumes the indexer either way.
        let idx = unsafe {
            // Disable indexing for non-video tracks
            FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_AUDIO as i32, 0, 0);
            FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_DATA as i32, 0, 0);
            FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_SUBTITLE as i32, 0, 0);
            FFMS_TrackTypeIndexSettings(idxer, FFMS_TrackType::FFMS_TYPE_ATTACHMENT as i32, 0, 0);

            if context.has_callbacks() {
                FFMS_SetProgressCallback(
                    idxer,
                    Some(progress_trampoline),
                    std::ptr::from_mut(&mut context).cast::<c_void>(),
                );
            }

            FFMS_DoIndexing2(idxer, 0, std::ptr::addr_of_mut!(err))
        };

        if idx.is_null() {
            if err.ErrorType == FFMS_Errors::FFMS_ERROR_CANCELLED as i32 {
                return Err(DecoderError::IndexingCancelled {
                    cause: get_error_message(err),
                });
            }
            let error_msg = get_error_message(err);
            return Err(DecoderError::GenericDecodeError {
                cause: format!("Failed to index input file: {}", error_msg),
            });
        }

        // SAFETY: verified `idx` is not null
        let write_result =
            unsafe { FFMS_WriteIndex(idx_cstr.as_ptr(), idx, std::ptr::addr_of_mut!(err)) };
        if write_result != 0 && on_write_failure == IndexWriteFailure::Fatal {
            let error_msg = get_error_message(err);
            // SAFETY: verified `idx` is not null and has not been handed to
            // anything else, so ownership is still ours.
            unsafe { FFMS_DestroyIndex(idx) };
            return Err(DecoderError::GenericDecodeError {
                cause: format!("Failed to write index file: {}", error_msg),
            });
        }
        // A tolerated failure leaves the index in memory only; the caller still
        // gets a working decoder from an unwritable location such as read-only
        // media.

        if let Some(progress) = context.progress.as_mut() {
            (**progress)(100);
        }

        Ok((idx, true))
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
        chroma_subsampling: ChromaSubsampling,
    ) -> Result<(), DecoderError> {
        let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
        let mut err = empty_error_info(&mut err_buffer);
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
            return Err(DecoderError::Ffms2InternalError { cause: msg });
        }

        // SAFETY: `self.video_source` cannot be null here
        self.video_details = unsafe { Self::get_video_details(self.video_source)? };

        Ok(())
    }

    unsafe fn get_video_details(
        video: *mut FFMS_VideoSource,
    ) -> Result<VideoDetails, DecoderError> {
        // SAFETY: caller must verify that `video` is not null
        unsafe {
            let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
            let mut err = empty_error_info(&mut err_buffer);

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

            // Color metadata is passed through by FFMS2 using FFmpeg numbering
            let color_range = ColorRange::from_raw(i64::from((*frame).ColorRange));
            let color_primaries = ColorPrimaries::from_raw(i64::from((*frame).ColorPrimaries));
            let color_trc =
                TransferCharacteristic::from_raw(i64::from((*frame).TransferCharateristics));
            let color_space = ColorSpace::from_raw(i64::from((*frame).ColorSpace));
            let chroma_location = ChromaLocation::from_raw(i64::from((*frame).ChromaLocation));

            let mastering_display = ((*frame).HasMasteringDisplayPrimaries != 0
                || (*frame).HasMasteringDisplayLuminance != 0)
                .then(|| MasteringDisplayMetadata {
                    primaries: ((*frame).HasMasteringDisplayPrimaries != 0).then(|| {
                        let primaries = [
                            (
                                (*frame).MasteringDisplayPrimariesX[0],
                                (*frame).MasteringDisplayPrimariesY[0],
                            ),
                            (
                                (*frame).MasteringDisplayPrimariesX[1],
                                (*frame).MasteringDisplayPrimariesY[1],
                            ),
                            (
                                (*frame).MasteringDisplayPrimariesX[2],
                                (*frame).MasteringDisplayPrimariesY[2],
                            ),
                        ];
                        let white_point = (
                            (*frame).MasteringDisplayWhitePointX,
                            (*frame).MasteringDisplayWhitePointY,
                        );
                        (primaries, white_point)
                    }),
                    luminance: ((*frame).HasMasteringDisplayLuminance != 0).then(|| {
                        (
                            (*frame).MasteringDisplayMinLuminance,
                            (*frame).MasteringDisplayMaxLuminance,
                        )
                    }),
                });

            let content_light_level =
                ((*frame).HasContentLightLevel != 0).then(|| ContentLightLevel {
                    max_cll: (*frame).ContentLightLevelMax,
                    max_fall: (*frame).ContentLightLevelAverage,
                });

            let inf = VideoDetails {
                width,
                height,
                bit_depth,
                chroma_sampling,
                frame_rate,
                total_frames,
                color_range,
                color_primaries,
                color_transfer_characteristic: color_trc,
                color_space,
                chroma_location,
                mastering_display,
                content_light_level,
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
        let mut err_buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];
        let mut err = empty_error_info(&mut err_buffer);
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
            return Err(DecoderError::Ffms2InternalError {
                cause: format!("Failed to read frame: {error_msg}"),
            });
        }

        let mut frame: Frame<T> = new_padded_frame(&self.video_details, luma_only)?;

        let chroma_height = self.video_details.height
            / self
                .video_details
                .chroma_sampling
                .subsample_ratio()
                .map_or(1, |(_x, y)| y.get()) as usize;
        // SAFETY: we assume that the values provided by FFMS2 are correct
        unsafe {
            frame.y_plane.copy_from_u8_slice_with_stride(
                slice::from_raw_parts(
                    (*raw_frame).Data[0],
                    (*raw_frame).Linesize[0] as usize * self.video_details.height,
                ),
                (*raw_frame).Linesize[0] as usize,
            )
        }
        .map_err(|e| DecoderError::GenericDecodeError {
            cause: format!("Failed to copy Y-plane data: {e}"),
        })?;
        if let Some(u_plane) = frame.u_plane.as_mut() {
            // SAFETY: we assume that the values provided by FFMS2 are correct
            unsafe {
                u_plane.copy_from_u8_slice_with_stride(
                    slice::from_raw_parts(
                        (*raw_frame).Data[1],
                        (*raw_frame).Linesize[1] as usize * chroma_height,
                    ),
                    (*raw_frame).Linesize[1] as usize,
                )
            }
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: format!("Failed to copy U-plane data: {e}"),
            })?;
        }
        if let Some(v_plane) = frame.v_plane.as_mut() {
            // SAFETY: we assume that the values provided by FFMS2 are correct
            unsafe {
                v_plane.copy_from_u8_slice_with_stride(
                    slice::from_raw_parts(
                        (*raw_frame).Data[2],
                        (*raw_frame).Linesize[2] as usize * chroma_height,
                    ),
                    (*raw_frame).Linesize[2] as usize,
                )
            }
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: format!("Failed to copy V-plane data: {e}"),
            })?;
        }

        Ok(frame)
    }

    /// Returns the track index of the video.
    #[inline]
    #[must_use]
    pub fn get_track_index(&self) -> i32 {
        self.index_handle.track
    }

    /// Returns the path of the video file this decoder was opened from.
    #[inline]
    #[must_use]
    pub fn get_video_path(&self) -> &Path {
        Path::new(&self.index_handle.path)
    }

    /// Returns the `.ffindex` file this decoder's index was read from or
    /// written to.
    #[inline]
    #[must_use]
    pub fn get_index_file(&self) -> &Path {
        &self.index_handle.index_path
    }
}

/// How an index file that cannot be written should be treated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IndexWriteFailure {
    /// Unrecoverable failure.
    Fatal,
    /// Report progress and carry on decoding from the in-memory index, so a
    /// read-only media directory is not a hard failure.
    Tolerated,
}

/// Ensure FFMS2 is initialized. Only called once per process.
fn ensure_ffms2_init() {
    FFMS2_INIT.call_once(|| {
        // SAFETY: FFI call with infallible parameters
        unsafe {
            FFMS_Init(0, 0);
        }
    });
}

/// FFMS2's default index location: the source path with `.ffindex` appended.
fn default_index_path(input: &Path) -> PathBuf {
    let mut path = input.to_path_buf().into_os_string();
    path.push(".ffindex");
    PathBuf::from(path)
}

/// Context shared with the FFMS2 progress trampoline for the duration of
/// `FFMS_DoIndexing2` (a synchronous call on this thread).
///
/// Two lifetimes rather than one: `&mut` is invariant over its lifetime, so the
/// two borrows cannot be forced to a common one without rejecting inputs whose
/// callback borrow is longer than the cancel closure's, or vice versa.
struct ProgressContext<'progress, 'cancelled> {
    progress: Option<IndexProgress<'progress>>,
    cancelled: Option<IndexCancelled<'cancelled>>,
}

impl ProgressContext<'_, '_> {
    /// Whether either callback needs the FFMS2 trampoline installed at all.
    fn has_callbacks(&self) -> bool {
        self.progress.is_some() || self.cancelled.is_some()
    }
}

/// Trampoline the FFMS2 indexer calls with the demuxer's byte position and the
/// source size. Returning non-zero asks FFMS2 to cancel indexing, which it then
/// reports as an `FFMS_ERROR_CANCELLED` failure from `FFMS_DoIndexing2`.
///
/// This is called from FFMS2's C++ stack. A panic in the user callback would
/// abort the process rather than unwind, so callbacks must not panic.
unsafe extern "C" fn progress_trampoline(
    current: i64,
    total: i64,
    opaque: *mut c_void,
) -> std::os::raw::c_int {
    // SAFETY: `opaque` is the `ProgressContext` borrowed for exactly the length
    // of the `FFMS_DoIndexing2` call that owns this callback.
    let context = unsafe { &mut *opaque.cast::<ProgressContext<'_, '_>>() };

    if let Some(cancelled) = context.cancelled
        && cancelled()
    {
        return 1;
    }

    if let Some(progress) = context.progress.as_mut()
        && let Some(percent) = index_progress_percent(current, total)
    {
        (**progress)(percent);
    }

    0
}

// FFmpeg pixel format constants (from libavutil/pixfmt.h)
// These are used to interpret FFMS_Frame::ConvertedPixelFormat values
// Using `FFMS_GetPixFmt` ensures we have the correct value regardless
// of the ffmpeg version we are linked against
//
// These are stored in `LazyLock`s because they can be frequently called,
// which should be a fast operation, but because it is an FFI,
// would always invoke an expensive `call` operation`.
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
    chroma_subsampling: ChromaSubsampling,
) -> Result<i32, DecoderError> {
    Ok(match (bit_depth, chroma_subsampling) {
        // 8-bit formats
        (8, ChromaSubsampling::Yuv420) => *AV_PIX_FMT_YUV420P,
        (8, ChromaSubsampling::Yuv422) => *AV_PIX_FMT_YUV422P,
        (8, ChromaSubsampling::Yuv444) => *AV_PIX_FMT_YUV444P,
        (8, ChromaSubsampling::Monochrome) => *AV_PIX_FMT_GRAY8,

        // 10-bit formats
        (10, ChromaSubsampling::Yuv420) => *AV_PIX_FMT_YUV420P10LE,
        (10, ChromaSubsampling::Yuv422) => *AV_PIX_FMT_YUV422P10LE,
        (10, ChromaSubsampling::Yuv444) => *AV_PIX_FMT_YUV444P10LE,
        (10, ChromaSubsampling::Monochrome) => *AV_PIX_FMT_GRAY10LE,

        // 12-bit formats
        (12, ChromaSubsampling::Yuv420) => *AV_PIX_FMT_YUV420P12LE,
        (12, ChromaSubsampling::Yuv422) => *AV_PIX_FMT_YUV422P12LE,
        (12, ChromaSubsampling::Yuv444) => *AV_PIX_FMT_YUV444P12LE,
        (12, ChromaSubsampling::Monochrome) => *AV_PIX_FMT_GRAY12LE,

        _ => {
            return Err(DecoderError::UnsupportedFormat {
                fmt: "Unsupported bit depth and subsampling combination".to_string(),
            });
        }
    })
}

/// Converts an FFMS2 byte position into a whole percentage, `0..=100`.
///
/// Returns `None` when `total` is not positive, which means FFMS2 could not
/// determine the source size, so there is no meaningful percentage to report.
fn index_progress_percent(current: i64, total: i64) -> Option<u8> {
    if total <= 0 {
        return None;
    }

    // `current` can over-report on the final read and be negative on odd
    // demuxers, so both ends are clamped.
    let percent = (current.max(0) as u128 * 100 / total as u128).min(100);
    Some(percent as u8)
}

const ERR_BUFFER_SIZE: usize = 1024;

/// Builds an `FFMS_ErrorInfo` that writes into the caller-owned stack buffer.
fn empty_error_info(buffer: &mut [c_char; ERR_BUFFER_SIZE]) -> FFMS_ErrorInfo {
    FFMS_ErrorInfo {
        ErrorType: 0,
        SubType: 0,
        BufferSize: ERR_BUFFER_SIZE as i32,
        Buffer: buffer.as_mut_ptr(),
    }
}

/// Extracts the error message from an `FFMS_ErrorInfo` struct.
///
/// # Safety
/// `err` must have been populated by an FFMS2 function call while its buffer is still alive.
fn get_error_message(err: FFMS_ErrorInfo) -> String {
    if err.Buffer.is_null() {
        return "Unknown error".to_string();
    }

    // SAFETY: FFMS2 writes a nul-terminated C string into the live error buffer.
    unsafe { std::ffi::CStr::from_ptr(err.Buffer) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_error_info_points_to_caller_owned_buffer() {
        let mut buffer: [c_char; ERR_BUFFER_SIZE] = [0; ERR_BUFFER_SIZE];

        let err = empty_error_info(&mut buffer);

        assert_eq!(err.ErrorType, 0);
        assert_eq!(err.SubType, 0);
        assert_eq!(err.BufferSize, ERR_BUFFER_SIZE as i32);
        assert_eq!(err.Buffer, buffer.as_mut_ptr());
    }

    #[test]
    fn default_index_path_appends_ffindex() {
        assert_eq!(
            default_index_path(Path::new("clip.mkv")),
            PathBuf::from("clip.mkv.ffindex")
        );
        assert_eq!(
            default_index_path(Path::new("dir/clip.mkv")),
            PathBuf::from("dir/clip.mkv.ffindex")
        );
    }

    #[test]
    fn percentage_is_clamped_to_a_valid_range() {
        assert_eq!(index_progress_percent(0, 1_000), Some(0));
        assert_eq!(index_progress_percent(1, 2), Some(50));
        assert_eq!(index_progress_percent(1_000, 1_000), Some(100));
        // FFMS2 can over-report on the final read, so the clamp matters.
        assert_eq!(index_progress_percent(2_000, 1_000), Some(100));
        assert_eq!(index_progress_percent(-1, 1_000), Some(0));
        assert_eq!(index_progress_percent(i64::MAX, 1), Some(100));
        // No size, no meaningful percentage.
        assert_eq!(index_progress_percent(500, 0), None);
        assert_eq!(index_progress_percent(500, -10), None);
    }

    /// A scratch directory holding one fixture video plus an index path that
    /// does not exist yet, removed on drop so nothing is left behind.
    struct Scratch {
        dir: PathBuf,
        video: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "av-decoders-ffms2-{}-{}",
                std::process::id(),
                name
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch directory");

            let video = dir.join("tt_sif.y4m");
            std::fs::copy("test_files/tt_sif.y4m", &video).expect("copy the y4m fixture");

            Self { dir, video }
        }

        fn index_path(&self) -> PathBuf {
            self.dir.join("custom.ffindex")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn build_index_reports_progress_and_reuses_the_cache() {
        let scratch = Scratch::new("build");
        let index = scratch.index_path();

        let mut seen = Vec::new();
        let built = Ffms2Decoder::build_index(
            &scratch.video,
            Some(&index),
            Some(&mut |percent| seen.push(percent)),
            None,
        )
        .expect("the fixture indexes");

        assert!(built, "a missing index should be built");
        assert!(index.exists(), "the index should have been written");
        assert_eq!(seen.first(), Some(&0), "progress starts at 0");
        assert_eq!(seen.last(), Some(&100), "progress ends at 100");
        assert!(seen.iter().all(|&percent| percent <= 100));

        // A second run finds the cache valid and brackets without indexing.
        let mut second = Vec::new();
        let rebuilt = Ffms2Decoder::build_index(
            &scratch.video,
            Some(&index),
            Some(&mut |percent| second.push(percent)),
            None,
        )
        .expect("the cached index is reused");

        assert!(!rebuilt, "a valid index should not be rebuilt");
        assert_eq!(second, vec![0, 100]);
    }

    #[test]
    fn build_index_rebuilds_a_foreign_index() {
        let scratch = Scratch::new("stale");
        let index = scratch.index_path();
        std::fs::write(&index, b"not an ffms2 index").expect("write a bogus index");

        let rebuilt =
            Ffms2Decoder::build_index(&scratch.video, Some(&index), None, None).expect("rebuild");

        assert!(rebuilt, "an unreadable index should be rebuilt");
        assert!(std::fs::metadata(&index).expect("index exists").len() > 16);
    }

    #[test]
    fn build_index_honours_a_cancellation_request() {
        let scratch = Scratch::new("cancel");
        let index = scratch.index_path();

        let error = Ffms2Decoder::build_index(&scratch.video, Some(&index), None, Some(&|| true))
            .expect_err("indexing should be cancelled");

        assert!(
            matches!(error, DecoderError::IndexingCancelled { .. }),
            "expected IndexingCancelled, got {error:?}"
        );
        assert!(!index.exists(), "no index should be written when cancelled");
    }

    #[test]
    fn with_index_builds_the_index_and_decodes() {
        let scratch = Scratch::new("with-index");
        let index = scratch.index_path();

        let mut seen = Vec::new();
        let mut decoder = Ffms2Decoder::with_index(
            &scratch.video,
            &index,
            None,
            Some(&mut |percent| seen.push(percent)),
        )
        .expect("open the fixture through a custom index");

        assert!(index.exists(), "with_index builds a missing index");
        assert_eq!(seen.first(), Some(&0));
        assert_eq!(seen.last(), Some(&100));
        assert_eq!(decoder.get_index_file(), index.as_path());

        let frame = decoder
            .read_video_frame::<u8>(0, false)
            .expect("decode the first frame");
        assert_eq!(frame.y_plane.height(), decoder.video_details.height);
        assert_eq!(frame.y_plane.width(), decoder.video_details.width);

        // Reopening reuses the index and still brackets the range.
        let mut second = Vec::new();
        Ffms2Decoder::with_index(
            &scratch.video,
            &index,
            None,
            Some(&mut |percent| second.push(percent)),
        )
        .expect("reopen through the cached index");

        assert_eq!(second, vec![0, 100]);
    }

    #[test]
    fn with_index_keeps_indexes_per_path() {
        let scratch = Scratch::new("per-path");
        let first = scratch.index_path();
        let second = scratch.dir.join("other.ffindex");

        Ffms2Decoder::build_index(&scratch.video, Some(&first), None, None).expect("build first");
        Ffms2Decoder::build_index(&scratch.video, Some(&second), None, None).expect("build second");

        assert!(first.exists() && second.exists());

        let mut decoder =
            Ffms2Decoder::with_index(&scratch.video, &second, None, None).expect("open second");
        assert_eq!(decoder.get_index_file(), second.as_path());
        assert!(
            decoder.read_video_frame::<u8>(0, false).is_ok(),
            "the second index decodes"
        );
    }

    #[test]
    fn track_index_out_of_range_is_rejected() {
        let scratch = Scratch::new("bad-track");
        let index = scratch.index_path();

        let Err(error) = Ffms2Decoder::with_index(&scratch.video, &index, Some(u8::MAX), None)
        else {
            panic!("an out-of-range track must not be dereferenced");
        };

        assert!(
            matches!(error, DecoderError::NoVideoStream),
            "expected NoVideoStream, got {error:?}"
        );
    }

    #[test]
    fn toggles_between_fatal_and_tolerated_index_write_failure() {
        let scratch = Scratch::new("unwritable");
        // The parent directory does not exist, so the index cannot be written
        // even though indexing the readable source file succeeds.
        let unwritable = scratch.dir.join("missing-dir").join("custom.ffindex");

        // `build_index` exists to produce that file, so it reports the failure.
        let Err(error) = Ffms2Decoder::build_index(&scratch.video, Some(&unwritable), None, None)
        else {
            panic!("an unwritable index path is fatal for build_index");
        };
        assert!(
            matches!(error, DecoderError::GenericDecodeError { .. }),
            "expected GenericDecodeError, got {error:?}"
        );

        // `with_index` only needs a decodable source, so it proceeds from the
        // in-memory index instead of failing.
        let mut decoder = Ffms2Decoder::with_index(&scratch.video, &unwritable, None, None)
            .expect("an unwritable cache must not break decoding");
        assert!(decoder.read_video_frame::<u8>(0, false).is_ok());
        assert!(!unwritable.exists());
    }
}
