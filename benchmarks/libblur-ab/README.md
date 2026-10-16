# novtb vs. rayon under libblur

This benchmark answers one question: does libblur run faster on novtb than on
rayon? It runs libblur 0.20.0's own filters, unmodified, and changes only the
`novtb` crate underneath them. It also checks what novtb does on a wasm target.

Scope: wall time and peak memory of libblur filters on one x86_64 machine.
Not in scope: other CPUs (no ARM or many-core run yet), other libblur versions,
and threaded wasm in a browser.

Result on this machine: rayon is equal or faster. The geometric mean speedup
over novtb is 1.06x for single calls and 1.22x for the nested batch. On
`wasm32-wasip1`, novtb panics and the rayon shim runs. The details follow.

## Backends

The harness builds twice. `[patch.crates-io]` points libblur's `novtb`
dependency at a different crate in each build:

| Backend | `novtb` crate | What it does |
|---|---|---|
| `novtb` | this repository, `34f365e` (0.1.13) | Spawns scoped OS threads per call and pins them to cores. |
| `rayon-static` | [`shim/`](shim/src/lib.rs), `NOVTB_SHIM=static` | Uses novtb's exact partitioning (one group per requested thread) and runs the groups on rayon's global pool. |
| `rayon-steal` | [`shim/`](shim/src/lib.rs), `NOVTB_SHIM=steal` | Gives rayon every chunk and lets it split and steal. |

The shim implements the novtb API that libblur 0.20 calls: `ThreadPool::new`,
`parallel_for`, `tb_par_chunks_mut`, `tb_par_chunks_exact_mut`, `take`,
`skip`, `for_each` and `for_each_enumerated`. `parallel_for` runs one rayon
task per index in both modes.

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

## Results

Raw data: [`results-x86_64.csv`](results-x86_64.csv). Machine and toolchain:
[`machine-x86_64.txt`](machine-x86_64.txt) (Xeon E-2276M, 6 cores and 12
threads, Linux, `powersave` governor). Background CPU before each run: median
5%, max 11%.

Generate the table below with `python3 summarize.py results-x86_64.csv --md`.
Times are the median of five runs. A ratio above 1.00 means rayon is faster.
"steal won" counts the rounds in which `rayon-steal` beat `novtb`.

| case | threads | novtb ms | rayon-static ms | rayon-steal ms | novtb/static | novtb/steal | steal won |
|---|---:|---:|---:|---:|---:|---:|---:|
| gauss_f32_r20@2048x2048 | 12 | 69.58 | 52.10 | 51.91 | 1.32 | 1.34 | 5/5 |
| gauss_f32_r50@2048x2048 | 12 | 272.83 | 272.53 | 203.23 | 1.00 | 1.36 | 5/5 |
| gauss_u8_r20@2048x2048 | 12 | 10.11 | 10.34 | 10.36 | 0.97 | 0.98 | 1/5 |
| gauss_u8_r50@2048x2048 | 12 | 20.66 | 19.45 | 18.30 | 1.07 | 1.13 | 5/5 |
| box_f32_r20@2048x2048 | 12 | 13.53 | 13.34 | 13.35 | 1.02 | 1.01 | 5/5 |
| box_f32_r50@2048x2048 | 12 | 18.52 | 18.27 | 18.24 | 1.01 | 1.02 | 5/5 |
| box_f32_r100@2048x2048 | 12 | 21.06 | 20.42 | 20.44 | 1.03 | 1.04 | 5/5 |
| stack_u8_r20@2048x2048 | 12 | 8.32 | 7.33 | 7.33 | 1.13 | 1.14 | 5/5 |
| stack_u8_r50@2048x2048 | 12 | 8.41 | 7.77 | 7.78 | 1.08 | 1.08 | 5/5 |
| stack_u8_r100@2048x2048 | 12 | 8.58 | 8.24 | 8.24 | 1.05 | 1.04 | 5/5 |
| fastgauss_u8_r20@2048x2048 | 12 | 9.52 | 9.35 | 9.41 | 1.01 | 1.02 | 5/5 |
| fastgauss_u8_r50@2048x2048 | 12 | 9.67 | 9.49 | 9.60 | 1.02 | 1.01 | 3/5 |
| fastgauss_u8_r100@2048x2048 | 12 | 9.84 | 10.00 | 10.01 | 0.98 | 0.98 | 0/5 |
| gauss_f32_r20@4096x4096 | 12 | 426.01 | 384.19 | 387.39 | 1.11 | 1.10 | 5/5 |
| gauss_f32_r50@4096x4096 | 12 | 1121.86 | 1120.65 | 1018.82 | 1.00 | 1.12 | 5/5 |
| gauss_u8_r20@4096x4096 | 12 | 43.95 | 46.01 | 45.11 | 0.95 | 0.94 | 1/5 |
| gauss_u8_r50@4096x4096 | 12 | 132.73 | 132.34 | 106.77 | 1.00 | 1.26 | 5/5 |
| box_f32_r20@4096x4096 | 12 | 55.58 | 54.71 | 54.81 | 1.02 | 1.02 | 4/5 |
| box_f32_r50@4096x4096 | 12 | 64.04 | 63.77 | 63.84 | 1.00 | 1.00 | 5/5 |
| box_f32_r100@4096x4096 | 12 | 81.95 | 83.89 | 83.37 | 0.98 | 1.00 | 2/5 |
| stack_u8_r20@4096x4096 | 12 | 37.39 | 36.07 | 35.71 | 1.04 | 1.05 | 5/5 |
| stack_u8_r50@4096x4096 | 12 | 40.45 | 39.06 | 38.96 | 1.03 | 1.04 | 5/5 |
| stack_u8_r100@4096x4096 | 12 | 42.57 | 41.61 | 41.94 | 1.02 | 1.03 | 5/5 |
| fastgauss_u8_r20@4096x4096 | 12 | 51.83 | 50.33 | 49.33 | 1.04 | 1.04 | 5/5 |
| fastgauss_u8_r50@4096x4096 | 12 | 53.81 | 51.99 | 52.79 | 1.04 | 1.03 | 5/5 |
| fastgauss_u8_r100@4096x4096 | 12 | 55.73 | 53.47 | 54.50 | 1.03 | 1.02 | 5/5 |
| box_f32_r50@8192x8192 | 12 | 253.39 | 252.93 | 252.44 | 1.00 | 1.00 | 3/5 |
| stack_u8_r50@8192x8192 | 12 | 157.43 | 156.05 | 155.68 | 1.02 | 1.01 | 3/5 |
| fastgauss_u8_r50@8192x8192 | 12 | 202.03 | 199.70 | 201.55 | 1.00 | 1.00 | 3/5 |
| gauss_f32_r20@2048x2048x36 (nested) | 12 | 3531.91 | 2869.30 | 2793.56 | 1.23 | 1.22 | 5/5 |

Geometric mean of the per-case ratios over the 29 single-call cases:

- `rayon-static`: 1.03x (min 0.95, max 1.32).
- `rayon-steal`: 1.06x (min 0.94, max 1.36).

Peak RSS of the nested case, median of five runs, with the range in
parentheses:

- `novtb`: 2026 MiB (2014--2093).
- `rayon-static`: 1938 MiB.
- `rayon-steal`: 1928 MiB (1820--2381).

### What the numbers show

- No case shows the cache-locality cost that the libblur#67 discussion
  expects from work-stealing. `rayon-steal` is as fast as `novtb` or faster
  in 25 of 29 single-call cases. The other four are 0.94--0.996x.
- `rayon-steal` is 12--36% faster on the large-radius Gaussians
  (`gauss_f32_r50`, `gauss_u8_r50`), where `rayon-static` matches `novtb`.
  Both static backends cut the image into 12 equal zones. A likely cause is
  that some zones finish late and the other threads wait; this benchmark does
  not measure per-thread time.
- With novtb's own partitioning, a persistent pool (`rayon-static`) is faster
  than `novtb` in 11 of the 13 cases at 2048x2048, by 0.1--14%, and by 32% on
  `gauss_f32_r20`. A 32% gap (18 ms) is too large for thread-spawn cost
  alone. The cause is not measured; core pinning is a candidate.
- rayon is slower on `gauss_u8_r20` (0.94--0.98x) and on
  `fastgauss_u8_r100@2048x2048` (0.98x, 0 of 5 rounds won).
- The nested batch is 1.22x faster on rayon. Its median peak RSS is 5% lower,
  but one `rayon-steal` round reached 2381 MiB against 2093 MiB for the worst
  `novtb` round. This test did not reproduce the unbounded growth from
  libblur#67, but the widest RSS spread belongs to `rayon-steal`.

### Limits of this data

- One machine: a 6-core laptop CPU with SMT. A run on ARM (Apple M-series) or
  on a 32+ core machine can change the result in either direction.
- Each nested-case round is a single batch, so its 1.22x rests on five
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
`/proc/stat`.
