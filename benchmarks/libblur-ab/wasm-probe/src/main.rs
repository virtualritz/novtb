//! One stack blur with four threads requested. On a target without
//! `std::thread` this is where novtb's `thread::scope` spawn fails.

use libblur::{AnisotropicRadius, BlurImageMut, FastBlurChannels, ThreadingPolicy};
use std::num::NonZeroUsize;

fn main() {
    let (w, h) = (256u32, 256u32);
    let mut px = vec![128u8; (w * h * 4) as usize];
    px[0] = 255;
    let mut img = BlurImageMut::borrow(&mut px, w, h, FastBlurChannels::Channels4);
    let policy = ThreadingPolicy::Fixed(NonZeroUsize::new(4).unwrap());
    libblur::stack_blur(&mut img, AnisotropicRadius::new(10), policy).unwrap();
    println!("ok: stack_blur with {policy} returned, px[0] = {}", px[0]);
}
