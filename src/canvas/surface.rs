//! The page's raster scale is independent of its current screen transform.
//! Servo receives the CSS viewport and settled raster scale in one update.
use gpui_kit::{Bounds, Pixels, Point, Size, point, px};
use servo::DevicePoint;

#[derive(Clone, Copy, Debug)]
pub(crate) struct SurfaceTransform {
    pub bounds: Bounds<Pixels>,
    pub zoom: f32,
    pub raster_scale: f32,
}

impl SurfaceTransform {
    pub fn device_point(self, position: Point<Pixels>) -> DevicePoint {
        DevicePoint::new(
            f32::from(position.x - self.bounds.origin.x) / self.zoom * self.raster_scale,
            f32::from(position.y - self.bounds.origin.y) / self.zoom * self.raster_scale,
        )
    }

    pub fn window_point(self, x: f32, y: f32) -> Point<Pixels> {
        point(
            self.bounds.origin.x + px(x / self.raster_scale * self.zoom),
            self.bounds.origin.y + px(y / self.raster_scale * self.zoom),
        )
    }
}

/// Reduce the raster scale uniformly at the device-pixel cap. Clamping the
/// dimensions alone would distort the raster. Servo keeps the CSS viewport exact
/// and rounds the scaled surface dimensions up to whole device pixels.
pub(crate) fn settled_scale(
    world: Size<Pixels>,
    zoom: f32,
    display: f32,
    window: Size<Pixels>,
) -> f32 {
    (zoom * display)
        .min(f32::from(window.width) * display / f32::from(world.width))
        .min(f32::from(window.height) * display / f32::from(world.height))
        .max(0.001)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::size;

    #[test]
    fn input_and_controls_round_trip_while_raster_scale_lags_camera() {
        for zoom in [0.35, 1., 2.] {
            for raster_scale in [0.7, 2., 4.] {
                let t = SurfaceTransform {
                    bounds: Bounds::new(
                        point(px(113.), px(87.)),
                        size(px(1280. * zoom), px(800. * zoom)),
                    ),
                    zoom,
                    raster_scale,
                };
                let shown = t.window_point(347. * raster_scale, 219. * raster_scale);
                let device = t.device_point(shown);
                assert!((device.x / raster_scale - 347.).abs() < 0.001);
                assert!((device.y / raster_scale - 219.).abs() < 0.001);
            }
        }
    }

    #[test]
    fn cap_preserves_css_aspect_and_never_exceeds_window_pixels() {
        let world = size(px(1280.), px(800.));
        let window = size(px(1440.), px(900.));
        assert_eq!(settled_scale(world, 0.35, 2., window), 0.7);
        assert_eq!(settled_scale(world, 2., 2., window), 2.25);
    }

    #[test]
    fn cap_reduces_both_axes_when_only_one_dimension_reaches_the_limit() {
        let world = size(px(1280.), px(800.));
        let window = size(px(900.), px(650.));
        let scale = settled_scale(world, 2., 2., window);
        assert_eq!(scale, 1.40625);
        assert_eq!((f32::from(world.width) * scale).ceil(), 1800.);
        assert_eq!((f32::from(world.height) * scale).ceil(), 1125.);
        assert!(f32::from(world.height) * scale < f32::from(window.height) * 2.);
    }
}
