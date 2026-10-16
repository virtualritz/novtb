//! Prints how many distinct threads ran novtb's zones. A quick check that a
//! backend actually runs in parallel on a given target:
//!
//! ```sh
//! cargo run --example zone_threads
//! cargo build --example zone_threads --target wasm32-wasip1-threads
//! wasmtime run -W threads=y -S threads=y target/wasm32-wasip1-threads/debug/examples/zone_threads.wasm
//! ```

use novtb::{ParallelZonedIterator, TbSliceMut, ThreadPool};
use std::collections::HashSet;
use std::sync::Mutex;

fn main() {
    let pool = ThreadPool::new(4);
    let seen = Mutex::new(HashSet::new());
    let mut rows = vec![0u64; 4096 * 64];
    rows.tb_par_chunks_exact_mut(4096)
        .for_each_enumerated(&pool, |y, row| {
            seen.lock().unwrap().insert(std::thread::current().id());
            // Enough work per row that the zones overlap in time.
            for (x, v) in row.iter_mut().enumerate() {
                *v = (0..200u64).fold((x + y) as u64, |a, k| {
                    a.wrapping_mul(6364136223846793005).wrapping_add(k)
                });
            }
        });
    let checksum = rows.iter().fold(0u64, |a, &v| a ^ v);
    println!(
        "zones ran on {} distinct thread(s) of 4 requested, checksum {checksum:016x}",
        seen.lock().unwrap().len()
    );
}
