//! Regions as scripts see them: the polygons of a scene's layers
//! (`CAdRegion`) and the clickable region of an entity (`CBRegion`),
//! with their point editing, and the scene's waypoint groups
//! (`CAdWaypointGroup`).
//!
//! Written with reference to `BRegion.cpp`, `AdRegion.cpp` and
//! `AdWaypointGroup.cpp` of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_formats::Rgba;
use luma_formats::scene::{Point, Region};
use luma_script::{NativeId, Return, Value, rgba};

use crate::objects::Kind;
use crate::world::World;

/// Which region a handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionRef {
    /// A region node of a layer of the current scene.
    Node {
        /// The layer's index.
        layer: usize,
        /// The node's index in the layer.
        index: usize,
    },
    /// The clickable region of an entity, by the entity's handle.
    Entity(NativeId),
}

fn color_int(c: Rgba) -> i32 {
    rgba(
        i32::from(c.r),
        i32::from(c.g),
        i32::from(c.b),
        i32::from(c.a),
    )
}

fn color_of(v: i32) -> Rgba {
    Rgba {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
        a: ((v >> 24) & 0xff) as u8,
    }
}

/// Edits a polygon (`CBRegion::ScCallMethod`): `AddPoint(x, y)`,
/// `InsertPoint(index, x, y)`, `SetPoint(index, x, y)`,
/// `RemovePoint(index)`. Returns what the call answers, or `None` for
/// another name; an index outside the polygon answers `false`.
fn edit_points(points: &mut Vec<Point>, name: &str, ints: &[i32]) -> Option<bool> {
    let int = |n: usize| ints.get(n).copied().unwrap_or(0);
    let index = |n: usize| usize::try_from(int(n)).ok().filter(|i| *i < points.len());
    Some(match name {
        "AddPoint" => {
            points.push(Point {
                x: int(0),
                y: int(1),
            });
            true
        }
        "InsertPoint" => match index(0) {
            Some(i) => {
                points.insert(
                    i,
                    Point {
                        x: int(1),
                        y: int(2),
                    },
                );
                true
            }
            None => false,
        },
        "SetPoint" => match index(0) {
            Some(i) => {
                points[i] = Point {
                    x: int(1),
                    y: int(2),
                };
                true
            }
            None => false,
        },
        "RemovePoint" => match index(0) {
            Some(i) => {
                points.remove(i);
                true
            }
            None => false,
        },
        _ => return None,
    })
}

impl World {
    /// The region a handle names.
    pub(crate) fn region_ref(&self, r: RegionRef) -> Option<&Region> {
        match r {
            RegionRef::Node { layer, index } => self.scene()?.region(layer, index),
            RegionRef::Entity(owner) => {
                let Kind::Entity(e) = self.native(owner)?.kind else {
                    return None;
                };
                self.entity_def(e)?.region.as_ref()
            }
        }
    }

    /// The region a handle names, mutable.
    pub(crate) fn region_ref_mut(&mut self, r: RegionRef) -> Option<&mut Region> {
        match r {
            RegionRef::Node { layer, index } => self.scene_mut()?.region_mut(layer, index),
            RegionRef::Entity(owner) => {
                let Kind::Entity(e) = self.native(owner)?.kind else {
                    return None;
                };
                self.entity_def_mut(e)?.region.as_mut()
            }
        }
    }

    /// The handle of an entity's region object, created when the entity
    /// has a region and no handle yet (`CAdEntity::m_Region` is a
    /// registered object of its own).
    pub(crate) fn register_entity_region(&mut self, owner: NativeId) -> Option<NativeId> {
        self.region_ref(RegionRef::Entity(owner))?;
        if let Some(id) = self.objects.find(|k| *k == Kind::EntityRegion(owner)) {
            return Some(id);
        }
        Some(self.objects.insert(Kind::EntityRegion(owner)))
    }

    /// The handle of an entity's region object, if it has one.
    pub fn entity_region_id(&self, owner: NativeId) -> Option<NativeId> {
        self.objects.find(|k| *k == Kind::EntityRegion(owner))
    }

    /// Gives an entity a clickable region (`CreateRegion`), empty until
    /// points are added, and returns its handle; an entity with a region
    /// keeps it.
    pub fn create_entity_region(&mut self, owner: NativeId) -> Option<NativeId> {
        let Kind::Entity(e) = self.native(owner)?.kind else {
            return None;
        };
        let entity = self.entity_def_mut(e)?;
        if entity.region.is_none() {
            entity.region = Some(Region::default());
        }
        self.register_entity_region(owner)
    }

    /// Takes an entity's region away (`DeleteRegion`); whether it had one.
    pub fn delete_entity_region(&mut self, owner: NativeId) -> bool {
        let Some(Kind::Entity(e)) = self.native(owner).map(|n| n.kind.clone()) else {
            return false;
        };
        let had = self
            .entity_def_mut(e)
            .is_some_and(|entity| entity.region.take().is_some());
        if let Some(id) = self.entity_region_id(owner) {
            self.objects.remove(id);
        }
        had
    }

    pub(crate) fn region_get(&self, r: RegionRef, name: &str) -> Option<Value> {
        let region = self.region_ref(r)?;
        let scene_region = matches!(r, RegionRef::Node { .. });
        Some(match name {
            "Type" => Value::from(if scene_region { "ad region" } else { "region" }),
            "Name" => Value::from(region.name.clone()),
            "Caption" => Value::from(region.caption.clone()),
            "Active" => Value::Bool(region.active),
            "NumPoints" => Value::Int(region.points.len() as i32),
            "X" => Value::Int(region.bounds().map_or(0, |b| b.left)),
            "Y" => Value::Int(region.bounds().map_or(0, |b| b.top)),
            "Ready" => Value::Bool(true),
            "Blocked" if scene_region => Value::Bool(region.blocked),
            "Decoration" if scene_region => Value::Bool(region.decoration),
            "Scale" if scene_region => Value::Float(f64::from(region.zoom.unwrap_or(0.0))),
            "AlphaColor" if scene_region => Value::Int(color_int(region.tint)),
            _ => return None,
        })
    }

    pub(crate) fn region_set(&mut self, r: RegionRef, name: &str, value: &Value) -> bool {
        let (text, int, flag, float) = (
            value.get_string(self),
            value.get_int(self),
            value.get_bool(self),
            value.get_float(self),
        );
        let caption = self.expand(&text);
        let scene_region = matches!(r, RegionRef::Node { .. });
        let Some(region) = self.region_ref_mut(r) else {
            return false;
        };
        match name {
            "Name" => region.name = text,
            "Caption" => region.caption = caption,
            "Active" => region.active = flag,
            "Blocked" if scene_region => region.blocked = flag,
            "Decoration" if scene_region => region.decoration = flag,
            "Scale" if scene_region => {
                region.zoom = if float == 0.0 {
                    None
                } else {
                    Some(float as f32)
                }
            }
            "AlphaColor" if scene_region => region.tint = color_of(int),
            _ => return false,
        }
        true
    }

    pub(crate) fn region_call(
        &mut self,
        r: RegionRef,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let ints: Vec<i32> = args.iter().map(|a| a.get_int(self)).collect();
        let int = |n: usize| ints.get(n).copied().unwrap_or(0);
        Some(match name {
            "GetPoint" => {
                let point = self
                    .region_ref(r)
                    .and_then(|region| {
                        usize::try_from(int(0))
                            .ok()
                            .and_then(|i| region.points.get(i))
                    })
                    .copied();
                match point {
                    Some(p) => {
                        let mut object = Value::object();
                        object.set_property("X", Value::Int(p.x), self);
                        object.set_property("Y", Value::Int(p.y), self);
                        Return::value(object)
                    }
                    None => Return::null(),
                }
            }
            "SkipTo" => {
                let (x, y) = (int(0), int(1));
                if let Some(region) = self.region_ref_mut(r) {
                    if let Some(b) = region.bounds() {
                        let (dx, dy) = (x - b.left, y - b.top);
                        for p in &mut region.points {
                            p.x += dx;
                            p.y += dy;
                        }
                    }
                }
                Return::null()
            }
            "AddPoint" | "InsertPoint" | "SetPoint" | "RemovePoint" => {
                let done = self
                    .region_ref_mut(r)
                    .and_then(|region| edit_points(&mut region.points, name, &ints))
                    .unwrap_or(false);
                Return::value(done)
            }
            _ => return None,
        })
    }

    // ----------------------------------------------------------------
    // Waypoint groups
    // ----------------------------------------------------------------

    /// The handle of a waypoint group of the scene (`GetWaypointGroup`),
    /// created on first request; `None` past the last group.
    pub fn waypoint_group_object(&mut self, index: usize) -> Option<NativeId> {
        if index >= self.scene()?.scene.waypoint_groups.len() {
            return None;
        }
        if let Some(id) = self.objects.find(|k| *k == Kind::WaypointGroup(index)) {
            return Some(id);
        }
        let id = self.objects.insert(Kind::WaypointGroup(index));
        let properties = self
            .scene()?
            .scene
            .waypoint_groups
            .get(index)?
            .properties
            .clone();
        self.apply_file_properties(id, &properties);
        Some(id)
    }

    pub(crate) fn waypoint_group_get(&self, index: usize, name: &str) -> Option<Value> {
        let group = self.scene()?.scene.waypoint_groups.get(index)?;
        Some(match name {
            "Type" => Value::from("waypoint-group"),
            "Name" => Value::from(group.name.clone()),
            "Caption" => Value::from(""),
            "Active" => Value::Bool(group.active),
            "Ready" => Value::Bool(true),
            _ => return None,
        })
    }

    pub(crate) fn waypoint_group_set(&mut self, index: usize, name: &str, value: &Value) -> bool {
        let (text, flag) = (value.get_string(self), value.get_bool(self));
        let Some(group) = self
            .scene_mut()
            .and_then(|v| v.scene.waypoint_groups.get_mut(index))
        else {
            return false;
        };
        match name {
            "Name" => group.name = text,
            "Active" => group.active = flag,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_are_edited_in_place() {
        let mut points = Vec::new();
        assert_eq!(edit_points(&mut points, "AddPoint", &[1, 2]), Some(true));
        assert_eq!(edit_points(&mut points, "AddPoint", &[5, 6]), Some(true));
        assert_eq!(
            edit_points(&mut points, "InsertPoint", &[1, 3, 4]),
            Some(true)
        );
        assert_eq!(
            edit_points(&mut points, "InsertPoint", &[3, 9, 9]),
            Some(false),
            "past the end is refused, as in the engine"
        );
        assert_eq!(edit_points(&mut points, "SetPoint", &[0, 0, 0]), Some(true));
        assert_eq!(
            edit_points(&mut points, "SetPoint", &[-1, 0, 0]),
            Some(false)
        );
        assert_eq!(edit_points(&mut points, "RemovePoint", &[2]), Some(true));
        assert_eq!(edit_points(&mut points, "RemovePoint", &[2]), Some(false));
        assert_eq!(points, vec![Point { x: 0, y: 0 }, Point { x: 3, y: 4 }]);
        assert_eq!(edit_points(&mut points, "GetPoint", &[0]), None);
    }
}
