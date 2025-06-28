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
    map::OwnedMap,
    node::Node,
    video_info::{Property, VideoInfo},
    vsscript::{Environment, EvalFlags},
};

const OUTPUT_INDEX: i32 = 0;

/// An interface that is used for decoding a video stream using Vapoursynth
pub struct VapoursynthDecoder {
    env: Environment,
    frames_read: usize,
    total_frames: usize,
}

impl VapoursynthDecoder {
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
    pub fn new<P: AsRef<Path>>(input: P) -> Result<VapoursynthDecoder, DecoderError> {
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
        let total_frames = {
            let (node, _) = env
                .get_output(OUTPUT_INDEX)
                .map_err(|_| DecoderError::NoVideoStream)?;
            get_num_frames(node.info())?
        };
        Ok(Self {
            env,
            frames_read: 0,
            total_frames,
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
        let total_frames = {
            let (node, _) = env
                .get_output(OUTPUT_INDEX)
                .map_err(|_| DecoderError::NoVideoStream)?;
            get_num_frames(node.info())?
        };
        Ok(Self {
            env,
            frames_read: 0,
            total_frames,
        })
    }

    pub(crate) fn set_arguments(
        &mut self,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<(), DecoderError> {
        let api = API::get().ok_or(DecoderError::VapoursynthInternalError {
            cause: "failed to get Vapoursynth API instance".to_string(),
        })?;
        let mut arguments_map = OwnedMap::new(api);

        if let Some(arguments) = arguments {
            for (key, value) in arguments {
                arguments_map
                    .set_data(key.as_str(), value.as_bytes())
                    .map_err(|e| DecoderError::VapoursynthArgsError {
                        cause: e.to_string(),
                    })?;
            }
        }

        self.env
            .set_variables(&arguments_map)
            .map_err(|e| DecoderError::VapoursynthArgsError {
                cause: e.to_string(),
            })
    }

    pub(crate) fn get_video_details(&self) -> Result<VideoDetails, DecoderError> {
        let (node, _) = self
            .env
            .get_output(OUTPUT_INDEX)
            .expect("output node exists--validated during initialization");
        let info = node.info();
        let (width, height) = get_resolution(info)?;
        Ok(VideoDetails {
            width,
            height,
            bit_depth: get_bit_depth(info)?,
            chroma_sampling: get_chroma_sampling(info)?,
            frame_rate: get_frame_rate(info)?,
        })
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

        if self.frames_read >= self.total_frames {
            return Err(DecoderError::EndOfFile);
        }

        let (node, _) = self
            .env
            .get_output(OUTPUT_INDEX)
            .expect("output node exists--validated during initialization");
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

    pub(crate) fn get_env(&mut self) -> &mut Environment {
        &mut self.env
    }

    pub(crate) fn get_output_node(&self) -> Node {
        let (node, _) = self
            .env
            .get_output(OUTPUT_INDEX)
            .expect("output node exists--validated during initialization");
        node
    }
}

/// Get the number of frames from a Vapoursynth `VideoInfo` struct.
fn get_num_frames(info: VideoInfo) -> Result<usize, DecoderError> {
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
fn get_bit_depth(info: VideoInfo) -> Result<usize, DecoderError> {
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
fn get_resolution(info: VideoInfo) -> Result<(usize, usize), DecoderError> {
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
