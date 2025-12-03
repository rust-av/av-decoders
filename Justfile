precommit:
    cargo fmt
    cargo clippy --tests --benches -- -D warnings
    cargo test
    cargo clippy --features vapoursynth --tests --benches -- -D warnings
    cargo test --features vapoursynth
    cargo clippy --features ffmpeg --tests --benches -- -D warnings
    cargo test --features ffmpeg
    cargo clippy --features ffms2 --tests --benches -- -D warnings
    cargo test --features ffms2
    cargo clippy --features ffmpeg,vapoursynth,ffms2 --tests --benches -- -D warnings
    cargo test --features ffmpeg,vapoursynth,ffms2
