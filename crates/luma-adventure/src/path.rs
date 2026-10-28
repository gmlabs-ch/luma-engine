//! Path finding for actors: the walkable floor, the correction of a
//! target that is not walkable, and the shortest path through waypoints
//! (see `docs/formats/actor.md`, "Walking").
//!
//! Written with reference to the path finder of `AdScene.cpp` in WME Lite
//! (MIT License, Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use luma_formats::Scene;
use luma_formats::scene::{Layer, Point, Region};

/// What the path finder asks of the scene: the floor, the polygons of
/// the other objects, and their waypoints.
pub struct Floor<'a> {
    /// The scene's main layer regions.
    pub scene: &'a Scene,
    /// Polygons of the other active objects, in scene coordinates.
    pub blockers: &'a [Vec<Point>],
    /// Active waypoints of the scene and of the other objects.
    pub waypoints: &'a [Point],
}

/// Whether a polygon contains a point (`CBRegion::PointInRegion`).
pub fn polygon_contains(points: &[Point], x: i32, y: i32) -> bool {
    Region {
        points: points.to_vec(),
        ..Region::default()
    }
    .contains(x, y)
}

impl Floor<'_> {
    /// Whether an actor is blocked at a point (`CAdScene::IsBlockedAt`
    /// with free objects): inside another object's polygon, or by the
    /// main layer's regions: the first `BLOCKED` region containing the
    /// point blocks; otherwise the point is free when a walkable region
    /// contains it, and `default_block` says what a point outside every
    /// region is.
    pub fn is_blocked_at(&self, x: i32, y: i32, default_block: bool) -> bool {
        if self.blockers.iter().any(|b| polygon_contains(b, x, y)) {
            return true;
        }
        let mut blocked = default_block;
        for region in self.scene.main_layer().into_iter().flat_map(Layer::regions) {
            if region.active && !region.decoration && region.contains(x, y) {
                if region.blocked {
                    return true;
                }
                blocked = false;
            }
        }
        blocked
    }

    /// Whether an actor can stand at a point (`CAdScene::IsWalkableAt`):
    /// not inside another object's polygon, and inside a walkable region
    /// of the main layer that no earlier `BLOCKED` region overrides.
    pub fn is_walkable_at(&self, x: i32, y: i32) -> bool {
        if self.blockers.iter().any(|b| polygon_contains(b, x, y)) {
            return false;
        }
        let mut walkable = false;
        for region in self.scene.main_layer().into_iter().flat_map(Layer::regions) {
            if region.active && !region.decoration && region.contains(x, y) {
                if region.blocked {
                    return false;
                }
                walkable = true;
            }
        }
        walkable
    }

    /// The length of the straight line between two points when nothing
    /// blocks it (`CAdScene::GetPointsDist`): the line is sampled one
    /// pixel at a time along its longer axis, and its length is that
    /// axis's distance. `None` when a sample is blocked.
    pub fn line_length(&self, p1: Point, p2: Point) -> Option<i32> {
        let (mut x1, mut y1, mut x2, mut y2) = (p1.x, p1.y, p2.x, p2.y);
        let x_length = (x2 - x1).abs();
        let y_length = (y2 - y1).abs();
        if x_length > y_length {
            if x1 > x2 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
            }
            let y_step = f64::from(y2 - y1) / f64::from(x2 - x1);
            let mut y = f64::from(y1);
            for x in x1..x2 {
                if self.is_blocked_at(x, y as i32, true) {
                    return None;
                }
                y += y_step;
            }
        } else {
            if y1 > y2 {
                std::mem::swap(&mut x1, &mut x2);
                std::mem::swap(&mut y1, &mut y2);
            }
            let x_step = if y2 == y1 {
                0.0
            } else {
                f64::from(x2 - x1) / f64::from(y2 - y1)
            };
            let mut x = f64::from(x1);
            for y in y1..y2 {
                if self.is_blocked_at(x as i32, y, true) {
                    return None;
                }
                x += x_step;
            }
        }
        Some(x_length.max(y_length))
    }

    /// Moves a target that is not walkable to a walkable point
    /// (`CAdScene::CorrectTargetPoint`): the nearest point found scanning
    /// right, left, up and down that has another walkable point 5 pixels
    /// further out, horizontal scans winning a tie; then, if still not
    /// walkable, the first walkable point on the way back from the
    /// target to `start` along the longer axis.
    pub fn correct_target(&self, start: Point, target: Point) -> Point {
        let Some(main) = self.scene.main_layer() else {
            return target;
        };
        if self.is_walkable_at(target.x, target.y) {
            return target;
        }
        let scan = |dx: i32, dy: i32, limit: i32| -> Option<i32> {
            let (mut x, mut y, mut length) = (target.x, target.y, 0);
            loop {
                let past = if dx > 0 {
                    x < limit
                } else if dx < 0 {
                    x >= 0
                } else if dy > 0 {
                    y < limit
                } else {
                    y >= 0
                };
                if !past {
                    return None;
                }
                if self.is_walkable_at(x, y) && self.is_walkable_at(x - 5 * dx, y - 5 * dy) {
                    return Some(length);
                }
                x += dx;
                y += dy;
                length += dx + dy;
            }
        };
        let right = scan(1, 0, main.width);
        let left = scan(-1, 0, 0);
        let up = scan(0, -1, 0);
        let down = scan(0, 1, main.height);
        if right.is_none() && left.is_none() && up.is_none() && down.is_none() {
            return target;
        }
        let nearest = |a: Option<i32>, b: Option<i32>| match (a, b) {
            (Some(a), Some(b)) => Some(if a.abs() < b.abs() { a } else { b }),
            (a, b) => a.or(b),
        };
        let offset_x = nearest(left, right);
        let offset_y = nearest(up, down);
        let mut corrected = target;
        match (offset_x, offset_y) {
            (Some(ox), Some(oy)) if ox.abs() < oy.abs() => corrected.x += ox,
            (Some(ox), None) => corrected.x += ox,
            (_, Some(oy)) => corrected.y += oy,
            (None, None) => {}
        }
        if self.is_walkable_at(corrected.x, corrected.y) {
            corrected
        } else {
            self.correct_target_along_line(start, corrected)
        }
    }

    /// `CAdScene::CorrectTargetPoint2`: from the target towards the
    /// start along the longer axis, the first walkable point. The
    /// original only scans while the coordinate grows, so a start on the
    /// other side leaves the target as it is.
    fn correct_target_along_line(&self, start: Point, target: Point) -> Point {
        let (x1, y1, x2, y2) = (target.x, target.y, start.x, start.y);
        let x_length = (x2 - x1).abs();
        let y_length = (y2 - y1).abs();
        if x_length > y_length {
            let y_step = (f64::from(y2 - y1) / f64::from(x2 - x1)).abs();
            let mut y = f64::from(y1);
            for x in x1..x2 {
                if self.is_walkable_at(x, y as i32) {
                    return Point { x, y: y as i32 };
                }
                y += y_step;
            }
        } else {
            let x_step = if y2 == y1 {
                0.0
            } else {
                (f64::from(x2 - x1) / f64::from(y2 - y1)).abs()
            };
            let mut x = f64::from(x1);
            for y in y1..y2 {
                if self.is_walkable_at(x as i32, y) {
                    return Point { x: x as i32, y };
                }
                x += x_step;
            }
        }
        target
    }

    /// The shortest path from `source` to `target` through the
    /// waypoints (`CAdScene::GetPath` and `PathFinderStep`): the source
    /// is moved to the nearest walkable point within 2 pixels when it is
    /// blocked, waypoints on blocked points are left out, and two
    /// vertices are joined when [`Floor::line_length`] sees between
    /// them. Returns the vertices from the (corrected) source to the
    /// target, or an empty path when the target cannot be reached.
    pub fn find_path(&self, source: Point, target: Point) -> Vec<Point> {
        let mut start = source;
        if self.is_blocked_at(source.x, source.y, true) {
            let mut best = 1000;
            for x in source.x - 2..=source.x + 2 {
                for y in source.y - 2..=source.y + 2 {
                    if self.is_walkable_at(x, y) {
                        let distance = (x - source.x).abs() + (y - source.y).abs();
                        if distance < best {
                            start = Point { x, y };
                            best = distance;
                        }
                    }
                }
            }
        }
        let mut vertices = vec![start, target];
        vertices.extend(
            self.waypoints
                .iter()
                .copied()
                .filter(|p| !self.is_blocked_at(p.x, p.y, true)),
        );
        let n = vertices.len();
        let mut distance = vec![i32::MAX; n];
        let mut origin: Vec<Option<usize>> = vec![None; n];
        let mut marked = vec![false; n];
        distance[0] = 0;
        loop {
            let lowest = (0..n)
                .filter(|&i| !marked[i])
                .min_by_key(|&i| distance[i])
                .filter(|&i| distance[i] != i32::MAX);
            let Some(current) = lowest else {
                return Vec::new();
            };
            marked[current] = true;
            if current == 1 {
                let mut path = Vec::new();
                let mut at = Some(current);
                while let Some(i) = at {
                    path.push(vertices[i]);
                    at = origin[i];
                }
                path.reverse();
                return path;
            }
            for i in 0..n {
                if marked[i] {
                    continue;
                }
                if let Some(length) = self.line_length(vertices[current], vertices[i]) {
                    if distance[current] + length < distance[i] {
                        distance[i] = distance[current] + length;
                        origin[i] = Some(current);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    fn room() -> Scene {
        Scene::parse(
            b"SCENE {
  LAYER {
    NAME=\"main\" MAIN=TRUE WIDTH=100 HEIGHT=100
    REGION { NAME=\"floor\" POINT {0,0} POINT {100,0} POINT {100,100} POINT {0,100} }
    REGION { NAME=\"wall\" BLOCKED=TRUE POINT {40,0} POINT {60,0} POINT {60,70} POINT {40,70} }
  }
}",
        )
        .unwrap()
    }

    #[test]
    fn blocked_and_walkable() {
        let scene = room();
        let floor = Floor {
            scene: &scene,
            blockers: &[],
            waypoints: &[],
        };
        assert!(floor.is_walkable_at(10, 10));
        assert!(!floor.is_walkable_at(50, 10));
        assert!(!floor.is_walkable_at(150, 10));
        assert!(!floor.is_blocked_at(10, 10, true));
        assert!(floor.is_blocked_at(50, 10, true));
        assert!(floor.is_blocked_at(150, 10, true));
        assert!(
            !floor.is_blocked_at(150, 10, false),
            "off the floor is not a block while walking"
        );
        let blockers = vec![vec![point(5, 5), point(15, 5), point(15, 15), point(5, 15)]];
        let floor = Floor {
            scene: &scene,
            blockers: &blockers,
            waypoints: &[],
        };
        assert!(!floor.is_walkable_at(10, 10));
        assert!(floor.is_blocked_at(10, 10, false));
    }

    #[test]
    fn lines_and_paths_avoid_the_wall() {
        let scene = room();
        let floor = Floor {
            scene: &scene,
            blockers: &[],
            waypoints: &[point(50, 95)],
        };
        assert_eq!(floor.line_length(point(10, 10), point(30, 20)), Some(20));
        assert_eq!(floor.line_length(point(10, 10), point(90, 10)), None);
        assert_eq!(floor.line_length(point(10, 10), point(10, 10)), Some(0));
        // Straight line free: source and target only.
        assert_eq!(
            floor.find_path(point(10, 10), point(30, 20)),
            vec![point(10, 10), point(30, 20)]
        );
        // Around the wall through the waypoint.
        assert_eq!(
            floor.find_path(point(10, 10), point(90, 10)),
            vec![point(10, 10), point(50, 95), point(90, 10)]
        );
        // No waypoint: unreachable.
        let none = Floor {
            scene: &scene,
            blockers: &[],
            waypoints: &[],
        };
        assert!(none.find_path(point(10, 10), point(90, 10)).is_empty());
        // A source one pixel inside the wall is moved out first.
        let path = floor.find_path(point(41, 10), point(10, 10));
        assert_eq!(
            path.first(),
            Some(&point(40, 10)),
            "the edge is outside the wall"
        );
    }

    #[test]
    fn corrects_targets() {
        let scene = room();
        let floor = Floor {
            scene: &scene,
            blockers: &[],
            waypoints: &[],
        };
        assert_eq!(
            floor.correct_target(point(10, 10), point(20, 20)),
            point(20, 20)
        );
        // Inside the wall, nearer its left edge: moved left, to the first
        // point with 5 free pixels behind it.
        assert_eq!(
            floor.correct_target(point(10, 10), point(45, 30)),
            point(35, 30)
        );
        // Nearer its right edge: moved right.
        assert_eq!(
            floor.correct_target(point(10, 10), point(56, 30)),
            point(65, 30)
        );
        // Below the wall is nearer than either side at y=68.
        assert_eq!(
            floor.correct_target(point(10, 10), point(50, 68)),
            point(50, 75)
        );
        // Off the scene to the right: scanned back to the edge.
        assert_eq!(
            floor.correct_target(point(10, 50), point(120, 50)),
            point(94, 50)
        );
    }
}
