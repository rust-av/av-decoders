# Changelog

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
