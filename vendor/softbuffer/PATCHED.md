# softbuffer 0.4.8, patched

The crate as published on crates.io, without its examples, benchmarks and CI files. The only change is
in `src/backends/cg.rs`, the macOS backend.

The original allocates a new buffer for every frame, wraps it in a `CGImage` and hands it to Core
Animation, which copies it. It reports the age of every buffer as 0, so the toolkit repaints the whole
window each time. At 2560 × 1600 a frame is 12 MB, and several were alive at once.

Here the window draws into two `IOSurface`s taken in turn, which Core Animation shows without a copy.
The age is real, so only what changed is drawn again. Both frames stay in memory: marking the idle one
volatile saved one frame, and a frame the system can take away is not worth the risk of a flicker.

A row's length has to be a multiple of 64 bytes: at any other width the window server shows an empty
window, and nothing reports it. So the buffer can be longer than `width * height`, and whoever draws
works out the row from its length. `vendor/i-slint-backend-winit` holds the one line that does, in
`renderer/sw.rs`. Other backends still give exactly `width * height`.

Drop this patch once upstream reuses its buffers on macOS.
