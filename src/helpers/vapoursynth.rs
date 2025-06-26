use crate::error::DecoderError;
use crate::VideoDetails;
use num_rational::Rational32;
use std::{mem::size_of, path::Path, slice};
use v_frame::{
    frame::Frame,
    pixel::{ChromaSampling, Pixel},
};
use vapoursynth::{
    video_info::{Property, VideoInfo},
    vsscript::{Environment, EvalFlags},
};

const OUTPUT_INDEX: i32 = 0;

pub struct VapoursynthDecoder {
    env: Environment,
    frames_read: usize,
    total_frames: usize,
}

impl VapoursynthDecoder {
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

    pub fn get_video_details(&self) -> Result<VideoDetails, DecoderError> {
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
    pub fn read_video_frame<T: Pixel>(
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
