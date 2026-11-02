//! The scene camera and the mapping between scene pixels and the 3D world.
//!
//! One perspective camera ties the painted background to the geometry:
//! its image plane is the scene's **main layer**, the optical axis passes
//! through the layer's centre, and the vertical field of view spans the
//! layer's height (or more, when the layer is wider than tall in
//! proportion to the screen). The scroll offset is folded into the
//! projection so that a 3D point always lands on the same **scene** pixel,
//! whatever the scroll. The formulas are in `docs/3d-actors.md`, section 4.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x sources
//! (`BRenderD3D.cpp`: `SetProjection`, `GetProjectionParams`, `Setup3D`;
//! `AdSceneGeometry.cpp`: `Convert2Dto3D`, `Convert3Dto2D`,
//! `StoreDrawingParams`; `AdGame.cpp`: `GetLayerSize`; Copyright (c)
//! 2003-2013 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::math::{self, Mat4, Vec3};
use crate::scene_geometry::Camera;

pub use crate::math::Viewport;

/// The near clipping distance when the scene gives none, world units.
pub const DEFAULT_NEAR_PLANE: f32 = 90.0;

/// The far clipping distance when the scene gives none, world units.
pub const DEFAULT_FAR_PLANE: f32 = 10000.0;

/// What a scene changes about its camera: the `FOV_OVERRIDE`,
/// `NEAR_CLIPPING_PLANE` and `FAR_CLIPPING_PLANE` keys. Each is `None`
/// (or negative, which counts as `None`) to keep the camera's own field of
/// view or the default clipping planes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraSettings {
    /// Vertical field of view in **radians**, replacing the camera's.
    pub fov_override: Option<f32>,
    /// Near clipping distance.
    pub near_plane: Option<f32>,
    /// Far clipping distance.
    pub far_plane: Option<f32>,
}

/// The 2D side of the projection: the game's resolution, the main layer's
/// size, the viewport the scene is drawn in and the current scroll offset,
/// all in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewSetup {
    /// The game's resolution `(width, height)`; the runtime ignores the
    /// scene's editor resolution keys.
    pub resolution: (i32, i32),
    /// The size of the scene's main layer `(width, height)`. With
    /// `SCROLL_3D_COMPATIBILITY` this is the resolution instead, so that
    /// the camera sees only the top-left screenful of the scene.
    pub layer: (i32, i32),
    /// The part of the screen the scene is drawn in: the whole screen
    /// unless the scene or the game sets a scene viewport.
    pub viewport: Viewport,
    /// The scene's scroll offset `(left, top)`: what is subtracted from a
    /// scene coordinate to get a screen coordinate of the main layer.
    pub scroll: (i32, i32),
}

impl ViewSetup {
    /// A scene drawn over the whole screen, not scrolled.
    pub fn full_screen(resolution: (i32, i32), layer: (i32, i32)) -> ViewSetup {
        ViewSetup {
            resolution,
            layer,
            viewport: Viewport::new(0, 0, resolution.0, resolution.1),
            scroll: (0, 0),
        }
    }

    /// The setup of a scene: its main layer, or the resolution when the
    /// scene asks for the pre-1.7 scrolling (`SCROLL_3D_COMPATIBILITY`),
    /// the scene viewport when there is one, and the scroll offset.
    pub fn for_scene(
        resolution: (i32, i32),
        main_layer: (i32, i32),
        scroll_3d_compatibility: bool,
        viewport: Option<Viewport>,
        scroll: (i32, i32),
    ) -> ViewSetup {
        ViewSetup {
            resolution,
            layer: if scroll_3d_compatibility {
                resolution
            } else {
                main_layer
            },
            viewport: viewport.unwrap_or(Viewport::new(0, 0, resolution.0, resolution.1)),
            scroll,
        }
    }
}

/// A ray in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where it starts (the camera position for a pick ray).
    pub origin: Vec3,
    /// Its direction, not normalised.
    pub direction: Vec3,
}

/// The matrices of a scene camera for a given 2D setup, and the
/// conversions between scene pixels and world points.
///
/// The projection is `PerspectiveFovLH(fov, VW/VH, near, far)` with
/// `_11` and `_22` scaled by `S = max(LW/RW, LH/RH) * RH/VH` and the
/// off-centre terms `_31 = -(OffX + (mleft - mright)/2 - ModW) / VW * 2`,
/// `_32 = (OffY + (mtop - mbottom)/2 - ModH) / VH * 2`, where `R` is the
/// resolution, `L` the layer, `V` the viewport, `m*` its margins,
/// `Off = scroll - viewport origin` and `Mod = max(0, (L - R) / 2)` (with
/// a layer shorter than the screen treated as the screen's height, see
/// [`Projection::new`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    view: Mat4,
    inverse_view: Mat4,
    projection: Mat4,
    setup: ViewSetup,
    fov: f32,
    near: f32,
    far: f32,
    mod_width: f32,
    mod_height: f32,
}

impl Projection {
    /// The projection of a camera for a 2D setup.
    pub fn new(camera: &Camera, settings: &CameraSettings, setup: ViewSetup) -> Projection {
        let fov = settings
            .fov_override
            .filter(|f| *f >= 0.0)
            .unwrap_or(camera.fov);
        let near = settings
            .near_plane
            .filter(|n| *n >= 0.0)
            .unwrap_or(DEFAULT_NEAR_PLANE);
        let far = settings
            .far_plane
            .filter(|f| *f >= 0.0)
            .unwrap_or(DEFAULT_FAR_PLANE);

        let (res_width, res_height) = (setup.resolution.0 as f32, setup.resolution.1 as f32);
        let (layer_width, mut layer_height) = (setup.layer.0 as f32, setup.layer.1 as f32);
        let mut mod_width = 0.0;
        let mut mod_height = 0.0;
        if layer_width > res_width {
            mod_width = (layer_width - res_width) / 2.0;
        }
        if layer_height > res_height {
            mod_height = (layer_height - res_height) / 2.0;
        }
        // A layer shorter than the screen is not scrolled: the camera sees
        // the whole screen height (new in 1.7.2.1; no equivalent for the
        // width).
        if layer_height < res_height {
            mod_height -= (res_height - layer_height) / 2.0;
            layer_height = res_height;
        }

        let vp = setup.viewport;
        let (vp_width, vp_height) = (vp.width as f32, vp.height as f32);
        let (mleft, mright) = (vp.x, setup.resolution.0 - vp.width - vp.x);
        let (mtop, mbottom) = (vp.y, setup.resolution.1 - vp.height - vp.y);

        let mut projection = Mat4::perspective_fov_lh(fov, vp_width / vp_height, near, far);
        let scale_mod = res_height / vp_height;
        let scale_ratio = (layer_width / res_width).max(layer_height / res_height);
        let offset_x = (setup.scroll.0 - vp.x) as f32;
        let offset_y = (setup.scroll.1 - vp.y) as f32;
        projection.m[0][0] *= scale_ratio * scale_mod;
        projection.m[1][1] *= scale_ratio * scale_mod;
        projection.m[2][0] =
            -(offset_x + ((mleft - mright) / 2) as f32 - mod_width) / vp_width * 2.0;
        projection.m[2][1] =
            (offset_y + ((mtop - mbottom) / 2) as f32 - mod_height) / vp_height * 2.0;

        let view = camera.view_matrix();
        Projection {
            view,
            inverse_view: view.inverse().unwrap_or_default(),
            projection,
            setup,
            fov,
            near,
            far,
            mod_width,
            mod_height,
        }
    }

    /// The view matrix of the camera.
    pub fn view(&self) -> &Mat4 {
        &self.view
    }

    /// The projection matrix, with the layer scale and the scroll folded
    /// in. With the view, it is what the GPU draws with.
    pub fn projection(&self) -> &Mat4 {
        &self.projection
    }

    /// `view * projection`.
    pub fn view_projection(&self) -> Mat4 {
        self.view * self.projection
    }

    /// The 2D setup this projection was built for.
    pub fn setup(&self) -> &ViewSetup {
        &self.setup
    }

    /// The field of view in use, radians.
    pub fn fov(&self) -> f32 {
        self.fov
    }

    /// The near and far clipping distances in use.
    pub fn clip_planes(&self) -> (f32, f32) {
        (self.near, self.far)
    }

    /// A world point to **scene** pixels and depth (`Convert3Dto2D`
    /// before its truncation): the viewport projection of the point plus
    /// the scroll offset less the viewport origin.
    pub fn project_f32(&self, world: Vec3) -> Vec3 {
        let vp = self.setup.viewport;
        let p = math::project(world, vp, &self.projection, &self.view, &Mat4::IDENTITY);
        Vec3::new(
            p.x + (self.setup.scroll.0 - vp.x) as f32,
            p.y + (self.setup.scroll.1 - vp.y) as f32,
            p.z,
        )
    }

    /// A world point to scene pixels (`Convert3Dto2D`), truncated toward
    /// zero as the original's `int` conversion does. An object's `X`/`Y`
    /// are the projection of its origin.
    pub fn project(&self, world: Vec3) -> (i32, i32) {
        let p = self.project_f32(world);
        (p.x as i32, p.y as i32)
    }

    /// The ray through a scene pixel (the first half of `Convert2Dto3D`):
    /// the pixel is moved by `-(mleft + mright)/2 - ModW` horizontally and
    /// the same vertically (integer margins, truncated back to a pixel),
    /// the camera-space direction is `((2X/VW - 1)/P11, -(2Y/VH - 1)/P22,
    /// 1)`, rotated into the world by the inverse view matrix, from the
    /// camera position.
    pub fn pick_ray(&self, x: i32, y: i32) -> Ray {
        let vp = self.setup.viewport;
        let (mleft, mright) = (vp.x, self.setup.resolution.0 - vp.width - vp.x);
        let (mtop, mbottom) = (vp.y, self.setup.resolution.1 - vp.height - vp.y);
        let x = (x as f32 - (((mleft + mright) / 2) as f32 + self.mod_width)) as i32;
        let y = (y as f32 - (((mtop + mbottom) / 2) as f32 + self.mod_height)) as i32;
        let camera_space = Vec3::new(
            ((2.0 * x as f32) / vp.width as f32 - 1.0) / self.projection.m[0][0],
            -((2.0 * y as f32) / vp.height as f32 - 1.0) / self.projection.m[1][1],
            1.0,
        );
        Ray {
            origin: self.inverse_view.translation_part(),
            direction: self.inverse_view.transform_normal(camera_space),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::deg_to_rad;
    use crate::node_patterns::NodePatterns;
    use crate::scene_geometry::SceneGeometry;
    use crate::three_ds::Builder;

    /// A camera on the -Z side looking at the origin along +Z, with a
    /// 90 degree vertical field of view: camera space equals world space
    /// shifted by 1000 in z, and tan(fov/2) = 1.
    fn camera() -> Camera {
        Camera {
            name: "Camera01".into(),
            position: Vec3::new(0.0, 0.0, -1000.0),
            target: Vec3::ZERO,
            bank: 0.0,
            fov: deg_to_rad(90.0),
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3 * (1.0 + a.abs().max(b.abs()))
    }

    #[test]
    fn the_layer_centre_is_the_optical_axis_and_the_height_spans_the_fov() {
        let p = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        assert_eq!(p.project(Vec3::ZERO), (400, 300));
        // px = LW/2 + (RH/2) * S * x / (z tan(fov/2)) = 400 + 300 * 100/1000.
        assert_eq!(p.project(Vec3::new(100.0, 0.0, 0.0)), (430, 300));
        // py = LH/2 - (RH/2) * y / z.
        assert_eq!(p.project(Vec3::new(0.0, 100.0, 0.0)), (400, 270));
        // The top of the image is y = z tan(fov/2) above the axis.
        let top = p.project_f32(Vec3::new(0.0, 1000.0, 0.0));
        assert!(close(top.y, 0.0));
        // Twice as far: half the offset.
        assert_eq!(p.project(Vec3::new(100.0, 0.0, 1000.0)), (415, 300));
        // Depth: the near plane (90) gives 0, points beyond it are positive.
        assert!(close(p.project_f32(Vec3::new(0.0, 0.0, -910.0)).z, 0.0));
        assert_eq!(p.fov(), deg_to_rad(90.0));
        assert_eq!(p.clip_planes(), (DEFAULT_NEAR_PLANE, DEFAULT_FAR_PLANE));
    }

    #[test]
    fn a_wider_layer_scales_and_recentres_and_the_scroll_cancels() {
        let setup = ViewSetup::full_screen((800, 600), (1600, 600));
        let p = Projection::new(&camera(), &CameraSettings::default(), setup);
        // S = max(2, 1) = 2, ModW = 400: the axis is at the layer centre.
        assert_eq!(p.project(Vec3::ZERO), (800, 300));
        assert_eq!(p.project(Vec3::new(100.0, 0.0, 0.0)), (860, 300));
        assert_eq!(p.project(Vec3::new(0.0, 100.0, 0.0)), (800, 240));
        // The same with a scroll: the scene position does not move, but
        // the projection matrix's off-centre term does.
        let scrolled = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup {
                scroll: (300, 0),
                ..setup
            },
        );
        assert_eq!(scrolled.project(Vec3::new(100.0, 0.0, 0.0)), (860, 300));
        assert!(close(
            scrolled.projection().m[2][0],
            -(300.0 - 400.0) / 800.0 * 2.0
        ));
        assert!(close(p.projection().m[2][0], 400.0 / 800.0 * 2.0));
        // The GPU sees screen space: with the scroll, a point at scene
        // x = 860 is drawn at screen x = 560.
        let clip = scrolled
            .view_projection()
            .transform_coord(Vec3::new(100.0, 0.0, 0.0));
        assert!(close((1.0 + clip.x) * 400.0, 560.0));
    }

    #[test]
    fn a_layer_shorter_than_the_screen_is_treated_as_the_screen_height() {
        let p = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 400)),
        );
        // ModH = -(600 - 400)/2 = -100, LH := 600, S = 1: the axis passes
        // through the centre of the short layer, the scale is the screen's.
        let origin = p.project_f32(Vec3::ZERO);
        assert!(
            close(origin.x, 400.0) && close(origin.y, 200.0),
            "{origin:?}"
        );
        // The exact 200 lands on 199.99998 in f32 and truncates to 199: the
        // original's int conversion has the same edge.
        assert!(matches!(p.project(Vec3::ZERO), (400, 199..=200)));
        assert_eq!(p.project(Vec3::new(0.0, 100.0, 0.0)), (400, 170));
    }

    #[test]
    fn a_scene_viewport_does_not_move_scene_positions() {
        let setup = ViewSetup {
            resolution: (800, 600),
            layer: (800, 600),
            viewport: Viewport::new(100, 50, 600, 500),
            scroll: (0, 0),
        };
        let p = Projection::new(&camera(), &CameraSettings::default(), setup);
        // S = 1 * 600/500 = 1.2 and VH = 500: S * VH / 2 = 300 as before.
        assert_eq!(p.project(Vec3::ZERO), (400, 300));
        assert_eq!(p.project(Vec3::new(100.0, 0.0, 0.0)), (430, 300));
        assert_eq!(p.project(Vec3::new(0.0, 100.0, 0.0)), (400, 270));
        // On screen the origin is drawn where a 2D object at scene
        // (400, 300) would be: viewport origin + scene position - scroll.
        let clip = p.view_projection().transform_coord(Vec3::ZERO);
        assert!(close(100.0 + (1.0 + clip.x) * 300.0, 500.0));
        assert!(close(50.0 + (1.0 - clip.y) * 250.0, 350.0));
        // And the pick ray through the same pixel comes back.
        let ray = p.pick_ray(430, 270);
        assert!(close(ray.direction.x / ray.direction.z, 0.1));
        assert!(close(ray.direction.y / ray.direction.z, 0.1));
    }

    #[test]
    fn the_pick_ray_inverts_the_projection() {
        let p = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        let ray = p.pick_ray(400, 300);
        assert_eq!(ray.origin, Vec3::new(0.0, 0.0, -1000.0));
        assert!(close(ray.direction.x, 0.0) && close(ray.direction.y, 0.0));
        assert!(ray.direction.z > 0.0);
        let ray = p.pick_ray(430, 270);
        assert!(close(ray.direction.x / ray.direction.z, 0.1));
        assert!(close(ray.direction.y / ray.direction.z, 0.1));
        // A wider layer: the pixel shift includes ModW.
        let wide = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (1600, 600)),
        );
        let ray = wide.pick_ray(860, 300);
        assert!(close(ray.direction.x / ray.direction.z, 0.1));
        assert!(close(ray.direction.y, 0.0));
    }

    #[test]
    fn settings_override_the_camera() {
        let settings = CameraSettings {
            fov_override: Some(deg_to_rad(60.0)),
            near_plane: Some(10.0),
            far_plane: Some(-1.0),
        };
        let p = Projection::new(
            &camera(),
            &settings,
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        assert_eq!(p.fov(), deg_to_rad(60.0));
        assert_eq!(p.clip_planes(), (10.0, DEFAULT_FAR_PLANE));
        // A narrower field of view magnifies: 300 * cot(30 deg) * 0.1.
        let x = p.project_f32(Vec3::new(100.0, 0.0, 0.0)).x;
        assert!(close(x, 400.0 + 300.0 / deg_to_rad(30.0).tan() * 0.1));
        let negative = CameraSettings {
            fov_override: Some(-1.0),
            ..CameraSettings::default()
        };
        let p = Projection::new(
            &camera(),
            &negative,
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        assert_eq!(p.fov(), deg_to_rad(90.0));
    }

    #[test]
    fn scroll_3d_compatibility_uses_the_resolution_as_the_layer() {
        let setup = ViewSetup::for_scene((800, 600), (1600, 600), true, None, (200, 0));
        assert_eq!(setup.layer, (800, 600));
        assert_eq!(setup.viewport, Viewport::new(0, 0, 800, 600));
        let setup = ViewSetup::for_scene(
            (800, 600),
            (1600, 600),
            false,
            Some(Viewport::new(1, 2, 3, 4)),
            (0, 0),
        );
        assert_eq!(setup.layer, (1600, 600));
        assert_eq!(setup.viewport, Viewport::new(1, 2, 3, 4));
    }

    #[test]
    fn truncation_follows_the_int_conversion() {
        let p = Projection::new(
            &camera(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        // 400 + 300 * 0.0333 = 409.99: truncated, not rounded.
        let f = p.project_f32(Vec3::new(33.3, 0.0, 0.0));
        assert!(f.x > 409.9 && f.x < 410.0);
        assert_eq!(p.project(Vec3::new(33.3, 0.0, 0.0)).0, 409);
    }

    /// A floor at height 0 under the camera's view.
    fn floor_scene() -> (SceneGeometry, Projection) {
        let floor = Builder::mesh(
            "walk_01",
            &[
                Vec3::new(-500.0, 0.0, -500.0),
                Vec3::new(-500.0, 0.0, 500.0),
                Vec3::new(500.0, 0.0, 500.0),
                Vec3::new(500.0, 0.0, -500.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        );
        let cam = Builder::camera(
            "Camera01",
            Vec3::new(0.0, 300.0, -1000.0),
            Vec3::new(0.0, 0.0, 0.0),
            0.0,
            19.0,
        );
        let g = SceneGeometry::load_3ds(
            &Builder::file(&[floor, cam], &[]),
            &NodePatterns::standard(),
        )
        .unwrap();
        let p = Projection::new(
            g.active_camera().unwrap(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        (g, p)
    }

    #[test]
    fn scene_pixels_round_trip_through_the_walk_plane() {
        let (g, p) = floor_scene();
        for point in [
            Vec3::ZERO,
            Vec3::new(120.0, 0.0, 200.0),
            Vec3::new(-300.0, 0.0, -100.0),
        ] {
            let (x, y) = p.project(point);
            let back = g.convert_2d_to_3d(&p, x, y).unwrap();
            // Truncation to a pixel moves the hit by a few units (the far
            // floor is seen at a grazing angle, so a pixel is several units).
            assert!(
                (back - point).length() < 10.0,
                "{point:?} -> ({x}, {y}) -> {back:?}"
            );
            assert!(back.y.abs() < 1e-2);
        }
        // Above the horizon there is no floor.
        assert!(g.convert_2d_to_3d(&p, 400, 0).is_none());
    }

    #[test]
    fn the_tolerant_conversion_scans_toward_the_floor() {
        let (g, p) = floor_scene();
        // The far edge of the floor (z = 500) projects to a scene row; a
        // pixel somewhat above it misses, and the scan finds the floor
        // below (the vertical shift wins ties and is the only axis here).
        let (x, edge_y) = p.project(Vec3::new(0.0, 0.0, 500.0));
        let y = edge_y - 15;
        assert!(g.convert_2d_to_3d(&p, x, y).is_none());
        let hit = g.convert_2d_to_3d_tolerant(&p, x, y).unwrap();
        // The scan lands 5 pixels under the edge row, which at that
        // grazing angle is well inside the floor.
        assert!(hit.z <= 500.0 && hit.z > 200.0, "{hit:?}");
        assert!(hit.x.abs() < 1.0, "{hit:?}");
        // A pixel the scan cannot reach within 1000 pixels: none.
        assert!(g.convert_2d_to_3d_tolerant(&p, 400, -5000).is_none());
        // A pixel that converts directly is returned as is.
        let (x, y) = p.project(Vec3::new(50.0, 0.0, 50.0));
        assert_eq!(
            g.convert_2d_to_3d_tolerant(&p, x, y),
            g.convert_2d_to_3d(&p, x, y)
        );
    }

    #[test]
    fn the_tolerant_conversion_prefers_the_smaller_shift() {
        // A narrow floor strip: from a pixel to its right, the horizontal
        // scan reaches it closer than the vertical one.
        let strip = Builder::mesh(
            "walk_01",
            &[
                Vec3::new(-50.0, 0.0, -500.0),
                Vec3::new(-50.0, 0.0, 500.0),
                Vec3::new(50.0, 0.0, 500.0),
                Vec3::new(50.0, 0.0, -500.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        );
        let cam = Builder::camera(
            "Camera01",
            Vec3::new(0.0, 300.0, -1000.0),
            Vec3::ZERO,
            0.0,
            19.0,
        );
        let g = SceneGeometry::load_3ds(
            &Builder::file(&[strip, cam], &[]),
            &NodePatterns::standard(),
        )
        .unwrap();
        let p = Projection::new(
            g.active_camera().unwrap(),
            &CameraSettings::default(),
            ViewSetup::full_screen((800, 600), (800, 600)),
        );
        let (x, y) = p.project(Vec3::new(80.0, 0.0, 0.0));
        assert!(g.convert_2d_to_3d(&p, x, y).is_none());
        let hit = g.convert_2d_to_3d_tolerant(&p, x, y).unwrap();
        assert!(hit.x <= 50.0 && hit.x > 20.0, "{hit:?}");
        assert!(hit.z.abs() < 20.0, "{hit:?}");
    }
}
