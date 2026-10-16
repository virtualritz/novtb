# novtb – Simple Zoned Data-Parallelism with Core Affinity

novtb provides a lightweight, brute-force data-parallel execution model with support for core pinning (CPU affinity). It's useful for workloads where threads should be pinned to specific cores for improved cache locality or performance consistency.

# Features

- Zone-based parallel iteration over chunked data
- Explicit thread pool with core affinity
- Simple API without complex task graphs or scheduling overhead

# Some examples

```rust
let pool = novtb::ThreadPool::new(thread_count as usize);
pool.parallel_for(|thread_index| {
    // Perform thread-specific heavy work here
);
```

```rust
let pool = novtb::ThreadPool::new(thread_count as usize);
dst.tb_par_chunks_mut(dst_stride as usize * tile_size)
    .for_each_enumerated(&pool, |cy, dst_rows| {
    // Process a tile at row 'cy' in 'dst_rows'
    });
```

# Backends

novtb splits the work into one zone per requested thread. The backend decides
where each zone runs:

| Target | Backend | Zones run on |
|---|---|---|
| native, default | scoped threads | one `std::thread` per zone, pinned to a core |
| native, `rayon` feature | rayon | one task per zone on rayon's global pool |
| wasm (`target_family = "wasm"`) | rayon, always | one task per zone on rayon's global pool |

Both backends split the work the same way and make one context per zone in
the `*_with_context` methods. The API is the same for both.

wasm has no `std::thread` spawn, so novtb always uses rayon there. In a
browser, initialize a Web Worker pool with
[`wasm-bindgen-rayon`](https://github.com/RReverser/wasm-bindgen-rayon) to get
real threads. Without a pool, rayon runs every zone on the calling thread.

```toml
# Opt in to the rayon backend on native targets.
novtb = { version = "0.1", features = ["rayon"] }
```

This project is licensed under either of

- BSD-3-Clause License (see [LICENSE](LICENSE.md))
- Apache License, Version 2.0 (see [LICENSE](LICENSE-APACHE.md))

at your option.
