use crate::VideoDetails;
use crate::error::DecoderError;
use num_rational::Rational32;
use std::{collections::HashMap, path::Path, slice};
use v_frame::{chroma::ChromaSubsampling, frame::Frame, pixel::Pixel};
use vapoursynth::{
    api::API,
    core::CoreRef,
    map::OwnedMap,
    node::Node,
    video_info::{Property, VideoInfo},
    vsscript::{Environment, Error as VsscriptError, EvalFlags},
};

use super::frame_builder::new_padded_frame;

const DEFAULT_OUTPUT_INDEX: i32 = 0;

/// Callback to modify the VapourSynth output node before frame decoding.
///
/// Receives the `CoreRef` and the initial output node; must return the modified node.
pub type ModifyNode = Box<
    dyn for<'core> Fn(CoreRef<'core>, Option<Node<'core>>) -> Result<Node<'core>, DecoderError>
        + 'static,
>;
/// The number of frames in the output video node.
pub type TotalFrames = usize;
// The width of the output video node.
pub type Width = usize;
// The height of the output video node.
pub type Height = usize;
// The bit depth of the output video node.
pub type BitDepth = usize;
// The name of the variable to set in the VapourSynth environment.
pub type VariableName = String;
// The value of the variable to set in the VapourSynth environment.
pub type VariableValue = String;

fn map_vsscript_error(error: &VsscriptError) -> DecoderError {
    match error {
        VsscriptError::CStringConversion(_)
        | VsscriptError::FileOpen(_)
        | VsscriptError::FileRead(_)
        | VsscriptError::PathInvalidUnicode => DecoderError::FileReadError {
            cause: error.to_string(),
        },
        VsscriptError::VSScript(vsscript_error) => DecoderError::FileReadError {
            cause: vsscript_error.to_string(),
        },
        VsscriptError::NoSuchVariable
        | VsscriptError::NoCore
        | VsscriptError::NoOutput
        | VsscriptError::NoAPI
        | VsscriptError::ScriptCreationFailed => DecoderError::VapoursynthInternalError {
            cause: error.to_string(),
        },
    }
}

/// An interface that is used for decoding a video stream using Vapoursynth
pub struct VapoursynthDecoder {
    #[allow(missing_docs)]
    pub env: Environment,
    #[allow(missing_docs)]
    modify_node: Option<ModifyNode>,
    video_details: Option<VideoDetails>,
    output_index: i32,
}

impl VapoursynthDecoder {
    /// Creates a new decoder with an empty VapourSynth environment.
    ///
    /// A valid output node must be registered via [`register_node_modifier`](Self::register_node_modifier)
    /// before decoding frames.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::VapoursynthInternalError`] if the VapourSynth environment
    /// cannot be initialized.
    #[inline]
    pub fn new() -> Result<VapoursynthDecoder, DecoderError> {
        let env = Environment::new().map_err(|e| map_vsscript_error(&e))?;
        Ok(Self {
            env,
            modify_node: None,
            video_details: None,
            output_index: DEFAULT_OUTPUT_INDEX,
        })
    }

    /// Creates a decoder from a VapourSynth script file (`.vpy`).
    ///
    /// The working directory is set to the directory containing the script.
    /// Pass `HashMap::new()` for `variables` if none are needed; `output_index` defaults to 0.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError`] if the file cannot be read, the script fails to execute,
    /// or the output has variable format/resolution/framerate.
    #[inline]
    pub fn from_file<P: AsRef<Path>>(
        input: P,
        variables: HashMap<VariableName, VariableValue>,
        output_index: Option<u8>,
    ) -> Result<VapoursynthDecoder, DecoderError> {
        let mut decoder = Self::new()?;
        decoder.set_variables(variables)?;
        if let Some(index) = output_index {
            decoder.output_index = index as i32;
        }
        decoder
            .get_env()
            .eval_file(input, EvalFlags::SetWorkingDir)
            .map_err(|e| map_vsscript_error(&e))?;
        Ok(decoder)
    }

    /// Creates a decoder from a VapourSynth script string.
    ///
    /// The script must call `clip.set_output()`. Pass `HashMap::new()` for `variables`
    /// if none are needed; `output_index` defaults to 0.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError`] if the script fails to execute, or the output has
    /// variable format/resolution/framerate.
    #[inline]
    pub fn from_script(
        script: &str,
        variables: HashMap<VariableName, VariableValue>,
        output_index: Option<u8>,
    ) -> Result<VapoursynthDecoder, DecoderError> {
        let mut decoder = Self::new()?;
        decoder.set_variables(variables)?;
        if let Some(index) = output_index {
            decoder.output_index = index as i32;
        }
        decoder
            .get_env()
            .eval_script(script)
            .map_err(|e| map_vsscript_error(&e))?;
        Ok(decoder)
    }

    /// Sets variables in the VapourSynth environment, accessible from scripts via `vs.get_output()`.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::VapoursynthArgsError`] if a variable cannot be set.
    #[inline]
    pub fn set_variables(
        &mut self,
        variables: HashMap<VariableName, VariableValue>,
    ) -> Result<(), DecoderError> {
        let api = API::get().ok_or_else(|| DecoderError::VapoursynthInternalError {
            cause: "failed to get Vapoursynth API instance".to_string(),
        })?;
        let mut variables_map = OwnedMap::new(api);

        for (name, value) in variables {
            variables_map
                .set_data(name.as_str(), value.as_bytes())
                .map_err(|e| DecoderError::VapoursynthArgsError {
                    cause: e.to_string(),
                })?;
        }

        self.env
            .set_variables(&variables_map)
            .map_err(|e| DecoderError::VapoursynthArgsError {
                cause: e.to_string(),
            })
    }

    /// Returns the video metadata.
    ///
    /// Color metadata is only available via
    /// [`get_extended_video_details`](Self::get_extended_video_details).
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::VapoursynthInternalError`] if the output node
    /// cannot be obtained.
    #[inline]
    pub fn get_video_details(&self) -> Result<VideoDetails, DecoderError> {
        match self.video_details {
            Some(details) => Ok(details),
            None => {
                let node = self.get_output_node()?;
                parse_video_details(node.info())
            }
        }
    }

    /// Returns the video metadata with color information filled in.
    ///
    /// Reads the first frame's VapourSynth properties (`_Range`/`_ColorRange`,
    /// `_Primaries`, `_Transfer`, `_Matrix`, `_ChromaLocation`). Properties that
    /// are absent or hold unknown values are reported as `None`. HDR metadata is
    /// not read.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::EndOfFile`] if the first frame cannot be decoded,
    /// [`DecoderError::VapoursynthInternalError`] if the output node cannot be
    /// obtained.
    #[inline]
    pub fn get_extended_video_details(&self) -> Result<VideoDetails, DecoderError> {
        let node = self.get_output_node()?;
        let mut details = match self.video_details {
            Some(details) => details,
            None => parse_video_details(node.info())?,
        };

        let frame = node.get_frame(0).map_err(|_| DecoderError::EndOfFile)?;

        let props = frame.props();
        // `_Range` supersedes the deprecated `_ColorRange`.
        details.color_range = get_int_prop(&props, "_Range")
            .and_then(crate::color::ColorRange::from_vapoursynth_range)
            .or_else(|| {
                get_int_prop(&props, "_ColorRange")
                    .and_then(crate::color::ColorRange::from_vapoursynth_color_range)
            });
        details.color_primaries =
            get_int_prop(&props, "_Primaries").and_then(crate::color::ColorPrimaries::from_raw);
        details.color_transfer_characteristic = get_int_prop(&props, "_Transfer")
            .and_then(crate::color::TransferCharacteristic::from_raw);
        details.color_space =
            get_int_prop(&props, "_Matrix").and_then(crate::color::ColorSpace::from_raw);
        details.chroma_location = get_int_prop(&props, "_ChromaLocation")
            .and_then(crate::color::ChromaLocation::from_vapoursynth_raw);

        Ok(details)
    }

    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
        frame_index: usize,
        luma_only: bool,
    ) -> Result<Frame<T>, DecoderError> {
        if self.video_details.is_some_and(|details| {
            details
                .total_frames
                .is_some_and(|total_frames| frame_index >= total_frames)
        }) {
            return Err(DecoderError::EndOfFile);
        }

        // Lazy load the total frame count.
        if self.video_details.is_none() {
            let node =
                Self::get_output_from(&self.env, self.output_index, self.modify_node.as_ref())?;
            self.video_details = Some(parse_video_details(node.info())?);
        }

        let node = self.get_output_node()?;

        let vs_frame = node
            .get_frame(frame_index)
            .map_err(|_| DecoderError::EndOfFile)?;

        let mut frame: Frame<T> = new_padded_frame(cfg, luma_only)?;

        frame
            .y_plane
            .copy_from_u8_slice_with_stride(
                // SAFETY: we assume that the values provided by VapourSynth are correct
                unsafe {
                    slice::from_raw_parts(
                        vs_frame.data_ptr(0),
                        vs_frame.stride(0) * vs_frame.height(0),
                    )
                },
                vs_frame.stride(0),
            )
            .map_err(|e| DecoderError::GenericDecodeError {
                cause: e.to_string(),
            })?;
        if let Some(u_plane) = frame.u_plane.as_mut() {
            u_plane
                .copy_from_u8_slice_with_stride(
                    // SAFETY: we assume that the values provided by VapourSynth are correct
                    unsafe {
                        slice::from_raw_parts(
                            vs_frame.data_ptr(1),
                            vs_frame.stride(1) * vs_frame.height(1),
                        )
                    },
                    vs_frame.stride(1),
                )
                .map_err(|e| DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                })?;
        }
        if let Some(v_plane) = frame.v_plane.as_mut() {
            v_plane
                .copy_from_u8_slice_with_stride(
                    // SAFETY: we assume that the values provided by VapourSynth are correct
                    unsafe {
                        slice::from_raw_parts(
                            vs_frame.data_ptr(2),
                            vs_frame.stride(2) * vs_frame.height(2),
                        )
                    },
                    vs_frame.stride(2),
                )
                .map_err(|e| DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                })?;
        }

        Ok(frame)
    }

    /// Returns a mutable reference to the VapourSynth environment.
    pub(crate) fn get_env(&mut self) -> &mut Environment {
        &mut self.env
    }

    /// Returns the VapourSynth output node, applying the registered modifier if any.
    ///
    /// # Errors
    ///
    /// Forwards errors from [`get_output`](Self::get_output).
    pub(crate) fn get_output_node(&self) -> Result<Node<'_>, DecoderError> {
        self.get_output(self.output_index, self.modify_node.as_ref())
    }

    /// Returns the index of the VapourSynth output node.
    #[inline]
    #[must_use]
    pub fn get_output_index(&self) -> i32 {
        self.output_index
    }

    /// Returns a reference to the registered modifier callback, if any.
    #[inline]
    #[must_use]
    pub fn get_node_modifier(&self) -> Option<&ModifyNode> {
        self.modify_node.as_ref()
    }

    /// Returns the VapourSynth output node at the specified index,
    /// modifying it with the optional callback.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::VapoursynthInternalError`] if the core cannot be obtained
    /// or no output node exists. Propagates any error from the callback itself.
    #[inline]
    pub fn get_output(
        &self,
        index: i32,
        modify_node: Option<&ModifyNode>,
    ) -> Result<Node<'_>, DecoderError> {
        Self::get_output_from(&self.env, index, modify_node)
    }

    /// Same as [`get_output`](Self::get_output) but borrows only the environment.
    fn get_output_from<'a>(
        env: &'a Environment,
        index: i32,
        modify_node: Option<&ModifyNode>,
    ) -> Result<Node<'a>, DecoderError> {
        let output_node = match env.get_output(index) {
            Ok(output) => {
                let (output_node, _) = output;
                Ok(Some(output_node))
            }
            Err(vapoursynth::vsscript::Error::NoOutput) => {
                if modify_node.is_some() {
                    Ok(None)
                } else {
                    Err(map_vsscript_error(&vapoursynth::vsscript::Error::NoOutput))
                }
            }
            Err(e) => Err(map_vsscript_error(&e)),
        }?;
        if let Some(modify_node) = modify_node {
            let core = env
                .get_core()
                .map_err(|e| DecoderError::VapoursynthInternalError {
                    cause: e.to_string(),
                })?;
            modify_node(core, output_node)
        } else {
            output_node.map_or_else(
                || Err(map_vsscript_error(&vapoursynth::vsscript::Error::NoOutput)),
                Ok,
            )
        }
    }

    /// Registers a callback to modify the VapourSynth output node before each frame decode.
    ///
    /// The callback is invoked with the `CoreRef` and current output node, and must return
    /// the modified node. The returned node is used for decoding and its metadata is cached.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::VapoursynthInternalError`] if the core cannot be obtained
    /// or no output node exists. Propagates any error from the callback itself.
    #[inline]
    pub fn register_node_modifier(
        &mut self,
        modify_node: ModifyNode,
    ) -> Result<Node<'_>, DecoderError> {
        let core = self
            .env
            .get_core()
            .map_err(|e| DecoderError::VapoursynthInternalError {
                cause: e.to_string(),
            })?;

        let output_node = {
            let res = self.env.get_output(self.output_index);
            match res {
                Ok((node, _)) => Some(node),
                Err(vapoursynth::vsscript::Error::NoOutput) => None,
                Err(e) => {
                    return Err(DecoderError::VapoursynthInternalError {
                        cause: e.to_string(),
                    });
                }
            }
        };
        let modified_node = modify_node(core, output_node)?;

        // Set the updated video details and total frames
        let video_details = parse_video_details(modified_node.info())?;
        self.video_details = Some(video_details);
        // Register the node modifier to be used during read_video_frame
        self.modify_node = Some(modify_node);

        Ok(modified_node)
    }
}

/// Extracts frame count from `VideoInfo`; rejects variable/zero-length streams.
fn get_num_frames(info: VideoInfo) -> Result<TotalFrames, DecoderError> {
    let num_frames = {
        if Property::Variable == info.resolution {
            return Err(DecoderError::VariableResolution);
        }
        if Property::Variable == info.framerate {
            return Err(DecoderError::VariableFramerate);
        }

        info.num_frames
    };

    if num_frames == 0 {
        return Err(DecoderError::EndOfFile);
    }

    Ok(num_frames)
}

/// Extracts bit depth from `VideoInfo`.
fn get_bit_depth(info: VideoInfo) -> Result<BitDepth, DecoderError> {
    let bits_per_sample = info.format.bits_per_sample();

    Ok(bits_per_sample as usize)
}

/// Extracts resolution from `VideoInfo`; rejects variable resolution.
fn get_resolution(info: VideoInfo) -> Result<(Width, Height), DecoderError> {
    let resolution = {
        match info.resolution {
            Property::Variable => {
                return Err(DecoderError::VariableResolution);
            }
            Property::Constant(x) => x,
        }
    };

    Ok((resolution.width, resolution.height))
}

/// Extracts frame rate from `VideoInfo`; rejects variable framerate.
fn get_frame_rate(info: VideoInfo) -> Result<Rational32, DecoderError> {
    match info.framerate {
        Property::Variable => Err(DecoderError::VariableFramerate),
        Property::Constant(fps) => Ok(Rational32::new(
            fps.numerator as i32,
            fps.denominator as i32,
        )),
    }
}

/// Extracts chroma subsampling from `VideoInfo`.
fn get_chroma_sampling(info: VideoInfo) -> Result<ChromaSubsampling, DecoderError> {
    let format = info.format;
    match format.color_family() {
        vapoursynth::format::ColorFamily::YUV => {
            let ss = (format.sub_sampling_w(), format.sub_sampling_h());
            match ss {
                (1, 1) => Ok(ChromaSubsampling::Yuv420),
                (1, 0) => Ok(ChromaSubsampling::Yuv422),
                (0, 0) => Ok(ChromaSubsampling::Yuv444),
                (x, y) => Err(DecoderError::UnsupportedChromaSubsampling {
                    x: x.into(),
                    y: y.into(),
                }),
            }
        }
        vapoursynth::format::ColorFamily::Gray => Ok(ChromaSubsampling::Monochrome),
        fmt => Err(DecoderError::UnsupportedFormat {
            fmt: fmt.to_string(),
        }),
    }
}

/// Parses all video metadata from a VapourSynth `VideoInfo`.
fn parse_video_details(info: VideoInfo) -> Result<VideoDetails, DecoderError> {
    let total_frames = get_num_frames(info)?;
    let (width, height) = get_resolution(info)?;
    Ok(VideoDetails {
        width,
        height,
        bit_depth: get_bit_depth(info)?,
        chroma_sampling: get_chroma_sampling(info)?,
        frame_rate: get_frame_rate(info)?,
        total_frames: Some(total_frames),
        // Color metadata requires decoding a frame; see `get_extended_video_details`
        color_range: None,
        color_primaries: None,
        color_transfer_characteristic: None,
        color_space: None,
        chroma_location: None,
        mastering_display: None,
        content_light_level: None,
    })
}

/// Reads an integer frame property, returning `None` when the key is absent.
fn get_int_prop(props: &vapoursynth::map::MapRef<'_, '_>, key: &str) -> Option<i64> {
    props.get_int(key).ok()
}
