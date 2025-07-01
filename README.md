# av-decoders

[![Crates.io Version](https://img.shields.io/crates/v/av-decoders)](https://crates.io/crates/av-decoders) [![docs.rs](https://img.shields.io/docsrs/av-decoders)](https://docs.rs/av-decoders/latest/av_decoders/)

A unified Rust library for video decoding that provides a consistent interface across multiple decoding backends. Part of the rust-av ecosystem, outputting frames in the standard `v_frame` format.

## Features

- **Multiple Decoding Backends**: Supports Y4M, FFmpeg, and VapourSynth decoders
- **Automatic Format Detection**: Automatically selects the best decoder for your input
- **Consistent API**: Same interface regardless of the underlying decoder
- **Zero-Copy Operations**: Efficient frame handling with minimal memory overhead
- **Comprehensive Error Handling**: Detailed error messages for debugging

## Supported Formats

| Backend                    | Formats             | Notes                              |
| -------------------------- | ------------------- | ---------------------------------- |
| **Y4M** (default)          | `.y4m`, `.yuv`      | Fastest, lowest overhead           |
| **FFmpeg** (optional)      | Most video formats  | Broad format support               |
| **VapourSynth** (optional) | Enhanced processing | Best metadata accuracy and seeking |

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
av-decoders = "0.1.0"
```

### Feature Flags

Enable additional decoders with feature flags:

```toml
[dependencies]
av-decoders = { version = "0.1.0", features = ["ffmpeg", "vapoursynth"] }
```

Available features:

- `ffmpeg` - Enable FFmpeg-based decoding for broad format support
- `vapoursynth` - Enable VapourSynth-based decoding for advanced processing
- `ffmpeg_static` - Link FFmpeg statically
- `ffmpeg_build` - Build FFmpeg from source

## Quick Start

### Basic Usage

```rust
use av_decoders::Decoder;

// Decode from a file
let mut decoder = Decoder::from_file("video.y4m")?;
let details = decoder.get_video_details();
println!("Video: {}x{} @ {} fps",
    details.width,
    details.height,
    details.frame_rate
);

// Read frames
while let Ok(frame) = decoder.read_video_frame::<u8>() {
    // Process the frame...
    println!("Read frame with {} planes", frame.planes.len());
}
```

### Reading from stdin

```rust
use av_decoders::Decoder;

let mut decoder = Decoder::from_stdin()?;
let frame = decoder.read_video_frame::<u8>()?;
```

### Working with Different Bit Depths

```rust
// For 8-bit video
let frame_8bit = decoder.read_video_frame::<u8>()?;

// For 10-bit video
let frame_10bit = decoder.read_video_frame::<u16>()?;
```

### Accessing Video Metadata

```rust
let details = decoder.get_video_details();
println!("Dimensions: {}x{}", details.width, details.height);
println!("Bit depth: {} bits", details.bit_depth);
println!("Chroma sampling: {:?}", details.chroma_sampling);
println!("Frame rate: {}", details.frame_rate);
```

## API Reference

### Main Functions

- `Decoder::from_file(path)` - Create a decoder from a file path
- `Decoder::from_stdin()` - Create a decoder reading from stdin

### Decoder Methods

- `get_video_details()` - Get video metadata and configuration
- `read_video_frame::<T>()` - Read the next frame as type T (u8 or u16)

### Video Details

The `VideoDetails` struct provides essential video information:

```rust
pub struct VideoDetails {
    pub width: usize,           // Frame width in pixels
    pub height: usize,          // Frame height in pixels
    pub bit_depth: usize,       // Bits per color component
    pub chroma_sampling: ChromaSampling, // Subsampling format
    pub frame_rate: Rational32, // Frames per second
}
```

## Backend Selection

The library automatically selects the best decoder based on:

1. **File Extension**: Y4M files (`.y4m`, `.yuv`) use the Y4M parser
2. **Feature Availability**: FFmpeg preferred over VapourSynth when both available

## Error Handling

```rust
use av_decoders::{Decoder, DecoderError};

match Decoder::from_file("video.mp4") {
    Ok(mut decoder) => {
        match decoder.read_video_frame::<u8>() {
            Ok(frame) => println!("Success!"),
            Err(DecoderError::EndOfFile) => println!("Reached end of video"),
            Err(e) => println!("Decode error: {}", e),
        }
    }
    Err(DecoderError::NoDecoder) => {
        println!("No decoder available - try enabling ffmpeg feature");
    }
    Err(e) => println!("Failed to open file: {}", e),
}
```

## Examples

### Frame Processing Pipeline

```rust
use av_decoders::Decoder;
use v_frame::pixel::ChromaSampling;

fn process_video(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = Decoder::from_file(path)?;
    let details = decoder.get_video_details();

    // Process based on video characteristics
    match details.chroma_sampling {
        ChromaSampling::Cs420 => println!("4:2:0 subsampling detected"),
        ChromaSampling::Cs422 => println!("4:2:2 subsampling detected"),
        ChromaSampling::Cs444 => println!("4:4:4 subsampling detected"),
        _ => println!("Other subsampling format"),
    }

    let mut frame_count = 0;
    while let Ok(_frame) = decoder.read_video_frame::<u8>() {
        frame_count += 1;
        if frame_count % 100 == 0 {
            println!("Processed {} frames", frame_count);
        }
    }

    println!("Total frames: {}", frame_count);
    Ok(())
}
```

## Building from Source

```bash
# Clone the repository
git clone https://github.com/rust-av/av-decoders
cd av-decoders

# Build with default features (Y4M only)
cargo build

# Build with all features
cargo build --features "ffmpeg,vapoursynth"

# Run tests
cargo test
```

## System Dependencies

### For FFmpeg support:

- FFmpeg development libraries
- Or use `ffmpeg_build` feature to compile from source

### For VapourSynth support:

- VapourSynth installation
- Python development headers

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Related Projects

- [v_frame](https://crates.io/crates/v_frame) - Video frame representation
- [rust-av](https://github.com/rust-av) - Rust multimedia ecosystem
