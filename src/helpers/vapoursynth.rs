use crate::error::DecoderError;
use crate::VideoDetails;
use num_rational::Rational32;
use std::{collections::HashMap, mem::size_of, path::Path, slice};
use v_frame::{
    frame::Frame,
    pixel::{ChromaSampling, Pixel},
};
use vapoursynth::{
    api::API,
    core::CoreRef,
    map::OwnedMap,
    node::Node,
    video_info::{Property, VideoInfo},
    vsscript::{Environment, EvalFlags},
};

const OUTPUT_INDEX: i32 = 0;

/// The type for the callback function used to modify the Vapoursynth node
/// before it is used to decode frames. This allows the user to modify
/// the node to suit their needs, such as adding filters, changing the
/// output format, etc.
///
/// The callback is called with the `CoreRef` and the VapourSynth output
/// node created during initialization.
///
/// The callback must return the modified node.
///
/// Arguments
///
/// * `core` - A reference to the VapourSynth core.
/// * `node` - The VapourSynth output node created during initialization.
///
/// Returns
///
/// Returns `Ok(vapoursynth::vsscript::Node)` on success, containing the modified
/// node that will be used for decoding. Returns `Err(DecoderError)` on failure.
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
    env: Environment,
    modify_node: Option<ModifyNode>,
    frames_read: usize,
    total_frames: Option<TotalFrames>,
    video_details: Option<VideoDetails>,
}

impl VapoursynthDecoder {
    /// Creates a new VapourSynth decoder from a new VapourSynth environment.
    ///
    /// This function creates a VapourSynth environment with no output. A valid output node
    /// must be provided with the `register_node_modifier` function before the decodercan be used
    /// to decode frames.
    ///
    /// # Returns
    ///
    /// Returns `Ok(VapoursynthDecoder)` on success, containing a configured decoder.
    ///
    /// # Errors
    ///
    /// This function can return the following error:
    ///
    /// * `DecoderError::VapoursynthInternalError` - If there are internal VapourSynth API issues,
    ///   missing core, no API access, or no output node defined
    ///
    /// # Requirements
    ///
    /// - VapourSynth must be installed and properly configured on the system
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
            | vapoursynth::vsscript::Error::NoAPI => DecoderError::VapoursynthInternalError {
                cause: e.to_string(),
            },
        })?;
        Ok(Self {
            env,
            modify_node: None,
            frames_read: 0,
            total_frames: None,
            video_details: None,
        })
    }

    /// Creates a new VapourSynth decoder from a VapourSynth script file.
    ///
    /// This function loads and executes a VapourSynth script file (typically with a `.vpy` extension),
    /// creates a VapourSynth environment, and initializes a decoder ready to read video frames.
    /// The working directory is set to the directory containing the script file.
    ///
    /// # Arguments
    ///
    /// * `input` - A path to the VapourSynth script file to load. Can be any type that implements
    ///   `AsRef<Path>`, such as `&str`, `String`, `PathBuf`, or `&Path`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(VapoursynthDecoder)` on success, containing a configured decoder ready
    /// to read video frames from the script's output node.
    ///
    /// # Errors
    ///
    /// This function can return several types of errors:
    ///
    /// * `DecoderError::FileReadError` - If the script file cannot be opened, read, or contains
    ///   invalid content. This also covers VapourSynth script execution errors.
    /// * `DecoderError::VapoursynthInternalError` - If there are internal VapourSynth API issues,
    ///   missing core, no API access, or no output node defined
    /// * `DecoderError::NoVideoStream` - If the script doesn't produce a valid output node
    /// * `DecoderError::VariableFormat` - If the output has variable format (not supported)
    /// * `DecoderError::VariableResolution` - If the output has variable resolution (not supported)
    /// * `DecoderError::VariableFramerate` - If the output has variable framerate (not supported)
    /// * `DecoderError::EndOfFile` - If the script produces zero frames
    ///
    /// # Requirements
    ///
    /// - VapourSynth must be installed and properly configured on the system
    /// - The script file must define an output node (usually assigned to a variable)
    /// - The output must have constant format, resolution, and framerate
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use av_decoders::VapoursynthDecoder;
    ///
    /// // Load a VapourSynth script file
    /// let decoder = VapoursynthDecoder::new("script.vpy")?;
    ///
    /// // Using PathBuf
    /// use std::path::PathBuf;
    /// let script_path = PathBuf::from("processing_script.vpy");
    /// let decoder = VapoursynthDecoder::new(&script_path)?;
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// # VapourSynth Script Example
    ///
    /// A typical VapourSynth script file might look like:
    /// ```python
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// # Load and process video
    /// clip = core.ffms2.Source('input.mkv')
    /// clip = core.resize.Bicubic(clip, width=1920, height=1080)
    ///
    /// # Set output
    /// clip.set_output()
    /// ```
    #[inline]
    pub fn from_file<P: AsRef<Path>>(input: P) -> Result<VapoursynthDecoder, DecoderError> {
        let env = Environment::from_file(input, EvalFlags::SetWorkingDir).map_err(|e| match e {
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
            | vapoursynth::vsscript::Error::NoAPI => DecoderError::VapoursynthInternalError {
                cause: e.to_string(),
            },
        })?;
        Ok(Self {
            env,
            modify_node: None,
            frames_read: 0,
            total_frames: None,
            video_details: None,
        })
    }

    /// Creates a new VapourSynth decoder from a VapourSynth script string.
    ///
    /// This function executes a VapourSynth script provided as a string, creates a
    /// VapourSynth environment, and initializes a decoder ready to read video frames.
    /// This is useful for dynamically generated scripts or when you want to embed
    /// the script directly in your code.
    ///
    /// # Arguments
    ///
    /// * `script` - A string containing the VapourSynth script code to execute.
    ///   The script should define an output node using `clip.set_output()` or similar.
    ///
    /// # Returns
    ///
    /// Returns `Ok(VapoursynthDecoder)` on success, containing a configured decoder ready
    /// to read video frames from the script's output node.
    ///
    /// # Errors
    ///
    /// This function can return several types of errors:
    ///
    /// * `DecoderError::FileReadError` - If the script contains syntax errors, references
    ///   non-existent files, or fails during execution
    /// * `DecoderError::VapoursynthInternalError` - If there are internal VapourSynth API issues,
    ///   missing core, no API access, or no output node defined in the script
    /// * `DecoderError::NoVideoStream` - If the script doesn't produce a valid output node
    /// * `DecoderError::VariableFormat` - If the output has variable format (not supported)
    /// * `DecoderError::VariableResolution` - If the output has variable resolution (not supported)
    /// * `DecoderError::VariableFramerate` - If the output has variable framerate (not supported)
    /// * `DecoderError::EndOfFile` - If the script produces zero frames
    ///
    /// # Requirements
    ///
    /// - VapourSynth must be installed and properly configured on the system
    /// - The script must define an output node using `clip.set_output()` or equivalent
    /// - The output must have constant format, resolution, and framerate
    /// - Any file paths referenced in the script must be accessible
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use av_decoders::VapoursynthDecoder;
    ///
    /// // Simple script that loads a video file
    /// let script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// clip = core.ffms2.Source('input.mp4')
    /// clip.set_output()
    /// "#;
    ///
    /// let decoder = VapoursynthDecoder::from_script(script)?;
    ///
    /// // More complex processing script
    /// let processing_script = r#"
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// # Load video
    /// clip = core.ffms2.Source('raw_footage.mkv')
    ///
    /// # Apply denoising
    /// clip = core.knlm.KNLMeansCL(clip, d=2, a=2, h=0.8)
    ///
    /// # Resize to 1080p
    /// clip = core.resize.Bicubic(clip, width=1920, height=1080)
    ///
    /// clip.set_output()
    /// "#;
    ///
    /// let decoder = VapoursynthDecoder::from_script(processing_script)?;
    /// # Ok::<(), av_decoders::DecoderError>(())
    /// ```
    ///
    /// # Performance Note
    ///
    /// VapourSynth scripts can be computationally intensive depending on the filters used.
    /// Consider the processing requirements when designing your scripts.
    #[inline]
    pub fn from_script(script: &str) -> Result<VapoursynthDecoder, DecoderError> {
        let env = Environment::from_script(script).map_err(|e| match e {
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
            | vapoursynth::vsscript::Error::NoAPI => DecoderError::VapoursynthInternalError {
                cause: e.to_string(),
            },
        })?;
        Ok(Self {
            env,
            modify_node: None,
            frames_read: 0,
            total_frames: None,
            video_details: None,
        })
    }

    /// Sets the variables in the VapourSynth environment.
    ///
    /// This function sets the variables in the VapourSynth environment provided
    /// in the `variables` HashMap.
    ///
    /// # Arguments
    ///
    /// * `variables` - A `std::collections::HashMap<VariableName, VariableValue>`
    ///   containing the variable names and values to set. These will be passed to the
    ///   VapourSynth environment and can be accessed within the script using
    ///   `vs.get_output()` or similar mechanisms. Pass `None` if no variables are needed.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `DecoderError::VapoursynthArgsError` if there is an error setting the variables.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use av_decoders::VapoursynthDecoder;
    /// use std::collections::HashMap;
    ///
    /// // Load a VapourSynth script file
    /// let mut decoder = VapoursynthDecoder::new("script.vpy");
    ///
    /// let variables = HashMap::from([
    ///     ("message".to_string(), "fluffy kittens".to_string()),
    ///     ("start_frame".to_string(), "82".to_string()),
    /// ]);
    ///
    /// decoder.set_variables(variables)?;
    /// ```
    ///
    /// # VapourSynth Script Example
    ///
    /// A typical VapourSynth script might look like:
    /// ```python
    /// import vapoursynth as vs
    /// core = vs.core
    ///
    /// start = parseInt(vs.get_output("start_frame", "100"))
    /// things = vs.get_output("message", "prancing ponies")
    /// print("We need more " + things + "!")
    /// clip = core.ffms2.Source('input.mp4')[start:]
    /// clip.set_output()
    /// ```
    #[inline]
    pub fn set_variables(
        &mut self,
        variables: HashMap<VariableName, VariableValue>,
    ) -> Result<(), DecoderError> {
        let api = API::get().ok_or(DecoderError::VapoursynthInternalError {
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
                let (details, _) = parse_video_details(node.info())?;
                Ok(details)
            }
        }
    }

    #[allow(clippy::transmute_ptr_to_ptr)]
    pub(crate) fn read_video_frame<T: Pixel>(
        &mut self,
        cfg: &VideoDetails,
    ) -> Result<Frame<T>, DecoderError> {
        const SB_SIZE_LOG2: usize = 6;
        const SB_SIZE: usize = 1 << SB_SIZE_LOG2;
        const SUBPEL_FILTER_SIZE: usize = 8;
        const FRAME_MARGIN: usize = 16 + SUBPEL_FILTER_SIZE;
        const LUMA_PADDING: usize = SB_SIZE + FRAME_MARGIN;

        if self
            .total_frames
            .is_some_and(|total_frames| self.frames_read >= total_frames)
        {
            return Err(DecoderError::EndOfFile);
        }

        let node = {
            let output_node = match self.env.get_output(OUTPUT_INDEX) {
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
        if self.total_frames.is_none() {
            let (video_details, total_frames) = parse_video_details(node.info())?;
            self.video_details = Some(video_details);
            self.total_frames = Some(total_frames);
        }

        let vs_frame = node
            .get_frame(self.frames_read)
            .map_err(|_| DecoderError::EndOfFile)?;
        self.frames_read += 1;

        let bytes = size_of::<T>();
        let mut f: Frame<T> =
            Frame::new_with_padding(cfg.width, cfg.height, cfg.chroma_sampling, LUMA_PADDING);

        // SAFETY: We are using the stride to compute the length of the data slice
        unsafe {
            f.planes[0].copy_from_raw_u8(
                slice::from_raw_parts(
                    vs_frame.data_ptr(0),
                    vs_frame.stride(0) * vs_frame.height(0),
                ),
                vs_frame.stride(0),
                bytes,
            );
            f.planes[1].copy_from_raw_u8(
                slice::from_raw_parts(
                    vs_frame.data_ptr(1),
                    vs_frame.stride(1) * vs_frame.height(1),
                ),
                vs_frame.stride(1),
                bytes,
            );
            f.planes[2].copy_from_raw_u8(
                slice::from_raw_parts(
                    vs_frame.data_ptr(2),
                    vs_frame.stride(2) * vs_frame.height(2),
                ),
                vs_frame.stride(2),
                bytes,
            );
        }
        Ok(f)
    }

    /// Get the VapourSynth environment.
    ///
    /// This function returns a mutable reference to the
    /// VapourSynth environment created during initialization.
    ///
    /// # Returns
    ///
    /// Returns `&mut vapoursynth::vsscript::Environment`.
    pub(crate) fn get_env(&mut self) -> &mut Environment {
        &mut self.env
    }

    /// Get the VapourSynth output node.
    ///
    /// This function returns a reference to the
    /// VapourSynth output node created during initialization.
    ///
    /// If a node modifier has been registered using `VapoursynthDecoder::register_node_modifier()`,
    /// the modified node will be returned instead.
    ///
    /// # Returns
    ///
    /// Returns `vapoursynth::vsscript::Node`.
    pub(crate) fn get_output_node(&self) -> Node {
        let output_node = match self.env.get_output(OUTPUT_INDEX) {
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

    /// Register a callback function that modifies the VapourSynth output node
    /// created during initializaation.
    ///
    /// # Arguments
    ///
    /// * `modify_node` - The callback function that modifies and returns the VapourSynth
    ///   output node given a `vapoursynth::vsscript::CoreRef` and a `vapoursynth::vsscript::Node`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(vapoursynth::vsscript::Node)` on success, containing the modified node ready
    /// to be used for decoding video frames.
    ///
    /// # Errors
    ///
    /// This function can return several types of errors:
    ///
    /// * `DecoderError::FileReadError` - If the script contains syntax errors, references
    ///   non-existent files, or fails during execution
    /// * `DecoderError::VapoursynthInternalError` - If there are internal VapourSynth API issues,
    ///   missing core, no API access, or no valid output node returned
    /// * `DecoderError::NoVideoStream` - If the script doesn't produce a valid output node
    /// * `DecoderError::VariableFormat` - If the output has variable format (not supported)
    /// * `DecoderError::VariableResolution` - If the output has variable resolution (not supported)
    /// * `DecoderError::VariableFramerate` - If the output has variable framerate (not supported)
    /// * `DecoderError::EndOfFile` - If the script produces zero frames
    #[inline]
    pub fn register_node_modifier(
        &mut self,
        modify_node: ModifyNode,
    ) -> Result<Node, DecoderError> {
        let core = self
            .env
            .get_core()
            .map_err(|e| DecoderError::VapoursynthInternalError {
                cause: e.to_string(),
            })?;

        let output_node = {
            let res = self.env.get_output(OUTPUT_INDEX);
            match res {
                Ok((node, _)) => Some(node),
                Err(vapoursynth::vsscript::Error::NoOutput) => None,
                Err(e) => {
                    return Err(DecoderError::VapoursynthInternalError {
                        cause: e.to_string(),
                    })
                }
            }
        };
        let modified_node = modify_node(core, output_node)?;

        // Set the updated video details and total frames
        let (video_details, total_frames) = parse_video_details(modified_node.info())?;
        self.video_details = Some(video_details);
        self.total_frames = Some(total_frames);
        // Register the node modifier to be used during read_video_frame
        self.modify_node = Some(modify_node);

        Ok(modified_node)
    }
}

/// Get the number of frames from a Vapoursynth `VideoInfo` struct.
fn get_num_frames(info: VideoInfo) -> Result<TotalFrames, DecoderError> {
    let num_frames = {
        if Property::Variable == info.format {
            return Err(DecoderError::VariableFormat);
        }
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

/// Get the bit depth from a Vapoursynth `VideoInfo` struct.
fn get_bit_depth(info: VideoInfo) -> Result<BitDepth, DecoderError> {
    let bits_per_sample = {
        match info.format {
            Property::Variable => {
                return Err(DecoderError::VariableFormat);
            }
            Property::Constant(x) => x.bits_per_sample(),
        }
    };

    Ok(bits_per_sample as usize)
}

/// Get the resolution from a Vapoursynth `VideoInfo` struct.
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

/// Get the frame rate from a Vapoursynth `VideoInfo` struct.
fn get_frame_rate(info: VideoInfo) -> Result<Rational32, DecoderError> {
    match info.framerate {
        Property::Variable => Err(DecoderError::VariableFramerate),
        Property::Constant(fps) => Ok(Rational32::new(
            fps.numerator as i32,
            fps.denominator as i32,
        )),
    }
}

/// Get the chroma sampling from a Vapoursynth `VideoInfo` struct.
fn get_chroma_sampling(info: VideoInfo) -> Result<ChromaSampling, DecoderError> {
    match info.format {
        Property::Variable => Err(DecoderError::VariableFormat),
        Property::Constant(x) => match x.color_family() {
            vapoursynth::format::ColorFamily::YUV => {
                let ss = (x.sub_sampling_w(), x.sub_sampling_h());
                match ss {
                    (1, 1) => Ok(ChromaSampling::Cs420),
                    (1, 0) => Ok(ChromaSampling::Cs422),
                    (0, 0) => Ok(ChromaSampling::Cs444),
                    (x, y) => Err(DecoderError::UnsupportedChromaSubsampling {
                        x: x.into(),
                        y: y.into(),
                    }),
                }
            }
            vapoursynth::format::ColorFamily::Gray => Ok(ChromaSampling::Cs400),
            fmt => Err(DecoderError::UnsupportedFormat {
                fmt: fmt.to_string(),
            }),
        },
    }
}

/// Get the `VideoDetails` and `TotalFrames` from a Vapoursynth `VideoInfo` struct.
fn parse_video_details(info: VideoInfo) -> Result<(VideoDetails, TotalFrames), DecoderError> {
    let total_frames = get_num_frames(info)?;
    let (width, height) = get_resolution(info)?;
    Ok((
        VideoDetails {
            width,
            height,
            bit_depth: get_bit_depth(info)?,
            chroma_sampling: get_chroma_sampling(info)?,
            frame_rate: get_frame_rate(info)?,
        },
        total_frames,
    ))
}
