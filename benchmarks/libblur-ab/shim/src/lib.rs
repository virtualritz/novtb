//! rayon-backed stand-in for the novtb API surface libblur 0.20 and 0.24 use.
//!
//! `NOVTB_SHIM=static` (default) keeps novtb's partitioning -- one group per
//! requested thread, same group size -- and runs the groups on rayon's global
//! pool, so the only difference to novtb is pool reuse vs. spawn-per-call.
//! `NOVTB_SHIM=steal` hands every chunk to rayon and lets it split adaptively.

use rayon::prelude::*;
use std::sync::OnceLock;

fn steal() -> bool {
    static MODE: OnceLock<bool> = OnceLock::new();
    *MODE.get_or_init(|| std::env::var("NOVTB_SHIM").is_ok_and(|v| v == "steal"))
}

pub struct ThreadPool {
    amount: usize,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        ThreadPool {
            amount: size.max(1),
        }
    }

    pub fn thread_count(&self) -> usize {
        self.amount
    }

    pub fn parallel_for<'scope, F>(&self, job: F)
    where
        F: 'scope + Fn(usize) + Send + Sync,
    {
        if self.amount <= 1 {
            job(0);
            return;
        }
        (0..self.amount).into_par_iter().for_each(job);
    }
}

pub struct ChunksExactMut<'data, T: Send> {
    chunk_size: usize,
    slice: &'data mut [T],
    rem: &'data mut [T],
}

impl<'data, T: Send> ChunksExactMut<'data, T> {
    pub fn new(chunk_size: usize, slice: &'data mut [T]) -> Self {
        let rem_len = slice.len() % chunk_size;
        let len = slice.len() - rem_len;
        let (slice, rem) = slice.split_at_mut(len);
        Self {
            chunk_size,
            slice,
            rem,
        }
    }

    pub fn take(mut self, n: usize) -> Self {
        let max_len = (self.chunk_size * n).min(self.slice.len());
        let (slice, rem) = self.slice.split_at_mut(max_len);
        self.slice = slice;
        self.rem = rem;
        self
    }

    pub fn skip(mut self, n: usize) -> Self {
        let skip_len = (self.chunk_size * n).min(self.slice.len());
        self.slice = &mut self.slice[skip_len..];
        self
    }

    pub fn len(&self) -> usize {
        self.slice.len() / self.chunk_size
    }

    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }

    pub fn into_remainder(self) -> &'data mut [T] {
        self.rem
    }

    pub fn remainder(&mut self) -> &mut [T] {
        self.rem
    }

    pub fn take_remainder(&mut self) -> &'data mut [T] {
        std::mem::take(&mut self.rem)
    }
}

pub struct ChunksMut<'data, T: Send> {
    chunk_size: usize,
    slice: &'data mut [T],
}

impl<'data, T: Send> ChunksMut<'data, T> {
    pub fn new(chunk_size: usize, slice: &'data mut [T]) -> Self {
        Self { chunk_size, slice }
    }

    pub fn take(mut self, n: usize) -> Self {
        let max_len = (self.chunk_size * n).min(self.slice.len());
        self.slice = &mut self.slice[..max_len];
        self
    }

    pub fn skip(mut self, n: usize) -> Self {
        let skip_len = (self.chunk_size * n).min(self.slice.len());
        self.slice = &mut self.slice[skip_len..];
        self
    }

    pub fn len(&self) -> usize {
        self.slice.len().div_ceil(self.chunk_size)
    }

    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }
}

pub trait TbSliceMut<T: Send> {
    fn as_tb_slice_mut(&mut self) -> &mut [T];

    fn tb_par_chunks_exact_mut(&mut self, chunk_size: usize) -> ChunksExactMut<'_, T> {
        assert!(chunk_size != 0, "chunk_size must not be zero");
        ChunksExactMut::new(chunk_size, self.as_tb_slice_mut())
    }

    fn tb_par_chunks_mut(&mut self, chunk_size: usize) -> ChunksMut<'_, T> {
        assert!(chunk_size != 0, "chunk_size must not be zero");
        ChunksMut::new(chunk_size, self.as_tb_slice_mut())
    }
}

impl<T: Send> TbSliceMut<T> for [T] {
    fn as_tb_slice_mut(&mut self) -> &mut [T] {
        self
    }
}

pub trait ParallelZonedIterator: Sized + Send {
    type Item: Send;

    fn for_each<'scope, F>(self, thread_pool: &ThreadPool, f: F)
    where
        F: 'scope + Fn(Self::Item) + Send + Sync;

    fn for_each_enumerated<'scope, F>(self, thread_pool: &ThreadPool, f: F)
    where
        F: 'scope + Fn(usize, Self::Item) + Send + Sync;

    fn for_each_with_context<'scope, F, C, CF>(self, pool: &ThreadPool, make_context: CF, f: F)
    where
        F: 'scope + Fn(&mut C, Self::Item) + Send + Sync,
        C: Send,
        CF: Fn() -> C + Send + Sync;

    fn for_each_enumerated_with_context<'scope, F, C, CF>(
        self,
        pool: &ThreadPool,
        make_context: CF,
        f: F,
    ) where
        F: 'scope + Fn(usize, &mut C, Self::Item) + Send + Sync,
        C: Send,
        CF: Fn() -> C + Send + Sync;
}

/// Same as novtb: the group size each of `threads` workers gets, rounded up
/// to whole chunks.
fn group_size(rows: usize, threads: usize, chunk_size: usize) -> usize {
    rows.div_ceil(threads).div_ceil(chunk_size) * chunk_size
}

/// Runs `f` over the chunks of `s` in order, with one context for all of them.
fn seq<'d, T: Send, C, F>(s: &'d mut [T], chunk_size: usize, exact: bool, base: usize, ctx: &mut C, f: &F)
where
    F: Fn(usize, &mut C, &'d mut [T]) + Send + Sync,
{
    if exact {
        for (i, c) in s.chunks_exact_mut(chunk_size).enumerate() {
            f(base + i, ctx, c);
        }
    } else {
        for (i, c) in s.chunks_mut(chunk_size).enumerate() {
            f(base + i, ctx, c);
        }
    }
}

/// Shared driver. `exact` selects chunks_exact (drop tail) vs. chunks (keep
/// tail). `static` mode makes one context per zone, as novtb does; `steal`
/// mode uses rayon's `for_each_init`, which makes one per split job.
fn drive<'d, T: Send, C: Send, CF, F>(
    slice: &'d mut [T],
    chunk_size: usize,
    pool: &ThreadPool,
    exact: bool,
    make_context: CF,
    f: F,
) where
    CF: Fn() -> C + Send + Sync,
    F: Fn(usize, &mut C, &'d mut [T]) + Send + Sync,
{
    if slice.is_empty() {
        return;
    }
    if pool.amount <= 1 {
        seq(slice, chunk_size, exact, 0, &mut make_context(), &f);
        return;
    }
    if steal() {
        if exact {
            slice
                .par_chunks_exact_mut(chunk_size)
                .enumerate()
                .for_each_init(&make_context, |ctx, (i, c)| f(i, ctx, c));
        } else {
            slice
                .par_chunks_mut(chunk_size)
                .enumerate()
                .for_each_init(&make_context, |ctx, (i, c)| f(i, ctx, c));
        }
        return;
    }
    let rows = (slice.len() / chunk_size).max(1) * chunk_size;
    let gs = group_size(rows, pool.amount, chunk_size);
    if gs >= rows {
        seq(slice, chunk_size, exact, 0, &mut make_context(), &f);
        return;
    }
    let per_group = gs / chunk_size;
    slice
        .par_chunks_mut(gs)
        .with_max_len(1)
        .enumerate()
        .for_each(|(g, s)| seq(s, chunk_size, exact, g * per_group, &mut make_context(), &f));
}

macro_rules! zoned {
    ($ty:ident, $exact:expr) => {
        impl<'data, T: Send> ParallelZonedIterator for $ty<'data, T> {
            type Item = &'data mut [T];

            fn for_each<'scope, F>(self, pool: &ThreadPool, f: F)
            where
                F: 'scope + Fn(Self::Item) + Send + Sync,
            {
                drive(self.slice, self.chunk_size, pool, $exact, || (), |_, _, c| f(c));
            }

            fn for_each_enumerated<'scope, F>(self, pool: &ThreadPool, f: F)
            where
                F: 'scope + Fn(usize, Self::Item) + Send + Sync,
            {
                drive(self.slice, self.chunk_size, pool, $exact, || (), |i, _, c| f(i, c));
            }

            fn for_each_with_context<'scope, F, C, CF>(self, pool: &ThreadPool, make_context: CF, f: F)
            where
                F: 'scope + Fn(&mut C, Self::Item) + Send + Sync,
                C: Send,
                CF: Fn() -> C + Send + Sync,
            {
                drive(self.slice, self.chunk_size, pool, $exact, make_context, |_, x, c| f(x, c));
            }

            fn for_each_enumerated_with_context<'scope, F, C, CF>(
                self,
                pool: &ThreadPool,
                make_context: CF,
                f: F,
            ) where
                F: 'scope + Fn(usize, &mut C, Self::Item) + Send + Sync,
                C: Send,
                CF: Fn() -> C + Send + Sync,
            {
                drive(self.slice, self.chunk_size, pool, $exact, make_context, f);
            }
        }
    };
}

zoned!(ChunksExactMut, true);
zoned!(ChunksMut, false);
