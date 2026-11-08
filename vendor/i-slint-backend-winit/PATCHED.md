# i-slint-backend-winit 1.18.1, patched

The crate as published on crates.io, with one change in `renderer/sw.rs`: the length of a row comes from
the buffer instead of being taken as the window's width. See `vendor/softbuffer/PATCHED.md` for why.
Slint is pinned to this exact version, so an upgrade means carrying that line over.
