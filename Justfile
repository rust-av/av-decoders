precommit:
    cargo fmt
    cargo clippy --tests --benches -- -D warnings
    cargo test
    cargo clippy --features vapoursynth --tests --benches -- -D warnings
    cargo test --features vapoursynth
    cargo clippy --features ffmpeg --tests --benches -- -D warnings
    cargo test --features ffmpeg
    cargo clippy --features ffmpeg,vapoursynth --tests --benches -- -D warnings
    cargo test --features ffmpeg,vapoursynth