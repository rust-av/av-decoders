#![allow(missing_docs)]
#![allow(clippy::unwrap_used)]

#[cfg(feature = "ffmpeg")]
use av_decoders::FfmpegDecoder;
#[cfg(feature = "vapoursynth")]
use av_decoders::VapoursynthDecoder;
use av_decoders::{Decoder, Y4mDecoder};
use criterion::{criterion_group, criterion_main, Criterion};
use std::{
    fs::File,
    hint::black_box,
    io::{BufReader, Read},
};

const TEST_FILE: &str = "./test_files/tt_sif.y4m";
const HBD_TEST_FILE: &str = "./test_files/tt_sif_10b.y4m";
const EXPECTED_FRAMECOUNT: usize = 112;

fn y4m_benchmark(c: &mut Criterion) {
    c.bench_function("y4m decode", |b| {
        b.iter_batched(
            || {
                let file = black_box(File::open(TEST_FILE).unwrap());
                let reader = black_box(BufReader::new(file));
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Y4m(black_box(
                    Y4mDecoder::new(Box::new(reader) as Box<dyn Read>).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u8>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

fn y4m_hbd_benchmark(c: &mut Criterion) {
    c.bench_function("y4m decode 10-bit", |b| {
        b.iter_batched(
            || {
                let file = black_box(File::open(HBD_TEST_FILE).unwrap());
                let reader = black_box(BufReader::new(file));
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Y4m(black_box(
                    Y4mDecoder::new(Box::new(reader) as Box<dyn Read>).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u16>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

#[cfg(feature = "vapoursynth")]
fn vapoursynth_benchmark(c: &mut Criterion) {
    c.bench_function("vapoursynth decode", |b| {
        let script = format!(
            r#"
import vapoursynth as vs
core = vs.core
clip = core.lsmas.LWLibavSource(source="{}")
clip.set_output(0)
"#,
            TEST_FILE
        );
        // Create the decoder once to build the index file
        let _ = Decoder::from_decoder_impl(av_decoders::DecoderImpl::Vapoursynth(black_box(
            VapoursynthDecoder::from_script(&script).unwrap(),
        )))
        .unwrap();

        b.iter_batched(
            || {
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Vapoursynth(black_box(
                    VapoursynthDecoder::from_script(&script).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u8>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

#[cfg(feature = "vapoursynth")]
fn vapoursynth_hbd_benchmark(c: &mut Criterion) {
    c.bench_function("vapoursynth decode 10-bit", |b| {
        let script = format!(
            r#"
import vapoursynth as vs
core = vs.core
clip = core.lsmas.LWLibavSource(source="{}")
clip.set_output(0)
"#,
            HBD_TEST_FILE
        );
        // Create the decoder once to build the index file
        let _ = Decoder::from_decoder_impl(av_decoders::DecoderImpl::Vapoursynth(black_box(
            VapoursynthDecoder::from_script(&script).unwrap(),
        )))
        .unwrap();

        b.iter_batched(
            || {
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Vapoursynth(black_box(
                    VapoursynthDecoder::from_script(&script).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u16>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

#[cfg(not(feature = "vapoursynth"))]
fn vapoursynth_benchmark(_c: &mut Criterion) {}

#[cfg(not(feature = "vapoursynth"))]
fn vapoursynth_hbd_benchmark(_c: &mut Criterion) {}

#[cfg(feature = "ffmpeg")]
fn ffmpeg_benchmark(c: &mut Criterion) {
    c.bench_function("ffmpeg decode", |b| {
        b.iter_batched(
            || {
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Ffmpeg(black_box(
                    FfmpegDecoder::new(TEST_FILE).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u8>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

#[cfg(feature = "ffmpeg")]
fn ffmpeg_hbd_benchmark(c: &mut Criterion) {
    c.bench_function("ffmpeg decode 10-bit", |b| {
        b.iter_batched(
            || {
                Decoder::from_decoder_impl(av_decoders::DecoderImpl::Ffmpeg(black_box(
                    FfmpegDecoder::new(HBD_TEST_FILE).unwrap(),
                )))
                .unwrap()
            },
            |mut decoder| {
                let mut frames = 0;
                while decoder.read_video_frame::<u16>().is_ok() {
                    frames += 1;
                }
                assert_eq!(frames, EXPECTED_FRAMECOUNT);
            },
            criterion::BatchSize::PerIteration,
        )
    });
}

#[cfg(not(feature = "ffmpeg"))]
fn ffmpeg_benchmark(_c: &mut Criterion) {}

#[cfg(not(feature = "ffmpeg"))]
fn ffmpeg_hbd_benchmark(_c: &mut Criterion) {}

criterion_group!(
    decoders_bench,
    y4m_benchmark,
    y4m_hbd_benchmark,
    vapoursynth_benchmark,
    vapoursynth_hbd_benchmark,
    ffmpeg_benchmark,
    ffmpeg_hbd_benchmark,
);
criterion_main!(decoders_bench);
