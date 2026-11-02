//! The 3D side of a scene (`CAdScene` with a `CAdSceneGeometry`): the
//! geometry loaded with the scene's `GEOMETRY` key, the active camera's
//! projection for the frame, the path finder the 3D actors share, the
//! fog, and the scripting of the lights, nodes and cameras. See
//! `docs/3d-actors.md`, sections 3, 4, 9 and 11.4.
//!
//! Written with reference to `AdScene.cpp` (`LoadBuffer`, `InitLoop`,
//! `ScCallMethod`, `ScGetProperty`, `ScSetProperty`, `Persist`) and
//! `AdSceneGeometry.cpp` (`EnableLights`, `GetLightColor`, `GetLightPos`,
//! `Persist`) of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_3d::math::Viewport;
use luma_3d::{
    CameraSettings, GEOMETRY_EXTENSION, Light, NODE_PATTERNS_EXTENSION, NodePatterns, PathFinder,
    Projection, SceneGeometry, Vec3, ViewSetup,
};
use luma_archive::Vfs;
use luma_formats::{Rgb, Scene};
use luma_script::{Context, Properties, Return, Value, rgba};
use serde::{Deserialize, Serialize};

use crate::world::World;

/// How many lights light one object: the figure of a Direct3D device
/// (`GetMaxActiveLights`, `Game.MaxActiveLights`); Luma's renderer takes
/// the same.
pub const MAX_LIGHTS: usize = 8;

/// The linear fog of a scene (`Scene.EnableFog`): colour, and the view
/// depths it starts and ends at, in world units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fog {
    /// `0xAARRGGBB` as the script gave it.
    pub color: i32,
    /// Where the fog starts.
    pub start: f32,
    /// Where it is complete.
    pub end: f32,
}

/// The 3D side of a loaded scene.
///
/// A saved game keeps the geometry file, the waypoint height, the active
/// camera, the state of each light and node by name and the fog (the
/// original's `CAdSceneGeometry::Persist`); the geometry is read again
/// from its file when the game loads ([`Scene3d::reload`]). The path
/// finder's open request is not kept: an actor waiting for a path asks
/// again (see `docs/saving.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "Saved", from = "Saved")]
pub struct Scene3d {
    /// The `.3ds` file, as the scene names it (`Scene.GeometryFile`).
    pub file: String,
    /// The classified geometry.
    pub geometry: SceneGeometry,
    /// The path finder, run once per frame.
    pub finder: PathFinder,
    /// The fog, when a script enabled it.
    pub fog: Option<Fog>,
    /// The state a saved game carried, applied when the geometry is read
    /// again.
    pending: Option<Saved>,
    /// The projection of the last update, for the drawing step.
    projection: Option<Projection>,
}

/// What a saved game keeps of the 3D side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    file: String,
    waypoint_height: f32,
    active_camera: Option<String>,
    /// Name, active, colour.
    lights: Vec<(String, bool, Rgb)>,
    /// Name, active, over blocks, planes and generics.
    nodes: Vec<(String, bool)>,
    fog: Option<Fog>,
}

impl From<Scene3d> for Saved {
    fn from(scene: Scene3d) -> Saved {
        if let Some(pending) = scene.pending {
            // Never reloaded since the game loaded: keep what was saved.
            return pending;
        }
        let geometry = &scene.geometry;
        Saved {
            file: scene.file,
            waypoint_height: geometry.waypoint_height,
            active_camera: geometry.active_camera().map(|c| c.name.clone()),
            lights: geometry
                .lights
                .iter()
                .map(|l| (l.name.clone(), l.active, l.color))
                .collect(),
            nodes: geometry
                .nodes()
                .map(|n| (n.name.clone(), n.active))
                .collect(),
            fog: scene.fog,
        }
    }
}

impl From<Saved> for Scene3d {
    fn from(saved: Saved) -> Scene3d {
        Scene3d {
            file: saved.file.clone(),
            geometry: SceneGeometry::default(),
            finder: PathFinder::new(),
            fog: saved.fog,
            pending: Some(saved),
            projection: None,
        }
    }
}

impl Scene3d {
    /// Loads the geometry a scene names (`GEOMETRY`), classified by its
    /// `.geometry` sidecar, with the scene's `WAYPOINT_HEIGHT` and
    /// `CAMERA` applied. `None` for a 2D scene; an error for a file that
    /// is not a `.3ds` or cannot be read or parsed (the original drops
    /// the geometry and logs). The warnings returned are logged by the
    /// caller.
    pub fn load(vfs: &Vfs, scene: &Scene) -> Option<Result<(Scene3d, Vec<String>), String>> {
        let file = scene.geometry.as_deref()?;
        Some(Self::load_file(vfs, file).map(|geometry| {
            let mut scene3d = Scene3d {
                file: file.to_string(),
                geometry,
                finder: PathFinder::new(),
                fog: None,
                pending: None,
                projection: None,
            };
            let warnings = scene3d.apply_scene_keys(scene);
            (scene3d, warnings)
        }))
    }

    /// Reads and classifies a geometry file.
    fn load_file(vfs: &Vfs, file: &str) -> Result<SceneGeometry, String> {
        let extension = file.rsplit('.').next().unwrap_or("");
        if !extension.eq_ignore_ascii_case(GEOMETRY_EXTENSION) {
            return Err(format!(
                "Error: no suitable loader found for file '{file}' (only .{GEOMETRY_EXTENSION} geometry is supported)"
            ));
        }
        let bytes = vfs
            .read(file)
            .map_err(|e| format!("Error loading scene geometry '{file}': {e}"))?;
        let sidecar = format!(
            "{}.{NODE_PATTERNS_EXTENSION}",
            &file[..file.len() - extension.len() - 1]
        );
        let patterns = NodePatterns::load(vfs.read(&sidecar).ok().as_deref());
        SceneGeometry::load_3ds(&bytes, &patterns)
            .map_err(|e| format!("Error loading scene geometry '{file}': {e}"))
    }

    /// Applies the scene's `WAYPOINT_HEIGHT` and `CAMERA` keys
    /// (`CAdScene::LoadBuffer` after the geometry); an unknown camera
    /// name is a warning and keeps the first camera.
    fn apply_scene_keys(&mut self, scene: &Scene) -> Vec<String> {
        let mut warnings = Vec::new();
        if let Some(height) = scene.waypoint_height.filter(|h| *h >= 0.0) {
            self.geometry.set_waypoint_height(height);
        }
        if let Some(camera) = scene.camera.as_deref().filter(|c| !c.is_empty()) {
            if !self.geometry.set_active_camera(camera) {
                warnings.push(format!(
                    "Warning: Camera '{camera}' not found in geometry file '{}'",
                    self.file
                ));
            }
        }
        warnings
    }

    /// Reads the geometry again after a saved game and puts the saved
    /// state back on it: waypoint height, active camera, lights and
    /// nodes by name. Names the file no longer has are dropped.
    pub fn reload(&mut self, vfs: &Vfs) -> Result<(), String> {
        let Some(saved) = self.pending.take() else {
            return Ok(());
        };
        self.geometry = Self::load_file(vfs, &saved.file)?;
        self.geometry.set_waypoint_height(saved.waypoint_height);
        if let Some(camera) = &saved.active_camera {
            self.geometry.set_active_camera(camera);
        }
        for (name, active, color) in &saved.lights {
            if let Some(light) = self.geometry.light_mut(name) {
                light.active = *active;
                light.color = *color;
            }
        }
        for (name, active) in &saved.nodes {
            self.geometry.set_node_enabled(name, *active);
        }
        self.fog = saved.fog;
        Ok(())
    }

    /// Whether the geometry still waits to be read again after a load.
    pub fn needs_reload(&self) -> bool {
        self.pending.is_some()
    }

    /// The projection built by the last update, for the drawing step.
    pub fn projection(&self) -> Option<&Projection> {
        self.projection.as_ref()
    }

    /// Keeps the projection of the frame.
    pub(crate) fn set_projection(&mut self, projection: Option<Projection>) {
        self.projection = projection;
    }

    /// The lights that light an object at `position`
    /// (`CAdSceneGeometry::EnableLights`): when more than [`MAX_LIGHTS`]
    /// lights are active, only the nearest to the position count (for a
    /// spot, the distance to the point three quarters of the way to its
    /// target); then, in file order and skipping the `ignored` names
    /// (case-insensitive), the active lights among those, at most
    /// `MAX_LIGHTS`.
    pub fn lights_for<'a>(
        geometry: &'a SceneGeometry,
        position: Vec3,
        ignored: &[String],
    ) -> Vec<&'a Light> {
        let active = geometry.lights.iter().filter(|l| l.active).count();
        let mut available: Vec<bool> = vec![true; geometry.lights.len()];
        if active > MAX_LIGHTS {
            let mut by_distance: Vec<(usize, f32)> = geometry
                .lights
                .iter()
                .enumerate()
                .filter(|(_, l)| l.active)
                .map(|(i, l)| {
                    let reference = if l.spotlight {
                        l.position + (l.target - l.position) * 0.75
                    } else {
                        l.position
                    };
                    (i, (reference - position).length().abs())
                })
                .collect();
            by_distance.sort_by(|a, b| a.1.total_cmp(&b.1));
            available.fill(false);
            for (rank, (index, _)) in by_distance.iter().enumerate() {
                available[*index] = rank < MAX_LIGHTS;
            }
        }
        let mut lights = Vec::new();
        for (index, light) in geometry.lights.iter().enumerate() {
            if lights.len() >= MAX_LIGHTS {
                break;
            }
            if ignored.iter().any(|n| n.eq_ignore_ascii_case(&light.name)) {
                continue;
            }
            if available[index] && light.active {
                lights.push(light);
            }
        }
        lights
    }
}

/// A light's colour as scripts read it (`GetLightColor`): the diffuse
/// colour packed as `0xAARRGGBB` with full alpha.
fn color_int(c: Rgb) -> i32 {
    rgba(i32::from(c.r), i32::from(c.g), i32::from(c.b), 255)
}

/// The RGB of a script colour (`SetLightColor`, `AmbientLightColor`); the
/// alpha is dropped.
fn rgb_of(v: i32) -> Rgb {
    Rgb {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
    }
}

/// An object `{ X, Y, Z }` for a 3D point.
fn point_object(p: Vec3) -> Value {
    let mut props = Properties::new();
    props.insert("X".to_string(), Value::Float(f64::from(p.x)));
    props.insert("Y".to_string(), Value::Float(f64::from(p.y)));
    props.insert("Z".to_string(), Value::Float(f64::from(p.z)));
    Value::Object(props)
}

impl World {
    /// The 3D side of the current scene, when it has geometry.
    pub fn scene3d(&self) -> Option<&Scene3d> {
        self.scene.as_ref()?.three_d.as_ref()
    }

    /// See [`World::scene3d`].
    pub(crate) fn scene3d_mut(&mut self) -> Option<&mut Scene3d> {
        self.scene.as_mut()?.three_d.as_mut()
    }

    /// The projection of the active camera for this frame (section 4.1
    /// of `docs/3d-actors.md`): the game's resolution, the main layer
    /// (the resolution with `SCROLL_3D_COMPATIBILITY`), the scene's own
    /// `VIEWPORT` else the game's scene viewport, the scroll offset, and
    /// the scene's `FOV_OVERRIDE` and clipping planes. `None` without
    /// geometry or without a camera.
    pub fn projection_3d(&self) -> Option<Projection> {
        let view = self.scene.as_ref()?;
        let scene3d = view.three_d.as_ref()?;
        let camera = scene3d.geometry.active_camera()?;
        let settings = CameraSettings {
            fov_override: view.scene.fov_override,
            near_plane: view.scene.near_clipping_plane,
            far_plane: view.scene.far_clipping_plane,
        };
        let resolution = (self.settings.width as i32, self.settings.height as i32);
        let main_layer = view
            .scene
            .main_layer()
            .map_or(resolution, |l| (l.width, l.height));
        let viewport = match &view.scene.viewport {
            Some(r) => Viewport::new(r.left, r.top, r.width() as i32, r.height() as i32),
            None => {
                let (x, y, w, h) = self.viewport();
                Viewport::new(x, y, w, h)
            }
        };
        let setup = ViewSetup::for_scene(
            resolution,
            main_layer,
            view.scene.scroll_3d_compatibility,
            Some(viewport),
            (self.offset_x, self.offset_y),
        );
        Some(Projection::new(camera, &settings, setup))
    }

    /// Runs the open path request of the scene's finder
    /// (`CAdSceneGeometry::InitLoop`), once per frame before the 3D
    /// actors update.
    pub(crate) fn run_path_finder_3d(&mut self) {
        if let Some(scene3d) = self.scene3d_mut() {
            let Scene3d {
                geometry, finder, ..
            } = scene3d;
            finder.run(geometry);
        }
    }

    /// Logs that a 3D scene method was called without geometry
    /// (`Scene.<name>: Scene doesn't contain any geometry.`).
    fn no_geometry(&mut self, ctx: &Context<'_>, name: &str) {
        self.runtime_error(
            ctx,
            &format!("Scene.{name}: Scene doesn't contain any geometry."),
        );
    }

    /// The 3D properties of the scene (`CAdScene::ScGetProperty`).
    pub(crate) fn scene3d_get(&self, name: &str) -> Option<Value> {
        let scene3d = self.scene3d();
        Some(match name {
            "GeometryFile" => scene3d.map_or(Value::Null, |s| Value::from(s.file.clone())),
            "WaypointsHeight" => {
                Value::Float(scene3d.map_or(0.0, |s| f64::from(s.geometry.waypoint_height)))
            }
            "NumLights" => Value::Int(scene3d.map_or(0, |s| s.geometry.lights.len() as i32)),
            "MaxShadowType" => Value::Int(
                self.scene()
                    .map_or(2, |v| i32::from(v.scene.max_shadow_type)),
            ),
            "AmbientLightColor" => Value::Int(
                self.scene()
                    .map_or(0, |v| color_int(v.scene.ambient_light_color)),
            ),
            _ => return None,
        })
    }

    /// Sets a 3D property of the scene (`CAdScene::ScSetProperty`).
    pub(crate) fn scene3d_set(&mut self, name: &str, value: &Value) -> bool {
        match name {
            "WaypointsHeight" => {
                let height = value.get_float(self) as f32;
                if let Some(scene3d) = self.scene3d_mut() {
                    scene3d.geometry.set_waypoint_height(height);
                }
            }
            "MaxShadowType" => {
                let kind = value.get_int(self).clamp(0, 3) as u8;
                if let Some(view) = self.scene_mut() {
                    view.scene.max_shadow_type = kind;
                }
            }
            "AmbientLightColor" => {
                let color = rgb_of(value.get_int(self));
                if let Some(view) = self.scene_mut() {
                    view.scene.ambient_light_color = color;
                }
            }
            _ => return false,
        }
        true
    }

    /// The 3D methods of the scene (`CAdScene::ScCallMethod`): nodes,
    /// lights, the camera and the fog. The methods that change the
    /// geometry are runtime errors without one; the queries answer
    /// `false`.
    pub(crate) fn scene3d_call(
        &mut self,
        name: &str,
        args: &[Value],
        ctx: Context<'_>,
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "EnableNode3D" | "DisableNode3D" => {
                let node = arg(0).get_string(self);
                let enabled = name == "EnableNode3D";
                match self.scene3d_mut() {
                    Some(scene3d) => {
                        Return::value(scene3d.geometry.set_node_enabled(&node, enabled))
                    }
                    None => {
                        self.no_geometry(&ctx, name);
                        Return::value(false)
                    }
                }
            }
            "IsNode3DEnabled" => {
                let node = arg(0).get_string(self);
                Return::value(
                    self.scene3d()
                        .is_some_and(|s| s.geometry.is_node_enabled(&node)),
                )
            }
            "EnableLight" | "DisableLight" => {
                let light = arg(0).get_string(self);
                let enabled = name == "EnableLight";
                match self.scene3d_mut() {
                    Some(scene3d) => {
                        Return::value(scene3d.geometry.set_light_enabled(&light, enabled))
                    }
                    None => {
                        self.no_geometry(&ctx, name);
                        Return::value(false)
                    }
                }
            }
            "IsLightEnabled" => {
                let light = arg(0).get_string(self);
                Return::value(
                    self.scene3d()
                        .is_some_and(|s| s.geometry.is_light_enabled(&light)),
                )
            }
            "GetLightName" => {
                let index = usize::try_from(arg(0).get_int(self)).ok();
                Return::value(
                    self.scene3d()
                        .zip(index)
                        .and_then(|(s, i)| s.geometry.lights.get(i))
                        .map_or(Value::Null, |l| Value::from(l.name.clone())),
                )
            }
            "SetLightColor" => {
                let light = arg(0).get_string(self);
                let color = rgb_of(arg(1).get_int(self));
                match self.scene3d_mut() {
                    Some(scene3d) => Return::value(scene3d.geometry.set_light_color(&light, color)),
                    None => {
                        self.no_geometry(&ctx, name);
                        Return::value(false)
                    }
                }
            }
            "GetLightColor" => {
                let light = arg(0).get_string(self);
                match self.scene3d() {
                    Some(scene3d) => Return::value(
                        scene3d
                            .geometry
                            .light(&light)
                            .map_or(0, |l| color_int(l.color)),
                    ),
                    None => {
                        self.no_geometry(&ctx, name);
                        Return::null()
                    }
                }
            }
            "GetLightPosition" => {
                let light = arg(0).get_string(self);
                match self.scene3d() {
                    Some(scene3d) => Return::value(point_object(
                        scene3d
                            .geometry
                            .light(&light)
                            .map_or(Vec3::ZERO, |l| l.position),
                    )),
                    None => {
                        self.no_geometry(&ctx, name);
                        Return::null()
                    }
                }
            }
            "SetActiveCamera" => {
                let camera = arg(0).get_string(self);
                match self.scene3d_mut() {
                    Some(scene3d) => {
                        if !scene3d.geometry.set_active_camera(&camera) {
                            self.runtime_error(&ctx, "Scene.SetActiveCamera failed.");
                        }
                    }
                    None => self.no_geometry(&ctx, name),
                }
                Return::null()
            }
            "EnableFog" => {
                let fog = Fog {
                    color: arg(0).get_int(self),
                    start: arg(1).get_float(self) as f32,
                    end: arg(2).get_float(self) as f32,
                };
                if let Some(scene3d) = self.scene3d_mut() {
                    scene3d.fog = Some(fog);
                }
                Return::null()
            }
            "DisableFog" => {
                if let Some(scene3d) = self.scene3d_mut() {
                    scene3d.fog = None;
                }
                Return::null()
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_3d::Vec3;
    use luma_3d::three_ds::Builder;

    fn light(name: &str, x: f32) -> Vec<u8> {
        Builder::light(name, Vec3::new(x, 100.0, 0.0), [1.0, 1.0, 1.0], None, false)
    }

    #[test]
    fn the_nearest_lights_count_and_ignored_ones_are_skipped() {
        let objects: Vec<Vec<u8>> = (0..10)
            .map(|i| light(&format!("L{i}"), i as f32 * 100.0))
            .collect();
        let bytes = Builder::file(&objects, &[]);
        let mut geometry = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        assert_eq!(geometry.lights.len(), 10);
        // Ten active lights: the eight nearest to x = 950 are L2..L9.
        let names: Vec<&str> = Scene3d::lights_for(&geometry, Vec3::new(950.0, 0.0, 0.0), &[])
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        assert_eq!(names, ["L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9"]);
        // An ignored light is skipped and not replaced.
        let names: Vec<&str> =
            Scene3d::lights_for(&geometry, Vec3::new(950.0, 0.0, 0.0), &["l5".to_string()])
                .iter()
                .map(|l| l.name.as_str())
                .collect();
        assert_eq!(names, ["L2", "L3", "L4", "L6", "L7", "L8", "L9"]);
        // With eight or fewer active, every active light counts, in order.
        geometry.set_light_enabled("L0", false);
        geometry.set_light_enabled("L9", false);
        let names: Vec<&str> = Scene3d::lights_for(&geometry, Vec3::ZERO, &[])
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        assert_eq!(names, ["L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8"]);
    }

    #[test]
    fn the_saved_state_comes_back_by_name() {
        let bytes = Builder::file(
            &[
                light("Omni01", 0.0),
                light("Omni02", 50.0),
                Builder::mesh(
                    "blk_box",
                    &[
                        Vec3::new(0.0, 0.0, 0.0),
                        Vec3::new(0.0, 0.0, 10.0),
                        Vec3::new(10.0, 0.0, 10.0),
                    ],
                    &[[0, 1, 2]],
                ),
                Builder::camera(
                    "Camera01",
                    Vec3::new(0.0, 0.0, -100.0),
                    Vec3::ZERO,
                    0.0,
                    50.0,
                ),
                Builder::camera(
                    "Camera02",
                    Vec3::new(0.0, 0.0, 100.0),
                    Vec3::ZERO,
                    0.0,
                    50.0,
                ),
            ],
            &[],
        );
        let geometry = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        let mut scene3d = Scene3d {
            file: "scenes\\g.3ds".into(),
            geometry,
            finder: PathFinder::new(),
            fog: Some(Fog {
                color: 0x00ff_ffff,
                start: 10.0,
                end: 500.0,
            }),
            pending: None,
            projection: None,
        };
        scene3d.geometry.set_active_camera("Camera02");
        scene3d.geometry.set_light_enabled("Omni01", false);
        scene3d.geometry.set_light_color(
            "Omni02",
            Rgb {
                r: 10,
                g: 20,
                b: 30,
            },
        );
        scene3d.geometry.set_node_enabled("blk_box", false);
        scene3d.geometry.set_waypoint_height(25.0);

        let saved = Saved::from(scene3d.clone());
        assert_eq!(saved.active_camera.as_deref(), Some("Camera02"));
        assert_eq!(saved.waypoint_height, 25.0);
        let mut back = Scene3d::from(saved.clone());
        assert!(back.needs_reload());
        // Saving again before the reload keeps the saved state.
        assert_eq!(Saved::from(back.clone()), saved);
        // The reload reads the same file: a file system with it.
        let dir = std::env::temp_dir().join(format!("luma-scene3d-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("scenes")).unwrap();
        std::fs::write(dir.join("scenes").join("g.3ds"), &bytes).unwrap();
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir);
        back.reload(&vfs).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!back.needs_reload());
        assert_eq!(back.geometry.active_camera().unwrap().name, "Camera02");
        assert!(!back.geometry.is_light_enabled("Omni01"));
        assert_eq!(back.geometry.light("Omni02").unwrap().color.g, 20);
        assert!(!back.geometry.is_node_enabled("blk_box"));
        assert_eq!(back.geometry.waypoint_height, 25.0);
        assert_eq!(back.fog, scene3d.fog);
    }
}
