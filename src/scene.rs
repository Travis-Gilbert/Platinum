//! The in-scene renderer: a Servo rendering
//! context whose frames are `IOSurface`s, handed to GPUI as `CVPixelBuffer`s
//! for `gpui::surface()`, so the page is an element in GPUI's own scene.
//!
//! SOURCING: surfman's `SwapChain` and `Device::native_surface`, set up as
//! Servo's own `SoftwareRenderingContext` is but on the hardware adapter, and
//! CoreVideo's `CVPixelBufferCreateWithIOSurface`. Written here because none
//! of Servo's rendering contexts hands out its front buffer. Ported
//! unchanged from Theorem UI's desktop host (`desktop/src/scene.rs`).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ptr::{self, NonNull};
use std::rc::Rc;
use std::sync::Arc;

use dpi::PhysicalSize;
use euclid::default::Size2D;
use gleam::gl::{self, Gl};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreateWithIOSurface, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVReturnSuccess,
};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};
use surfman::chains::{PreserveBuffer, SwapChain, SwapChainAPI as _};
use surfman::{
    Connection, Context, ContextAttributeFlags, ContextAttributes, Device, Error, GLApi, GLVersion,
    Surface, SurfaceAccess, SurfaceTexture, SurfaceType,
};

/// Frames handed to GPUI and not yet given back to Servo to draw into. GPUI
/// may still be drawing the one before the newest when Servo presents again.
const FRAMES_IN_FLIGHT: usize = 3;

pub struct SceneRenderingContext {
    size: Cell<PhysicalSize<u32>>,
    gleam: Rc<dyn Gl>,
    glow: Arc<glow::Context>,
    device: RefCell<Device>,
    context: RefCell<Context>,
    swap_chain: SwapChain<Device>,
    /// Presented surfaces, oldest first; the newest is `frame`.
    shown: RefCell<VecDeque<Surface>>,
    frame: RefCell<Option<CFRetained<CVPixelBuffer>>>,
}

impl SceneRenderingContext {
    pub fn new(size: PhysicalSize<u32>) -> Result<Self, Error> {
        let connection = Connection::new()?;
        let adapter = connection.create_adapter()?;
        let device = connection.create_device(&adapter)?;
        let version = match connection.gl_api() {
            GLApi::GLES => GLVersion { major: 3, minor: 0 },
            GLApi::GL => GLVersion { major: 3, minor: 2 },
        };
        let flags = ContextAttributeFlags::ALPHA
            | ContextAttributeFlags::DEPTH
            | ContextAttributeFlags::STENCIL;
        let descriptor = device.create_context_descriptor(&ContextAttributes { flags, version })?;
        let mut context = device.create_context(&descriptor, None)?;
        let surface = device.create_surface(
            &context,
            SurfaceAccess::GPUOnly,
            SurfaceType::Generic {
                size: Size2D::new(size.width as i32, size.height as i32),
            },
        )?;
        if let Err((error, mut surface)) = device.bind_surface_to_context(&mut context, surface) {
            let _ = device.destroy_surface(&mut context, &mut surface);
            let _ = device.destroy_context(&mut context);
            return Err(error);
        }
        device.make_context_current(&context)?;
        // SAFETY: the loader returns the context's own entry points, and the
        // context outlives both function tables.
        let gleam = unsafe {
            match connection.gl_api() {
                GLApi::GL => gl::GlFns::load_with(|name| device.get_proc_address(&context, name)),
                GLApi::GLES => {
                    gl::GlesFns::load_with(|name| device.get_proc_address(&context, name))
                }
            }
        };
        let glow = unsafe {
            glow::Context::from_loader_function(|name| device.get_proc_address(&context, name))
        };
        let swap_chain = SwapChain::create_attached(&device, &mut context, SurfaceAccess::GPUOnly)?;
        Ok(Self {
            size: Cell::new(size),
            gleam,
            glow: Arc::new(glow),
            device: RefCell::new(device),
            context: RefCell::new(context),
            swap_chain,
            shown: RefCell::default(),
            frame: RefCell::default(),
        })
    }

    /// The last presented frame, for `gpui::surface()`.
    pub fn frame(&self) -> Option<CFRetained<CVPixelBuffer>> {
        self.frame.borrow().clone()
    }

    /// The color most of the last frame's top row shows, as RGB: what sits
    /// under the toolbar. Sampled across the row, so a logo or a link in a
    /// page's header does not decide it.
    pub fn top_color(&self) -> Option<[u8; 3]> {
        const SAMPLES: usize = 32;
        let frame = self.frame()?;
        let (width, height) = (
            CVPixelBufferGetWidth(&frame),
            CVPixelBufferGetHeight(&frame),
        );
        if width < 2 || height == 0 {
            return None;
        }
        let flags = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: the buffer is locked while its memory is read, and every
        // read is a pixel of its last row, the top of the picture.
        let samples: Vec<[u8; 4]> = unsafe {
            if CVPixelBufferLockBaseAddress(&frame, flags) != kCVReturnSuccess {
                return None;
            }
            let base = CVPixelBufferGetBaseAddress(&frame).cast::<[u8; 4]>();
            let samples = (!base.is_null()).then(|| {
                let top = base.byte_add((height - 1) * CVPixelBufferGetBytesPerRow(&frame));
                (0..SAMPLES)
                    .map(|i| *top.add(i * (width - 1) / (SAMPLES - 1)))
                    .collect()
            });
            CVPixelBufferUnlockBaseAddress(&frame, flags);
            samples?
        };
        let most = samples
            .iter()
            .max_by_key(|sample| samples.iter().filter(|other| other == sample).count())?;
        Some([most[2], most[1], most[0]])
    }

    /// The last frame's pixels, for `read` to look at while the buffer is
    /// locked: BGRA bytes, rows bottom first, with the width, the height and
    /// the bytes to a row. `None` before the first frame.
    pub fn with_pixels<R>(&self, read: impl FnOnce(&[u8], usize, usize, usize) -> R) -> Option<R> {
        let frame = self.frame()?;
        let (width, height) = (
            CVPixelBufferGetWidth(&frame),
            CVPixelBufferGetHeight(&frame),
        );
        let flags = CVPixelBufferLockFlags::ReadOnly;
        // SAFETY: the buffer is locked while its memory is read, a locked
        // buffer's base address covers every row, and `read` cannot keep the
        // slice past the unlock.
        unsafe {
            if CVPixelBufferLockBaseAddress(&frame, flags) != kCVReturnSuccess {
                return None;
            }
            let base = CVPixelBufferGetBaseAddress(&frame).cast::<u8>();
            let read = (!base.is_null()).then(|| {
                let stride = CVPixelBufferGetBytesPerRow(&frame);
                read(
                    std::slice::from_raw_parts(base, stride * height),
                    width,
                    height,
                    stride,
                )
            });
            CVPixelBufferUnlockBaseAddress(&frame, flags);
            read
        }
    }

    fn framebuffer(&self) -> gl::GLuint {
        let device = self.device.borrow();
        let context = self.context.borrow();
        device
            .context_surface_info(&context)
            .ok()
            .flatten()
            .and_then(|info| info.framebuffer_object)
            .map_or(0, |framebuffer| framebuffer.0.get())
    }
}

/// A `CVPixelBuffer` over an `IOSurface`, sharing its memory.
fn pixel_buffer(device: &Device, surface: &Surface) -> Option<CFRetained<CVPixelBuffer>> {
    let io_surface = device.native_surface(surface).0;
    let mut buffer = ptr::null_mut();
    // SAFETY: the out pointer is valid, and a created buffer is returned
    // retained, which `CFRetained::from_raw` takes over.
    unsafe {
        let status =
            CVPixelBufferCreateWithIOSurface(None, &io_surface, None, NonNull::from(&mut buffer));
        (status == kCVReturnSuccess)
            .then(|| NonNull::new(buffer))
            .flatten()
            .map(|buffer| CFRetained::from_raw(buffer))
    }
}

impl Drop for SceneRenderingContext {
    fn drop(&mut self) {
        let device = self.device.get_mut();
        let context = self.context.get_mut();
        let _ = self.swap_chain.destroy(device, context);
        for mut surface in self.shown.get_mut().drain(..) {
            let _ = device.destroy_surface(context, &mut surface);
        }
        let _ = device.destroy_context(context);
    }
}

impl RenderingContext for SceneRenderingContext {
    fn prepare_for_rendering(&self) {
        self.gleam
            .bind_framebuffer(gl::FRAMEBUFFER, self.framebuffer());
    }

    /// As Servo's own contexts read: the back buffer, flipped to rows top
    /// first.
    fn read_to_image(&self, rect: DeviceIntRect) -> Option<RgbaImage> {
        let gl = &self.gleam;
        gl.bind_framebuffer(gl::FRAMEBUFFER, self.framebuffer());
        gl.bind_vertex_array(0);
        let pixels = gl.read_pixels(
            rect.min.x,
            rect.min.y,
            rect.width(),
            rect.height(),
            gl::RGBA,
            gl::UNSIGNED_BYTE,
        );
        let stride = rect.width() as usize * 4;
        let flipped = pixels
            .chunks_exact(stride)
            .rev()
            .flatten()
            .copied()
            .collect();
        RgbaImage::from_raw(rect.width() as u32, rect.height() as u32, flipped)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        if self.size.get() == size || size.width == 0 || size.height == 0 {
            return;
        }
        self.size.set(size);
        let device = self.device.borrow();
        let mut context = self.context.borrow_mut();
        let size = Size2D::new(size.width as i32, size.height as i32);
        if let Err(error) = self.swap_chain.resize(&device, &mut context, size) {
            log::error!("resizing the page's frames: {error:?}");
        }
    }

    /// Swaps, keeps the new front buffer as the frame GPUI draws, and gives
    /// the oldest shown buffer back to Servo, or destroys it if the page has
    /// since been resized.
    fn present(&self) {
        let device = self.device.borrow();
        let mut context = self.context.borrow_mut();
        if let Err(error) = self
            .swap_chain
            .swap_buffers(&device, &mut context, PreserveBuffer::No)
        {
            log::error!("presenting the page: {error:?}");
            return;
        }
        let Some(surface) = self.swap_chain.take_pending_surface() else {
            return;
        };
        *self.frame.borrow_mut() = pixel_buffer(&device, &surface);
        let mut shown = self.shown.borrow_mut();
        shown.push_back(surface);
        while shown.len() > FRAMES_IN_FLIGHT {
            let Some(mut oldest) = shown.pop_front() else {
                break;
            };
            if device.surface_info(&oldest).size == self.swap_chain.size() {
                self.swap_chain.recycle_surface(oldest);
            } else {
                let _ = device.destroy_surface(&mut context, &mut oldest);
            }
        }
    }

    fn make_current(&self) -> Result<(), Error> {
        self.device
            .borrow()
            .make_context_current(&self.context.borrow())
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.gleam.clone()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.glow.clone()
    }

    fn create_texture(&self, surface: Surface) -> Option<(SurfaceTexture, u32, Size2D<i32>)> {
        let device = self.device.borrow();
        let mut context = self.context.borrow_mut();
        let size = device.surface_info(&surface).size;
        let texture = device
            .create_surface_texture(&mut context, surface)
            .map_err(|(error, _)| log::error!("WebGL surface texture: {error:?}"))
            .ok()?;
        let object = device
            .surface_texture_object(&texture)
            .map_or(0, |texture| texture.0.get());
        Some((texture, object, size))
    }

    fn destroy_texture(&self, texture: SurfaceTexture) -> Option<Surface> {
        let device = self.device.borrow();
        let mut context = self.context.borrow_mut();
        device
            .destroy_surface_texture(&mut context, texture)
            .map_err(|(error, _)| error)
            .ok()
    }

    fn connection(&self) -> Option<Connection> {
        Some(self.device.borrow().connection())
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_video::{CVPixelBufferGetPixelFormatType, kCVPixelFormatType_32BGRA};

    use super::*;

    /// What GPUI's BGRA path expects (`vendor/gpui-pre-apple`): BGRA bytes,
    /// rows in OpenGL's order, the bottom of the picture first.
    #[test]
    fn a_presented_frame_is_a_bgra_buffer_bottom_row_first() {
        let context = SceneRenderingContext::new(PhysicalSize::new(8, 8)).unwrap();
        context.make_current().unwrap();
        context.prepare_for_rendering();
        let gl = context.gleam_gl_api();
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.enable(gl::SCISSOR_TEST);
        gl.scissor(0, 4, 8, 4);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.disable(gl::SCISSOR_TEST);
        gl.finish();

        let read = context
            .read_to_image(DeviceIntRect::from_size((8, 8).into()))
            .unwrap();
        assert_eq!(read.get_pixel(0, 0).0, [255, 0, 0, 255], "read top first");

        context.present();
        let frame = context.frame().expect("a frame");
        assert_eq!(
            CVPixelBufferGetPixelFormatType(&frame),
            kCVPixelFormatType_32BGRA
        );
        assert_eq!(
            (
                CVPixelBufferGetWidth(&frame),
                CVPixelBufferGetHeight(&frame)
            ),
            (8, 8)
        );
        // SAFETY: the buffer is locked while its memory is read.
        let (first, last) = unsafe {
            let flags = CVPixelBufferLockFlags::ReadOnly;
            assert_eq!(
                CVPixelBufferLockBaseAddress(&frame, flags),
                kCVReturnSuccess
            );
            let base = CVPixelBufferGetBaseAddress(&frame).cast::<[u8; 4]>();
            let stride = CVPixelBufferGetBytesPerRow(&frame);
            let first = *base;
            let last = *base.byte_add(7 * stride);
            CVPixelBufferUnlockBaseAddress(&frame, flags);
            (first, last)
        };
        assert_eq!(first, [255, 0, 0, 255], "bottom row, blue, first");
        assert_eq!(last, [0, 0, 255, 255], "top row, red, last");

        // Frames keep coming, and old buffers go back to Servo.
        for _ in 0..FRAMES_IN_FLIGHT + 2 {
            context.present();
        }
        assert_eq!(context.shown.borrow().len(), FRAMES_IN_FLIGHT);
    }

    /// A header's logo does not color the toolbar; the header's own color does.
    #[test]
    fn the_top_color_is_what_most_of_the_top_row_shows() {
        let context = SceneRenderingContext::new(PhysicalSize::new(64, 8)).unwrap();
        assert_eq!(context.top_color(), None, "nothing presented yet");
        context.make_current().unwrap();
        context.prepare_for_rendering();
        let gl = context.gleam_gl_api();
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.enable(gl::SCISSOR_TEST);
        gl.scissor(0, 4, 64, 4);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.scissor(0, 7, 12, 1);
        gl.clear_color(0.0, 1.0, 0.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.disable(gl::SCISSOR_TEST);
        gl.finish();
        context.present();

        assert_eq!(context.top_color(), Some([255, 0, 0]));
    }

    /// The palette reads a real frame: BGRA bytes, a row's stride, either
    /// half of the picture.
    #[test]
    fn a_presented_frame_hands_its_pixels_over_for_a_palette() {
        let context = SceneRenderingContext::new(PhysicalSize::new(64, 8)).unwrap();
        assert!(
            context.with_pixels(|_, _, _, _| ()).is_none(),
            "nothing presented yet"
        );
        context.make_current().unwrap();
        context.prepare_for_rendering();
        let gl = context.gleam_gl_api();
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.enable(gl::SCISSOR_TEST);
        gl.scissor(0, 4, 64, 4);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.disable(gl::SCISSOR_TEST);
        gl.finish();
        context.present();

        let (width, height, stride, swatches) = context
            .with_pixels(|pixels, width, height, stride| {
                (
                    width,
                    height,
                    stride,
                    crate::site::palette(pixels, width, height, stride),
                )
            })
            .expect("a frame");
        assert_eq!((width, height), (64, 8));
        assert!(stride >= 64 * 4);
        let colors: Vec<_> = swatches.iter().map(|swatch| swatch.color).collect();
        assert_eq!(swatches.len(), 2, "{colors:?}");
        // Lab and back moves a channel by a step at most.
        let has = |want: [u8; 3]| {
            colors
                .iter()
                .any(|color| color.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 2))
        };
        assert!(has([255, 0, 0]) && has([0, 0, 255]), "{colors:?}");
        assert!(
            swatches
                .iter()
                .all(|swatch| (swatch.share - 0.5).abs() < 0.05)
        );
    }
}
