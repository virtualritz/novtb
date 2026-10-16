//! Times libblur filters at VFX-like sizes and radii. The threading backend is
//! chosen at build time by which `novtb` the `[patch]` resolves to; this binary
//! only labels its rows with `$BACKEND`.

use libblur::{
    AnisotropicRadius, BlurImage, BlurImageMut, BoxBlurParameters, ConvolutionMode, EdgeMode,
    EdgeMode2D, FastBlurChannels, GaussianBlurParams, IeeeBinaryConvolutionMode, ThreadingPolicy,
};
use rayon::prelude::*;
use std::hint::black_box;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Op {
    /// Gaussian with kernel `2r+1` and sigma `r/3`.
    GaussF32(u32),
    GaussU8(u32),
    /// Box with kernel `2r+1`.
    BoxF32(u32),
    StackU8(u32),
    FastGaussU8(u32),
}

impl Op {
    fn name(self) -> String {
        match self {
            Op::GaussF32(r) => format!("gauss_f32_r{r}"),
            Op::GaussU8(r) => format!("gauss_u8_r{r}"),
            Op::BoxF32(r) => format!("box_f32_r{r}"),
            Op::StackU8(r) => format!("stack_u8_r{r}"),
            Op::FastGaussU8(r) => format!("fastgauss_u8_r{r}"),
        }
    }
}

fn gauss_params(r: u32) -> GaussianBlurParams {
    GaussianBlurParams::new(2 * r + 1, r as f64 / 3.0)
}

const FULL: &[Op] = &[
    Op::GaussF32(20),
    Op::GaussF32(50),
    Op::GaussU8(20),
    Op::GaussU8(50),
    Op::BoxF32(20),
    Op::BoxF32(50),
    Op::BoxF32(100),
    Op::StackU8(20),
    Op::StackU8(50),
    Op::StackU8(100),
    Op::FastGaussU8(20),
    Op::FastGaussU8(50),
    Op::FastGaussU8(100),
];

/// Radius-independent filters only: 8192x8192 Gaussians would dominate the run time.
const HUGE: &[Op] = &[Op::BoxF32(50), Op::StackU8(50), Op::FastGaussU8(50)];

const MATRIX: &[((u32, u32), &[Op])] = &[
    ((2048, 2048), FULL),
    ((4096, 4096), FULL),
    ((8192, 8192), HUGE),
];

struct Bufs {
    w: u32,
    h: u32,
    src_f: Vec<f32>,
    dst_f: Vec<f32>,
    src_u: Vec<u8>,
    dst_u: Vec<u8>,
}

impl Bufs {
    fn new(w: u32, h: u32) -> Self {
        let n = (w * h * 4) as usize;
        // Deterministic pseudo-random content so every backend sees the same input.
        let mut x = 0x9E37_79B9u32;
        let src_u: Vec<u8> = (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 24) as u8
            })
            .collect();
        let src_f = src_u.iter().map(|&v| v as f32 / 255.0).collect();
        Bufs {
            w,
            h,
            src_f,
            dst_f: vec![0.0; n],
            dst_u: src_u.clone(),
            src_u,
        }
    }

    fn run(&mut self, op: Op, policy: ThreadingPolicy) {
        let (w, h) = (self.w, self.h);
        let ch = FastBlurChannels::Channels4;
        let edges = EdgeMode2D::new(EdgeMode::Clamp);
        match op {
            Op::GaussF32(r) => {
                let src = BlurImage::borrow(&self.src_f, w, h, ch);
                let mut dst = BlurImageMut::borrow(&mut self.dst_f, w, h, ch);
                libblur::gaussian_blur_f32(
                    &src,
                    &mut dst,
                    gauss_params(r),
                    edges,
                    policy,
                    IeeeBinaryConvolutionMode::Normal,
                )
                .unwrap();
            }
            Op::GaussU8(r) => {
                let src = BlurImage::borrow(&self.src_u, w, h, ch);
                let mut dst = BlurImageMut::borrow(&mut self.dst_u, w, h, ch);
                libblur::gaussian_blur(
                    &src,
                    &mut dst,
                    gauss_params(r),
                    edges,
                    policy,
                    ConvolutionMode::FixedPoint,
                )
                .unwrap();
            }
            Op::BoxF32(r) => {
                let src = BlurImage::borrow(&self.src_f, w, h, ch);
                let mut dst = BlurImageMut::borrow(&mut self.dst_f, w, h, ch);
                libblur::box_blur_f32(&src, &mut dst, BoxBlurParameters::new(2 * r + 1), policy)
                    .unwrap();
            }
            Op::StackU8(r) => {
                // In-place filters: reset from the source so every run does the same work.
                self.dst_u.copy_from_slice(&self.src_u);
                let mut img = BlurImageMut::borrow(&mut self.dst_u, w, h, ch);
                libblur::stack_blur(&mut img, AnisotropicRadius::new(r), policy).unwrap();
            }
            Op::FastGaussU8(r) => {
                self.dst_u.copy_from_slice(&self.src_u);
                let mut img = BlurImageMut::borrow(&mut self.dst_u, w, h, ch);
                libblur::fast_gaussian(&mut img, AnisotropicRadius::new(r), policy, edges).unwrap();
            }
        }
    }

    /// FNV-1a over the output, to check backends compute the same pixels.
    fn digest(&self, op: Op) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        match op {
            Op::GaussF32(_) | Op::BoxF32(_) => self
                .dst_f
                .iter()
                .flat_map(|v| v.to_bits().to_le_bytes())
                .for_each(&mut eat),
            _ => self.dst_u.iter().copied().for_each(&mut eat),
        }
        h
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Runs `f` for at least `budget` and `min_samples`, after one warm-up call
/// (more if the budget allows), and returns (median, p10, p90, n) in microseconds.
fn measure(mut f: impl FnMut(), budget: Duration, min_samples: usize) -> (f64, f64, f64, usize) {
    let warm = Instant::now();
    f();
    while warm.elapsed() < budget / 4 {
        f();
    }
    let mut samples = Vec::new();
    let start = Instant::now();
    while start.elapsed() < budget || samples.len() < min_samples {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_secs_f64() * 1e6);
    }
    samples.sort_by(f64::total_cmp);
    (
        percentile(&samples, 0.5),
        percentile(&samples, 0.1),
        percentile(&samples, 0.9),
        samples.len(),
    )
}

/// Peak resident set size of this process so far, in MiB.
fn peak_rss_mib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map_or(0, |kib| kib / 1024)
}

fn main() {
    let backend = std::env::var("BACKEND").unwrap_or_else(|_| "?".into());
    let budget = Duration::from_millis(
        std::env::var("BUDGET_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000),
    );
    // CASE selects one case exactly, FILTER by substring; LIST=1 prints names.
    let case_eq = std::env::var("CASE").ok();
    let filter = std::env::var("FILTER").unwrap_or_default();
    let list = std::env::var("LIST").is_ok();
    let skip = |case: &str| {
        let hit = case_eq
            .as_deref()
            .map_or(case.contains(&filter), |c| c == case);
        if hit && list {
            println!("{case}");
        }
        !hit || list
    };
    let policy = ThreadingPolicy::Adaptive;

    for &((w, h), ops) in MATRIX {
        let mut bufs: Option<Bufs> = None;
        for &op in ops {
            let case = format!("{}@{w}x{h}", op.name());
            if skip(&case) {
                continue;
            }
            let bufs = bufs.get_or_insert_with(|| Bufs::new(w, h));
            let (med, p10, p90, n) = measure(|| bufs.run(black_box(op), policy), budget, 7);
            println!(
                "{backend},single_call,{case},{},{med:.1},{p10:.1},{p90:.1},{n},{:016x},{}",
                policy.thread_count(w, h),
                bufs.digest(op),
                peak_rss_mib()
            );
        }
    }

    // libblur issue #67: independent blurs launched from an outer rayon loop,
    // each allocating its own image inside the task, as a renderer blurring
    // many layers at once does. Measures time and peak memory; it runs once
    // per process because peak RSS cannot be reset.
    let (w, h, items, op) = (2048u32, 2048u32, 36usize, Op::GaussF32(20));
    let case = format!("{}@{w}x{h}x{items}", op.name());
    if !skip(&case) {
        let t = Instant::now();
        let digests: Vec<u64> = (0..items)
            .into_par_iter()
            .map(|_| {
                let mut b = Bufs::new(w, h);
                b.run(op, policy);
                b.digest(op)
            })
            .collect();
        let us = t.elapsed().as_secs_f64() * 1e6;
        println!(
            "{backend},nested_alloc,{case},{},{us:.1},{us:.1},{us:.1},1,{:016x},{}",
            policy.thread_count(w, h),
            digests[0],
            peak_rss_mib()
        );
    }
}
