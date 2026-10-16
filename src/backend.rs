/*
 * // Copyright (c) Radzivon Bartoshyk 6/2025. All rights reserved.
 * //
 * // Redistribution and use in source and binary forms, with or without modification,
 * // are permitted provided that the following conditions are met:
 * //
 * // 1.  Redistributions of source code must retain the above copyright notice, this
 * // list of conditions and the following disclaimer.
 * //
 * // 2.  Redistributions in binary form must reproduce the above copyright notice,
 * // this list of conditions and the following disclaimer in the documentation
 * // and/or other materials provided with the distribution.
 * //
 * // 3.  Neither the name of the copyright holder nor the names of its
 * // contributors may be used to endorse or promote products derived from
 * // this software without specific prior written permission.
 * //
 * // THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * // AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * // IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * // DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
 * // FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * // DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * // SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * // CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * // OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * // OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

//! Where a zone runs. Every parallel entry point splits its work into zones
//! the same way and hands all but the first zone to [`Scope::spawn`]; the
//! first zone runs inside the [`scope`] closure itself.
//!
//! - Default: one scoped OS thread per zone, pinned round-robin to a core.
//! - `rayon` feature, and always on wasm (which cannot spawn `std::thread`s):
//!   one task per zone on rayon's global pool. On the web, `wasm-bindgen-rayon`
//!   backs that pool with Web Workers; without a pool rayon runs every zone on
//!   the calling thread.

#[cfg(not(any(feature = "rayon", target_family = "wasm")))]
mod imp {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CORE_RING_INDEX: AtomicUsize = AtomicUsize::new(0);

    pub(crate) struct Scope<'a, 'scope, 'env> {
        inner: &'scope std::thread::Scope<'scope, 'env>,
        cores: &'a [core_affinity::CoreId],
    }

    impl<'scope> Scope<'_, 'scope, '_> {
        pub(crate) fn spawn<F>(&self, f: F)
        where
            F: FnOnce() + Send + 'scope,
        {
            let offset = CORE_RING_INDEX.fetch_add(1, Ordering::Relaxed);
            let core = self.cores.get(offset % self.cores.len().max(1)).cloned();
            self.inner.spawn(move || {
                if let Some(core_id) = core {
                    core_affinity::set_for_current(core_id);
                }
                f();
            });
        }
    }

    pub(crate) fn scope<'env, F, T>(f: F) -> T
    where
        F: for<'a, 'scope> FnOnce(&Scope<'a, 'scope, 'env>) -> T + Send,
        T: Send,
    {
        let cores = core_affinity::get_core_ids().unwrap_or_default();
        std::thread::scope(|s| {
            f(&Scope {
                inner: s,
                cores: &cores,
            })
        })
    }
}

#[cfg(any(feature = "rayon", target_family = "wasm"))]
mod imp {
    // Native builds keep the `core_affinity` dependency when `rayon` is on,
    // because a feature cannot remove a dependency.
    #[cfg(not(target_family = "wasm"))]
    use core_affinity as _;

    pub(crate) struct Scope<'a, 'scope> {
        inner: &'a rayon::Scope<'scope>,
    }

    impl<'scope> Scope<'_, 'scope> {
        pub(crate) fn spawn<F>(&self, f: F)
        where
            F: FnOnce() + Send + 'scope,
        {
            self.inner.spawn(move |_| f());
        }
    }

    pub(crate) fn scope<'scope, F, T>(f: F) -> T
    where
        F: for<'a> FnOnce(&Scope<'a, 'scope>) -> T + Send,
        T: Send,
    {
        rayon::scope(|s| f(&Scope { inner: s }))
    }
}

pub(crate) use imp::scope;
