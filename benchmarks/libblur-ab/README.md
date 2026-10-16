# novtb vs. rayon under libblur

This benchmark answers one question: does libblur run faster on novtb than on
rayon? It runs libblur's own filters, unmodified, and changes only the `novtb`
crate underneath them. It also checks what novtb does on a wasm target.

Scope: wall time and peak memory of libblur 0.24.0 and 0.20.0 filters on one
x86_64 machine. 0.24.0 is the current release; 0.20.0 is the version citra
pins. Not in scope: other CPUs (no ARM or many-core run yet) and threaded wasm
in a browser.

Result on this machine: rayon is equal or faster on both versions. On 0.24.0,
`rayon-steal` is never slower than novtb (worst case 0.999x). Its geometric
mean speedup is 1.07x for single calls and 1.23x for the nested batch. On
`wasm32-wasip1`, novtb panics and the rayon shim runs. The details follow.

## Backends

The harness builds twice. `[patch.crates-io]` points libblur's `novtb`
dependency at a different crate in each build:

| Backend | `novtb` crate | What it does |
|---|---|---|
| `novtb` | novtb `34f365e` (0.1.13), this branch's `../..` | Spawns scoped OS threads per call and pins them to cores. |
| `rayon-static` | [`shim/`](shim/src/lib.rs), `NOVTB_SHIM=static` | Uses novtb's exact partitioning (one group per requested thread) and runs the groups on rayon's global pool. |
| `rayon-steal` | [`shim/`](shim/src/lib.rs), `NOVTB_SHIM=steal` | Gives rayon every chunk and lets it split and steal. |

The shim implements the novtb API that libblur 0.20 and 0.24 call: `ThreadPool::new`,
`parallel_for`, `tb_par_chunks_mut`, `tb_par_chunks_exact_mut`, `take`,
`skip`, `for_each`, `for_each_enumerated` and the two `*_with_context`
methods. `parallel_for` runs one rayon task per index in both modes. In the
`*_with_context` methods, `static` mode makes one context per zone, as novtb
does, and `steal` mode uses rayon's `for_each_init`, which makes one context
per split job.

`rayon-static` against `novtb` isolates one variable: a persistent pool versus
spawning threads per call. `rayon-steal` against `novtb` compares
work-stealing with novtb's static zones.

## Method

Each case is one libblur call on an RGBA image with `ThreadingPolicy::Adaptive`,
which gives 12 threads at these sizes. The Gaussian uses kernel `2r+1` and
sigma `r/3`. The box blur uses kernel `2r+1`. Stack blur and fast Gaussian take
the radius as is.

[`run.sh`](run.sh) runs every case as follows:

1. Wait until no compiler runs and other processes use less than 12% of all
   CPUs, sampled from `/proc/stat` for 0.5 s.
2. Run the case on `novtb`, then on `rayon-static`, then on `rayon-steal`.
   Each run is a separate process.
3. Repeat steps 1 and 2 for five rounds.

A run makes one warm-up call, then calls the filter for at least 1 s and at
least 7 times. It records the median call time. A ratio comes from the two
runs of the same round, so a burst of outside load affects both sides.
[`summarize.py`](summarize.py) reports the median of the five ratios.

Every row carries an FNV-1a hash of the output pixels. All rows of a case have
the same hash, so all three backends compute the same image.

The nested case reproduces the pattern from
[libblur#67](https://github.com/awxkee/libblur/issues/67). An outer rayon
`into_par_iter` runs 36 tasks. Each task allocates its own 2048x2048 image
(about 160 MiB of buffers), blurs it with `Adaptive` and drops it. The case
records the wall time of the whole batch and the process's peak RSS
(`VmHWM`). It runs once per process, because peak RSS cannot be reset.

## Results: libblur 0.24.0

Raw data: [`results-x86_64-libblur-0.24.csv`](results-x86_64-libblur-0.24.csv).
Machine and toolchain: [`machine-x86_64-libblur-0.24.txt`](machine-x86_64-libblur-0.24.txt)
(Xeon E-2276M, 6 cores and 12 threads, Linux, `powersave` governor).
Background CPU before each run: median 3%, max 10%.

Generate the table below with
`python3 summarize.py results-x86_64-libblur-0.24.csv --md`. Times are the
median of five runs. A ratio above 1.00 means rayon is faster. "steal won"
counts the rounds in which `rayon-steal` beat `novtb`. The last row is the
nested case.

| case | threads | novtb ms | rayon-static ms | rayon-steal ms | novtb/static | novtb/steal | steal won |
|---|---:|---:|---:|---:|---:|---:|---:|
| gauss_f32_r20@2048x2048 | 12 | 69.75 | 54.36 | 53.33 | 1.29 | 1.30 | 5/5 |
| gauss_f32_r50@2048x2048 | 12 | 272.52 | 271.64 | 195.29 | 1.00 | 1.39 | 5/5 |
| gauss_u8_r20@2048x2048 | 12 | 9.26 | 9.15 | 9.14 | 1.01 | 1.01 | 4/5 |
| gauss_u8_r50@2048x2048 | 12 | 25.73 | 22.87 | 21.66 | 1.13 | 1.19 | 5/5 |
| box_f32_r20@2048x2048 | 12 | 13.58 | 13.33 | 13.32 | 1.02 | 1.02 | 5/5 |
| box_f32_r50@2048x2048 | 12 | 18.48 | 18.24 | 18.20 | 1.02 | 1.01 | 5/5 |
| box_f32_r100@2048x2048 | 12 | 20.85 | 19.92 | 19.81 | 1.04 | 1.05 | 5/5 |
| stack_u8_r20@2048x2048 | 12 | 8.19 | 7.16 | 7.19 | 1.14 | 1.14 | 5/5 |
| stack_u8_r50@2048x2048 | 12 | 8.46 | 7.60 | 7.58 | 1.11 | 1.12 | 5/5 |
| stack_u8_r100@2048x2048 | 12 | 8.66 | 8.18 | 8.16 | 1.06 | 1.06 | 5/5 |
| fastgauss_u8_r20@2048x2048 | 12 | 9.64 | 9.11 | 9.09 | 1.07 | 1.06 | 5/5 |
| fastgauss_u8_r50@2048x2048 | 12 | 9.79 | 9.47 | 9.37 | 1.04 | 1.05 | 5/5 |
| fastgauss_u8_r100@2048x2048 | 12 | 9.89 | 9.90 | 10.00 | 1.00 | 1.00 | 3/5 |
| gauss_f32_r20@4096x4096 | 12 | 425.64 | 379.42 | 382.04 | 1.11 | 1.11 | 5/5 |
| gauss_f32_r50@4096x4096 | 12 | 1107.53 | 1104.21 | 983.84 | 1.00 | 1.12 | 5/5 |
| gauss_u8_r20@4096x4096 | 12 | 40.59 | 40.79 | 39.76 | 1.00 | 1.01 | 4/5 |
| gauss_u8_r50@4096x4096 | 12 | 169.91 | 168.59 | 144.84 | 1.00 | 1.16 | 5/5 |
| box_f32_r20@4096x4096 | 12 | 53.61 | 53.19 | 53.20 | 1.01 | 1.01 | 5/5 |
| box_f32_r50@4096x4096 | 12 | 62.65 | 61.96 | 62.03 | 1.01 | 1.01 | 5/5 |
| box_f32_r100@4096x4096 | 12 | 78.67 | 81.61 | 78.72 | 0.98 | 1.00 | 2/5 |
| stack_u8_r20@4096x4096 | 12 | 36.73 | 34.66 | 34.42 | 1.05 | 1.06 | 5/5 |
| stack_u8_r50@4096x4096 | 12 | 39.79 | 37.22 | 37.06 | 1.07 | 1.07 | 5/5 |
| stack_u8_r100@4096x4096 | 12 | 42.51 | 39.96 | 40.24 | 1.06 | 1.05 | 5/5 |
| fastgauss_u8_r20@4096x4096 | 12 | 50.25 | 47.56 | 48.24 | 1.06 | 1.03 | 4/5 |
| fastgauss_u8_r50@4096x4096 | 12 | 54.12 | 51.88 | 51.80 | 1.05 | 1.05 | 5/5 |
| fastgauss_u8_r100@4096x4096 | 12 | 54.17 | 52.41 | 52.23 | 1.03 | 1.03 | 4/5 |
| box_f32_r50@8192x8192 | 12 | 245.77 | 244.33 | 244.62 | 1.01 | 1.00 | 3/5 |
| stack_u8_r50@8192x8192 | 12 | 163.89 | 158.12 | 159.17 | 1.03 | 1.02 | 4/5 |
| fastgauss_u8_r50@8192x8192 | 12 | 208.27 | 210.88 | 208.40 | 0.99 | 1.02 | 4/5 |
| gauss_f32_r20@2048x2048x36 | 12 | 3480.30 | 2935.63 | 2824.80 | 1.19 | 1.23 | 5/5 |

Geometric mean of the per-case ratios over the 29 single-call cases:

- `rayon-static`: 1.05x (min 0.98, max 1.29).
- `rayon-steal`: 1.07x (min 0.999, max 1.39).

Peak RSS of the nested case over five runs, median with the range in
parentheses:

- `novtb`: 2033 MiB (2003--2059).
- `rayon-static`: 1938 MiB (1871--2299).
- `rayon-steal`: 1933 MiB (1849--2137).

### What the numbers show

- No case shows the cache-locality cost that the libblur#67 discussion
  expects from work-stealing. `rayon-steal` is as fast as `novtb` or faster
  in 28 of 29 single-call cases; the other one is 0.999x.
- Per-zone contexts do not favor novtb. 0.24.0 moved box blur and the 1D
  Gaussian to `for_each_enumerated_with_context`. `rayon-steal` makes more
  contexts there than novtb does, and is still 1.00--1.05x on all box cases.
- `rayon-steal` is 12--39% faster on the `r50` Gaussians. `rayon-static`
  matches `novtb` on three of these four cases (1.13x on
  `gauss_u8_r50@2048x2048`). Both static backends cut the image into 12 equal
  zones. A likely cause is that some zones finish late and the other threads
  wait; this benchmark does not measure per-thread time.
- With novtb's own partitioning, a persistent pool (`rayon-static`) is faster
  than `novtb` in all 13 cases at 2048x2048, by 0.2--14%, and by 29% on
  `gauss_f32_r20`. A 29% gap (15 ms) is too large for thread-spawn cost
  alone. The cause is not measured; core pinning is a candidate.
- The nested batch is 1.23x faster on `rayon-steal`, and its median peak RSS
  is 5% lower. The highest single peak (2299 MiB) belongs to `rayon-static`.
  This test did not reproduce the unbounded growth from libblur#67, but both
  rayon backends spread wider in peak RSS than `novtb`.

## Results: libblur 0.20.0

Raw data: [`results-x86_64-libblur-0.20.csv`](results-x86_64-libblur-0.20.csv),
machine: [`machine-x86_64-libblur-0.20.txt`](machine-x86_64-libblur-0.20.txt).
Same machine, cases and method, run with the shim at commit `eb22572`, which
targets libblur 0.20.0. Summarize it the same way.

- `rayon-static`: geometric mean 1.03x (min 0.95, max 1.32).
- `rayon-steal`: geometric mean 1.06x (min 0.94, max 1.36).
- Nested batch: 1.22x on `rayon-steal`. Peak RSS medians: `novtb` 2026 MiB
  (2014--2093), `rayon-steal` 1928 MiB (1820--2381).
- `rayon-steal` was slower than `novtb` on `gauss_u8_r20` (0.94--0.98x) and on
  `fastgauss_u8_r100@2048x2048` (0.98x). On 0.24.0 these three cases are
  1.00--1.01x.

### Limits of this data

- One machine: a 6-core laptop CPU with SMT. A run on ARM (Apple M-series) or
  on a 32+ core machine can change the result in either direction.
- Each nested-case round is a single batch, so its ratio rests on five
  measurements per backend.
- Seven cases ran only 7 or 8 calls per run, because one call takes 0.1 to
  1.1 s: `gauss_f32_r50` at both sizes, `gauss_f32_r20@4096x4096`,
  `gauss_u8_r50@4096x4096`, and the three 8192x8192 cases.
- The shim uses rayon's global pool and ignores the thread count that
  `Adaptive` asks for in `steal` mode. At these sizes, `Adaptive` asks for all
  12 threads anyway.

A run that would falsify the result: the same harness on another machine with
`novtb` faster than `rayon-steal` across most cases.

## wasm

[`wasm-probe/`](wasm-probe/src/main.rs) makes one `stack_blur` call with
`ThreadingPolicy::Fixed(4)` on `wasm32-wasip1`. That target has no
`std::thread`, the same as `wasm32-unknown-unknown` in a browser.

- `novtb` panics in `std::thread::scope`: `failed to spawn thread: Os { code: 58, kind: Unsupported, message: "Not supported" }`.
- The rayon shim returns normally. rayon runs the work on the calling thread
  when it cannot spawn threads. In a browser, `wasm-bindgen-rayon` gives
  rayon a pool of Web Workers.

So on wasm, any libblur filter that goes through novtb works only with
`ThreadingPolicy::Single`.

Reproduce:

```sh
cd wasm-probe
for arm in novtb:../../.. rayon:../shim; do
  cargo build --release --target wasm32-wasip1 --target-dir ../target/wasm-${arm%%:*} \
    --config "patch.crates-io.novtb.path=\"${arm#*:}\""
  wasmtime ../target/wasm-${arm%%:*}/wasm32-wasip1/release/wasm-probe.wasm
done
```

## Run it

```sh
ROUNDS=5 BUDGET_MS=1000 ./run.sh > results.csv   # about 20 min on an idle machine
python3 summarize.py results.csv
```

`FILTER=gauss` limits the run to cases whose name contains `gauss`.
`IDLE_PCT` sets the idle threshold in percent. The run needs a Linux
`/proc/stat`. To measure libblur 0.20.0, set `libblur = "=0.20.0"` in
`harness/Cargo.toml`.
