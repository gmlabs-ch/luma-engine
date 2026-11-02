//! Stand-ins for the 3D side when the `3d` feature is off: the world and
//! the scene keep their fields and a saved game its layout, with nothing
//! in them, so that a 2D-only build reads the saved games of a 3D build
//! that hold no 3D content, and refuses the others with a clear message.

use luma_script::{Context, NativeId, Return, Value};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::input::HitArea;
use crate::world::World;

const MESSAGE: &str =
    "the saved game holds 3D content; this build of Luma Engine was made without the `3d` feature";

/// The 3D actor, which this build cannot hold.
pub mod actor3d {
    /// Uninhabited: no 3D actor exists without the `3d` feature.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Actor3d {}
}

/// The GPU side of the 3D actors, empty in this build.
pub mod draw3d {
    /// Nothing to keep without the `3d` feature.
    #[derive(Debug, Default)]
    pub struct Render3d;
}

/// The 3D side of a scene, which this build cannot hold.
pub mod scene3d {
    /// Uninhabited: no scene geometry exists without the `3d` feature.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Scene3d {}
}

impl Serialize for actor3d::Actor3d {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

impl<'de> Deserialize<'de> for actor3d::Actor3d {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(D::Error::custom(MESSAGE))
    }
}

impl Serialize for scene3d::Scene3d {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

impl<'de> Deserialize<'de> for scene3d::Scene3d {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(D::Error::custom(MESSAGE))
    }
}

impl World {
    pub(crate) fn update_actors3d(&mut self) {}

    pub(crate) fn upload_3d(&mut self, _: &luma_render::Renderer) {
        let draw3d::Render3d = self.render3d;
    }

    pub(crate) fn three_d_overlays<'a>(&'a self, _: &mut crate::scene_view::Overlays<'a>) {}

    pub(crate) fn reload_3d(&mut self) {}

    pub(crate) fn main_object_is_3d(&self) -> bool {
        false
    }

    pub(crate) fn scene_bound_actors3d(&self) -> Vec<NativeId> {
        Vec::new()
    }

    pub(crate) fn actor3d_hits(&self, _: NativeId, _: (i32, i32), _: &mut Vec<HitArea>) {}

    pub(crate) fn actor3d_hit_at(&self, _: NativeId, _: i32, _: i32) -> bool {
        false
    }

    /// The 3D properties of the scene answer nothing without the feature;
    /// a 2D game never reads them.
    pub(crate) fn scene3d_get(&self, _: &str) -> Option<Value> {
        None
    }

    pub(crate) fn scene3d_set(&mut self, _: &str, _: &Value) -> bool {
        false
    }

    pub(crate) fn scene3d_call(&mut self, _: &str, _: &[Value], _: Context<'_>) -> Option<Return> {
        None
    }
}
