use crate::error::DecoderError;
use crate::{LUMA_PADDING, VideoDetails};
use num_rational::Rational32;
use std::{
    collections::HashMap,
    num::{NonZeroU8, NonZeroUsize},
    path::Path,
    slice,
};
use v_frame::{
    chroma::ChromaSubsampling,
    frame::{Frame, FrameBuilder},
    pixel::Pixel,
};
use vapoursynth::{
    api::API,
    core::CoreRef,
    map::OwnedMap,
    node::Node,
    video_info::{Property, VideoInfo},
    vsscript::{Environment, EvalFlags},
};

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
        let env = Environment::new().map_err(|e| match e {
            vapoursynth::vsscript::Error::CStringConversion(_)
            | vapoursynth::vsscript::Error::FileOpen(_)
            | vapoursynth::vsscript::Error::FileRead(_)
            | vapoursynth::vsscript::Error::PathInvalidUnicode => DecoderError::FileReadError {
                cause: e.to_string(),
            },
            vapoursynth::vsscript::Error::VSScript(vsscript_error) => DecoderError::FileReadError {
                cause: vsscript_error.to_string(),
            },
            vapoursynth::vsscript::Error::NoSuchVariable
            | vapoursynth::vsscript::Error::NoCore
            | vapoursynth::vsscript::Error::NoOutput
            | vapoursynth::vsscript::Error::NoAPI
            | vapoursynth::vsscript::Error::ScriptCreationFailed => {
                DecoderError::VapoursynthInternalError {
                    cause: e.to_string(),
                }
            }
        })?;
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
            .map_err(|e| match e {
                vapoursynth::vsscript::Error::CStringConversion(_)
                | vapoursynth::vsscript::Error::FileOpen(_)
                | vapoursynth::vsscript::Error::FileRead(_)
                | vapoursynth::vsscript::Error::PathInvalidUnicode => DecoderError::FileReadError {
                    cause: e.to_string(),
                },
                vapoursynth::vsscript::Error::VSScript(vsscript_error) => {
                    DecoderError::FileReadError {
                        cause: vsscript_error.to_string(),
                    }
                }
                vapoursynth::vsscript::Error::NoSuchVariable
                | vapoursynth::vsscript::Error::NoCore
                | vapoursynth::vsscript::Error::NoOutput
                | vapoursynth::vsscript::Error::NoAPI
                | vapoursynth::vsscript::Error::ScriptCreationFailed => {
                    DecoderError::VapoursynthInternalError {
                        cause: e.to_string(),
                    }
                }
            })?;
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
        decoder.get_env().eval_script(script).map_err(|e| match e {
            vapoursynth::vsscript::Error::CStringConversion(_)
            | vapoursynth::vsscript::Error::FileOpen(_)
            | vapoursynth::vsscript::Error::FileRead(_)
            | vapoursynth::vsscript::Error::PathInvalidUnicode => DecoderError::FileReadError {
                cause: e.to_string(),
            },
            vapoursynth::vsscript::Error::VSScript(vsscript_error) => DecoderError::FileReadError {
                cause: vsscript_error.to_string(),
            },
            vapoursynth::vsscript::Error::NoSuchVariable
            | vapoursynth::vsscript::Error::NoCore
            | vapoursynth::vsscript::Error::NoOutput
            | vapoursynth::vsscript::Error::NoAPI
            | vapoursynth::vsscript::Error::ScriptCreationFailed => {
                DecoderError::VapoursynthInternalError {
                    cause: e.to_string(),
                }
            }
        })?;
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

    pub(crate) fn get_video_details(&self) -> Result<VideoDetails, DecoderError> {
        match self.video_details {
            Some(details) => Ok(details),
            None => {
                let node = self.get_output_node();
                let details = parse_video_details(node.info())?;
                Ok(details)
            }
        }
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

        let node = {
            let output_node = match self.env.get_output(self.output_index) {
                Ok(output) => {
                    let (output_node, _) = output;
                    Some(output_node)
                }
                Err(vapoursynth::vsscript::Error::NoOutput) => {
                    if self.modify_node.is_some() {
                        None
                    } else {
                        panic!("output node exists--validated during initialization");
                    }
                }
                Err(_) => panic!("unexpected error when getting output node"),
            };
            if let Some(modify_node) = self.modify_node.as_ref() {
                let core =
                    self.env
                        .get_core()
                        .map_err(|e| DecoderError::VapoursynthInternalError {
                            cause: e.to_string(),
                        })?;
                modify_node(core, output_node).map_err(|e| {
                    DecoderError::VapoursynthInternalError {
                        cause: e.to_string(),
                    }
                })?
            } else {
                output_node.expect("output node exists--validated during initialization")
            }
        };

        // Lazy load the total frame count
        if self.video_details.is_none() {
            let video_details = parse_video_details(node.info())?;
            self.video_details = Some(video_details);
        }

        let vs_frame = node
            .get_frame(frame_index)
            .map_err(|_| DecoderError::EndOfFile)?;

        let mut frame: Frame<T> = FrameBuilder::new(
            NonZeroUsize::new(cfg.width).ok_or_else(|| DecoderError::GenericDecodeError {
                cause: "Zero-width resolution is not supported".to_string(),
            })?,
            NonZeroUsize::new(cfg.height).ok_or_else(|| DecoderError::GenericDecodeError {
                cause: "Zero-height resolution is not supported".to_string(),
            })?,
            if luma_only {
                ChromaSubsampling::Monochrome
            } else {
                cfg.chroma_sampling
            },
            NonZeroU8::new(cfg.bit_depth as u8).ok_or_else(|| {
                DecoderError::GenericDecodeError {
                    cause: "Zero-bit-depth is not supported".to_string(),
                }
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
                NonZeroUsize::new(vs_frame.stride(0)).expect("zero stride should be impossible"),
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
                    NonZeroUsize::new(vs_frame.stride(1))
                        .expect("zero stride should be impossible"),
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
                    NonZeroUsize::new(vs_frame.stride(2))
                        .expect("zero stride should be impossible"),
                )
                .map_err(|e| DecoderError::GenericDecodeError {
                    cause: e.to_string(),
                })?;
        }

        Ok(frame)
    }

    /// Returns an immutable reference to the VapourSynth environment.
    pub(crate) fn get_environment(&self) -> &Environment {
        &self.env
    }

    /// Returns a mutable reference to the VapourSynth environment.
    pub(crate) fn get_env(&mut self) -> &mut Environment {
        &mut self.env
    }

    /// Returns the VapourSynth output node, applying the registered modifier if any.
    pub(crate) fn get_output_node(&self) -> Node<'_> {
        let output_node = match self.env.get_output(self.output_index) {
            Ok(output) => {
                let (output_node, _) = output;
                Some(output_node)
            }
            Err(vapoursynth::vsscript::Error::NoOutput) => {
                if self.modify_node.is_some() {
                    None
                } else {
                    panic!("output node does not exist");
                }
            }
            Err(_) => panic!("unexpected error when getting output node"),
        };
        if let Some(modify_node) = self.modify_node.as_ref() {
            let core = self
                .env
                .get_core()
                .expect("core exists--validated during initialization");
            modify_node(core, output_node)
                .expect("modified node exists--validated during registration")
        } else {
            output_node.expect("output node exists--validated during initialization")
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
    })
}
