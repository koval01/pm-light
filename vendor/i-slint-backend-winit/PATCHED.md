# i-slint-backend-winit 1.18.1, patched

The crate as published on crates.io, with these changes:

- `renderer/sw.rs`: the length of a row comes from the buffer instead of being taken as the window's width. See
  `vendor/softbuffer/PATCHED.md` for why.
- `winitwindowadapter.rs`, `RedrawRequested`: on X11 and Wayland, a redraw the toolkit did not ask for marks the whole
  window dirty first. X11 sends one, through winit, when a child window, a webview, moves off the area it covered,
  and the system has lost what was shown there; only the changed parts were drawn and shown, so the rest stayed
  black (seen in the long run's pictures on Linux).

Slint is pinned to this exact version, so an upgrade means carrying these changes over.
