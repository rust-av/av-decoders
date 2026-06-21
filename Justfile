precommit:
    cargo fmt --all
    cargo clippy --tests --benches -- -D warnings
    cargo test
    cargo clippy --features ffmpeg,vapoursynth,ffms2 --tests --benches -- -D warnings
    cargo test --features ffmpeg,vapoursynth,ffms2
