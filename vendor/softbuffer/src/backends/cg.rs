//! Softbuffer implementation using CoreGraphics.
use crate::backend_interface::*;
use crate::error::InitError;
use crate::{Rect, SoftBufferError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, Message};
use objc2_core_foundation::CGPoint;
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueChangeNewKey,
    NSKeyValueObservingOptions, NSNumber, NSObject, NSObjectNSKeyValueObserverRegistration,
    NSString, NSValue,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CATransaction};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use std::ffi::c_void;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops::Deref;
use std::ptr::{self, NonNull};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SoftbufferObserver"]
    #[ivars = SendCALayer]
    #[derive(Debug)]
    struct Observer;

    /// NSKeyValueObserving
    impl Observer {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            self.update(key_path, change);
        }
    }
);

impl Observer {
    fn new(layer: &CALayer) -> Retained<Self> {
        let this = Self::alloc().set_ivars(SendCALayer(layer.retain()));
        unsafe { msg_send![super(this), init] }
    }

    fn update(
        &self,
        key_path: Option<&NSString>,
        change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
    ) {
        let layer = self.ivars();

        let change =
            change.expect("requested a change dictionary in `addObserver`, but none was provided");
        let new = change
            .objectForKey(unsafe { NSKeyValueChangeNewKey })
            .expect("requested change dictionary did not contain `NSKeyValueChangeNewKey`");

        // NOTE: Setting these values usually causes a quarter second animation to occur, which is
        // undesirable.
        //
        // However, since we're setting them inside an observer, there already is a transaction
        // ongoing, and as such we don't need to wrap this in a `CATransaction` ourselves.

        if key_path == Some(ns_string!("contentsScale")) {
            let new = new.downcast::<NSNumber>().unwrap();
            let scale_factor = new.as_cgfloat();

            // Set the scale factor of the layer to match the root layer when it changes (e.g. if
            // moved to a different monitor, or monitor settings changed).
            layer.setContentsScale(scale_factor);
        } else if key_path == Some(ns_string!("bounds")) {
            let new = new.downcast::<NSValue>().unwrap();
            let bounds = new.get_rect().expect("new bounds value was not CGRect");

            // Set `bounds` and `position` so that the new layer is inside the superlayer.
            //
            // This differs from just setting the `bounds`, as it also takes into account any
            // translation that the superlayer may have that we'd want to preserve.
            layer.setFrame(bounds);
        } else {
            panic!("unknown observed keypath {key_path:?}");
        }
    }
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceCreate(properties: *const c_void) -> *mut c_void;
    fn IOSurfaceLock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceUnlock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceGetBaseAddress(surface: *mut c_void) -> *mut c_void;
    fn IOSurfaceGetBytesPerRow(surface: *mut c_void) -> usize;
    fn IOSurfaceSetValue(surface: *mut c_void, key: *const c_void, value: *const c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

const PIXEL_FORMAT_BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const BYTES_PER_PIXEL: usize = 4;
/// The window server shows nothing at all for a surface with rows of any other multiple.
const ROW_ALIGNMENT: usize = 64;

/// Patched in this repository: the original made a new buffer for every frame, handed it over as an
/// image for Core Animation to copy, and so left the whole window to be repainted each time. A
/// window this size is 12 MB, so three of those lived at once. This one draws into two surfaces
/// taken in turn, which the system shows as they are, and only what changed is drawn again.
#[derive(Debug)]
struct Frame {
    surface: NonNull<c_void>,
    width: usize,
    height: usize,
    /// In pixels.
    stride: usize,
}

impl Frame {
    fn new(width: usize, height: usize) -> Option<Self> {
        let bytes_per_row = (width * BYTES_PER_PIXEL).next_multiple_of(ROW_ALIGNMENT);
        let number = |value: usize| NSNumber::new_usize(value);
        let keys = [
            ns_string!("IOSurfaceWidth"),
            ns_string!("IOSurfaceHeight"),
            ns_string!("IOSurfaceBytesPerElement"),
            ns_string!("IOSurfaceBytesPerRow"),
            ns_string!("IOSurfacePixelFormat"),
        ];
        let values = [
            number(width),
            number(height),
            number(BYTES_PER_PIXEL),
            number(bytes_per_row),
            NSNumber::new_u32(PIXEL_FORMAT_BGRA),
        ];
        let values = [&*values[0], &*values[1], &*values[2], &*values[3], &*values[4]];
        let properties = NSDictionary::from_slices(&keys, &values);
        // SAFETY: The dictionary lives through the call, and the surface is ours to release.
        let surface = NonNull::new(unsafe {
            IOSurfaceCreate(Retained::as_ptr(&properties).cast())
        })?;
        let stride = bytes_per_row / BYTES_PER_PIXEL;
        let frame = Self { surface, width, height, stride };
        // SAFETY: The surface is valid.
        if unsafe { IOSurfaceGetBytesPerRow(surface.as_ptr()) } != bytes_per_row {
            return None;
        }
        // SAFETY: The surface is valid, and these are a key and a value it knows.
        unsafe {
            IOSurfaceSetValue(
                surface.as_ptr(),
                Retained::as_ptr(&NSString::from_str("IOSurfaceColorSpace")).cast(),
                Retained::as_ptr(&NSString::from_str("kCGColorSpaceSRGB")).cast(),
            );
        }
        Some(frame)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        // SAFETY: Made by `IOSurfaceCreate`, and released only here.
        unsafe { CFRelease(self.surface.as_ptr()) };
    }
}

// SAFETY: IOSurfaces are made to be shared, across threads and even processes.
unsafe impl Send for Frame {}

#[derive(Debug)]
pub struct CGImpl<D, W> {
    /// Our layer.
    layer: SendCALayer,
    /// The layer that our layer was created from.
    ///
    /// Can also be retrieved from `layer.superlayer()`.
    root_layer: SendCALayer,
    observer: Retained<Observer>,
    /// The frames taken in turn.
    frames: [Option<Frame>; 2],
    /// The frame on screen.
    shown: usize,
    /// The width of the underlying buffer.
    width: usize,
    /// The height of the underlying buffer.
    height: usize,
    window_handle: W,
    _display: PhantomData<D>,
}

impl<D, W> Drop for CGImpl<D, W> {
    fn drop(&mut self) {
        // SAFETY: Registered in `new`, must be removed before the observer is deallocated.
        unsafe {
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("contentsScale"));
            self.root_layer
                .removeObserver_forKeyPath(&self.observer, ns_string!("bounds"));
        }
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for CGImpl<D, W> {
    type Context = D;
    type Buffer<'a>
        = BufferImpl<'a, D, W>
    where
        Self: 'a;

    fn new(window_src: W, _display: &D) -> Result<Self, InitError<W>> {
        // `NSView`/`UIView` can only be accessed from the main thread.
        let _mtm = MainThreadMarker::new().ok_or(SoftBufferError::PlatformError(
            Some("can only access Core Graphics handles from the main thread".to_string()),
            None,
        ))?;

        let root_layer = match window_src.window_handle()?.as_raw() {
            RawWindowHandle::AppKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `AppKitWindowHandle` contains a valid pointer to an `NSView`.
                //
                // We use `NSObject` here to avoid importing `objc2-app-kit`.
                let view: &NSObject = unsafe { handle.ns_view.cast().as_ref() };

                // Force the view to become layer backed
                let _: () = unsafe { msg_send![view, setWantsLayer: Bool::YES] };

                // SAFETY: `-[NSView layer]` returns an optional `CALayer`
                let layer: Option<Retained<CALayer>> = unsafe { msg_send![view, layer] };
                layer.expect("failed making the view layer-backed")
            }
            RawWindowHandle::UiKit(handle) => {
                // SAFETY: The pointer came from `WindowHandle`, which ensures that the
                // `UiKitWindowHandle` contains a valid pointer to an `UIView`.
                //
                // We use `NSObject` here to avoid importing `objc2-ui-kit`.
                let view: &NSObject = unsafe { handle.ui_view.cast().as_ref() };

                // SAFETY: `-[UIView layer]` returns `CALayer`
                let layer: Retained<CALayer> = unsafe { msg_send![view, layer] };
                layer
            }
            _ => return Err(InitError::Unsupported(window_src)),
        };

        // Add a sublayer, to avoid interfering with the root layer, since setting the contents of
        // e.g. a view-controlled layer is brittle.
        let layer = CALayer::new();
        root_layer.addSublayer(&layer);

        // Set the anchor point and geometry. Softbuffer's uses a coordinate system with the origin
        // in the top-left corner.
        //
        // NOTE: This doesn't really matter unless we start modifying the `position` of our layer
        // ourselves, but it's nice to have in place.
        layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        layer.setGeometryFlipped(true);

        // Do not use auto-resizing mask.
        //
        // This is done to work around a bug in macOS 14 and above, where views using auto layout
        // may end up setting fractional values as the bounds, and that in turn doesn't propagate
        // properly through the auto-resizing mask and with contents gravity.
        //
        // Instead, we keep the bounds of the layer in sync with the root layer using an observer,
        // see below.
        //
        // layer.setAutoresizingMask(kCALayerHeightSizable | kCALayerWidthSizable);

        let observer = Observer::new(&layer);
        // Observe changes to the root layer's bounds and scale factor, and apply them to our layer.
        //
        // The previous implementation updated the scale factor inside `resize`, but this works
        // poorly with transactions, and is generally inefficient. Instead, we update the scale
        // factor only when needed because the super layer's scale factor changed.
        //
        // Note that inherent in this is an explicit design decision: We control the `bounds` and
        // `contentsScale` of the layer directly, and instead let the `resize` call that the user
        // controls only be the size of the underlying buffer.
        //
        // SAFETY: Observer deregistered in `Drop` before the observer object is deallocated.
        unsafe {
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("contentsScale"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
            root_layer.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("bounds"),
                NSKeyValueObservingOptions::New | NSKeyValueObservingOptions::Initial,
                ptr::null_mut(),
            );
        }

        // Set the content so that it is placed in the top-left corner if it does not have the same
        // size as the surface itself.
        //
        // TODO(madsmtm): Consider changing this to `kCAGravityResize` to stretch the content if
        // resized to something that doesn't fit, see #177.
        layer.setContentsGravity(unsafe { kCAGravityTopLeft });

        // A frame's alpha means nothing: the window has its own shape.
        layer.setOpaque(true);

        // Grab initial width and height from the layer (whose properties have just been initialized
        // by the observer using `NSKeyValueObservingOptionInitial`).
        let size = layer.bounds().size;
        let scale_factor = layer.contentsScale();
        let width = (size.width * scale_factor) as usize;
        let height = (size.height * scale_factor) as usize;

        Ok(Self {
            layer: SendCALayer(layer),
            root_layer: SendCALayer(root_layer),
            observer,
            frames: [None, None],
            shown: 0,
            width,
            height,
            _display: PhantomData,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        self.width = width.get() as usize;
        self.height = height.get() as usize;
        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let index = 1 - self.shown;
        let (width, height) = (self.width, self.height);
        let fits = |frame: &Option<Frame>| {
            frame
                .as_ref()
                .is_some_and(|frame| frame.width == width && frame.height == height)
        };
        let shown_fits = fits(&self.frames[self.shown]);
        let mut age = if shown_fits { 2 } else { 0 };
        if !fits(&self.frames[index]) {
            age = 0;
            self.frames[index] = None;
            self.frames[index] = Frame::new(width, height);
        }
        let frame = self.frames[index].as_ref().ok_or_else(|| {
            SoftBufferError::PlatformError(Some("no surface for this size".into()), None)
        })?;
        let (surface, stride) = (frame.surface, frame.stride);
        // SAFETY: The surface is valid, and unlocked when the buffer goes.
        if unsafe { IOSurfaceLock(surface.as_ptr(), 0, ptr::null_mut()) } != 0 {
            return Err(SoftBufferError::PlatformError(
                Some("could not lock a surface".into()),
                None,
            ));
        }
        // SAFETY: A locked surface has its memory in place.
        let pixels = NonNull::new(unsafe { IOSurfaceGetBaseAddress(surface.as_ptr()) })
            .ok_or_else(|| {
                SoftBufferError::PlatformError(Some("a surface has no memory".into()), None)
            })?
            .cast();
        Ok(BufferImpl {
            imp: self,
            index,
            pixels,
            length: stride * height,
            age,
            presented: false,
        })
    }
}

#[derive(Debug)]
pub struct BufferImpl<'a, D, W> {
    imp: &'a mut CGImpl<D, W>,
    index: usize,
    pixels: NonNull<u32>,
    length: usize,
    age: u8,
    presented: bool,
}

// SAFETY: The pixels belong to a frame, which may be sent, and are reached only through this buffer.
unsafe impl<D: Send, W: Send> Send for BufferImpl<'_, D, W> {}

impl<D, W> BufferImpl<'_, D, W> {
    fn frame(&self) -> Option<&Frame> {
        self.imp.frames[self.index].as_ref()
    }
}

impl<D, W> Drop for BufferImpl<'_, D, W> {
    fn drop(&mut self) {
        if self.presented {
            return;
        }
        if let Some(frame) = self.frame() {
            // SAFETY: Locked in `buffer_mut`, and the pixels are not used after this.
            unsafe { IOSurfaceUnlock(frame.surface.as_ptr(), 0, ptr::null_mut()) };
        }
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'_, D, W> {
    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.imp.width as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.imp.height as u32).unwrap()
    }

    #[inline]
    fn pixels(&self) -> &[u32] {
        // SAFETY: The surface stays locked for as long as this buffer lives.
        unsafe { std::slice::from_raw_parts(self.pixels.as_ptr(), self.length) }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        // SAFETY: The surface stays locked for as long as this buffer lives.
        unsafe { std::slice::from_raw_parts_mut(self.pixels.as_ptr(), self.length) }
    }

    fn age(&self) -> u8 {
        self.age
    }

    fn present(mut self) -> Result<(), SoftBufferError> {
        self.presented = true;
        let Some(frame) = self.frame() else {
            return Ok(());
        };
        let surface = frame.surface;
        // SAFETY: Locked in `buffer_mut`, and `drop` sees that this is done.
        unsafe { IOSurfaceUnlock(surface.as_ptr(), 0, ptr::null_mut()) };

        // The CALayer has a default action associated with a change in the layer contents, causing
        // a quarter second fade transition to happen every time a new buffer is applied. This can
        // be avoided by wrapping the operation in a transaction and disabling all actions.
        CATransaction::begin();
        CATransaction::setDisableActions(true);

        // SAFETY: An IOSurface is a valid class for `contents`.
        unsafe {
            self.imp
                .layer
                .setContents(Some(&*surface.as_ptr().cast::<AnyObject>()))
        };

        CATransaction::commit();

        self.imp.shown = self.index;
        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.present()
    }
}

#[derive(Debug)]
struct SendCALayer(Retained<CALayer>);

// SAFETY: CALayer is dubiously thread safe, like most things in Core Animation.
// But since we make sure to do our changes within a CATransaction, it is
// _probably_ fine for us to use CALayer from different threads.
//
// See also:
// https://developer.apple.com/documentation/quartzcore/catransaction/1448267-lock?language=objc
// https://stackoverflow.com/questions/76250226/how-to-render-content-of-calayer-on-a-background-thread
unsafe impl Send for SendCALayer {}
// SAFETY: Same as above.
unsafe impl Sync for SendCALayer {}

impl Deref for SendCALayer {
    type Target = CALayer;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
