# Changelog

## Version 0.9.0

- feat!: change the ffms2 `set_output_format` function to take `ChromaSubsampling`, this allows converting to grayscale

## Version 0.8.2

- chore: bump v_frame to 0.5
- fix: fix broken behaviors in HBD decoding

## Version 0.8.1 [yanked]

- fix: fix ffms2 loading of chroma planes
- fix: improve error messages

## Version 0.8.0 [yanked]

- chore!: update to `v_frame` 0.4

## Version 0.7.0

- fix!: update compatibility with Vapoursynth R73
- fix: use correct naming convention for ffms2 index files

## Version 0.6.6

- feat: add `luma_only` option to only return luma planes, may help perf for some applications

## Version 0.6.5

- fix: fix width and height returned from ffms2 `get_video_details` when no scaling was happening
- fix: return error code when ffms2 rescaling fails
- fix: proper video format conversion for ffms2

## Version 0.6.4

- feat: add `ffms2_static` feature to link a static ffms2

## Version 0.6.3

- feat: make ffms2's `video_details` field public to make using `set_output_format` less painful

## Version 0.6.2

- feat: add `set_output_format` method for ffms2 decoder
- bump `ffms2-sys` dependency to 0.3

## Version 0.6.1

- fix: actually have the ffms2 decoder decode the whole image
- fix: ensure linear decoding mode for ffms decoder (in the future this should probably be a user-switchable param)
- perf: enable threading for ffms decoder

## Version 0.6.0 [yanked]

- feat: add ffms2 decoder interface

## Version 0.5.0

- feat: compatibility with ffmpeg 8.0

## Version 0.4.0

- feat!: add support for passing variables to VapourSynth scripts

## Version 0.3.1

- fix: properly escape paths for VS scripts on Windows

## Version 0.3.0

- [Breaking/Feature] Add seeking support to VapoursynthDecoder
- [Feature] Add `modify_node` callback to VapoursynthDecoder
- Fix the `from_file` method so that the Vapoursynth decoder is prioritized for `.vpy` inputs,
  and will be used as a fallback and work properly for video inputs. Ffmpeg will be prioritzed
  for video inputs if both features are enabled.

## Version 0.2.0

- [Breaking] Move the `from_*` methods into `Decoder`. I wanted to do this from the
  start, but had to avoid it due to fighting with generics.
- [Breaking] In order to make this work, remove the generics from `Decoder`. These
  are only used by the y4m decoder anyway, and this is not a hotspot
  where dynamic dispatch would harm performance.
- [Feature] Expose the `FfmpegDecoder` and `VapoursynthDecoder` for users who need
  to manually instantiate a decoder.
- [Feature] Add a new `from_decoder_impl` method on `Decoder` to take a manually
  instantiated decoder.
- Specify minimum Rust version

## Version 0.1.0

- Initial release
