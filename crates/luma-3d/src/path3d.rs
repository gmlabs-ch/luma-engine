//! The 3D path finder of a scene and the path an actor follows.
//!
//! A [`PathFinder`] answers one request at a time: it lifts the two ends
//! to the waypoint height, runs a plain Dijkstra over the source, the
//! target and the waypoints of the active groups, with an edge wherever
//! [`SceneGeometry::direct_path_exists`] says the straight segment is free,
//! and, when the target cannot be reached, reruns once toward an
//! *alternate target* short of the nearest block hit. The result is a
//! [`Path3D`], the list of points the actor walks through with a cursor.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x sources
//! (`AdSceneGeometry.cpp`, `AdPath3D.cpp`, `AdPathPoint3D.cpp`, Copyright
//! (c) 2003-2013 Jan Nedoma). See `NOTICE` at the root of the repository.

use crate::math::Vec3;
use crate::scene_geometry::SceneGeometry;

/// How far the alternate target backs off from the block hit toward the
/// node the blocked edge started from, in world units.
pub const ALTERNATE_TARGET_BACKOFF: f32 = 30.0;

/// A path of 3D points with a cursor (`CAdPath3D`). An actor takes the
/// points with [`Self::first`] and [`Self::advance`] and reads the one it is
/// walking toward with [`Self::current`].
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Path3D {
    points: Vec<Vec3>,
    /// The cursor: `None` before `first`/`advance`, else an index that may
    /// run past the end (the original's `m_CurrIndex`, `-1` and up).
    cursor: Option<usize>,
    ready: bool,
}

impl Path3D {
    /// An empty path, not ready, cursor before the first point.
    pub fn new() -> Path3D {
        Path3D::default()
    }

    /// A path from saved state: its points, its cursor and whether it is
    /// ready.
    pub fn from_parts(points: Vec<Vec3>, cursor: Option<usize>, ready: bool) -> Path3D {
        Path3D {
            points,
            cursor,
            ready,
        }
    }

    /// Drops the points, rewinds the cursor and clears the ready flag
    /// (`Reset`).
    pub fn reset(&mut self) {
        self.points.clear();
        self.cursor = None;
        self.ready = false;
    }

    /// Appends a point (`AddPoint`).
    pub fn add_point(&mut self, point: Vec3) {
        self.points.push(point);
    }

    /// Sets the ready flag and returns its previous value (`SetReady`).
    pub fn set_ready(&mut self, ready: bool) -> bool {
        std::mem::replace(&mut self.ready, ready)
    }

    /// Whether the path finder has finished this path (`m_Ready`). A
    /// ready path may be empty: no route, or the target was the start.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// The points, in walking order.
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// The cursor, for saving: `None` before the first point.
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Rewinds the cursor to the first point and returns it (`GetFirst`);
    /// `None`, and the cursor untouched, for an empty path.
    pub fn first(&mut self) -> Option<Vec3> {
        let point = *self.points.first()?;
        self.cursor = Some(0);
        Some(point)
    }

    /// Advances the cursor and returns the point there (`GetNext`; the
    /// name avoids `Iterator::next`): from
    /// the initial position it yields the first point; past the end it
    /// keeps advancing and yields `None`.
    pub fn advance(&mut self) -> Option<Vec3> {
        let index = self.cursor.map_or(0, |i| i.saturating_add(1));
        self.cursor = Some(index);
        self.points.get(index).copied()
    }

    /// The point under the cursor (`GetCurrent`); `None` before the first
    /// point or past the end.
    pub fn current(&self) -> Option<Vec3> {
        self.points.get(self.cursor?).copied()
    }
}

/// The handle of an accepted request: the path it produces is read back
/// with it, so that a later request cannot be mistaken for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(u64);

/// A node of the search (`CAdPathPoint3D`).
#[derive(Debug, Clone, PartialEq)]
struct Node {
    pos: Vec3,
    /// `f32::MAX` until reached.
    distance: f32,
    marked: bool,
    /// The node this one was reached from.
    origin: Option<usize>,
}

impl Node {
    fn new(pos: Vec3, distance: f32) -> Node {
        Node {
            pos,
            distance,
            marked: false,
            origin: None,
        }
    }
}

/// The scene's path finder (the `m_PF*` state of `CAdSceneGeometry`).
///
/// One request is open at a time: [`Self::request`] refuses while busy,
/// and the actor asks again next frame. [`Self::run`] computes the open
/// request to the end, as the original's `InitLoop` does once per game
/// loop (its 15 ms budget compares a frame time that does not change
/// during the loop, so the loop only ends when the finder is done).
/// [`Self::take_path`] then hands the result to the holder of the ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct PathFinder {
    busy: bool,
    rerun: bool,
    source: Vec3,
    target: Vec3,
    /// The origin means "none", as in the original.
    alternate_target: Vec3,
    alternate_dist: f32,
    nodes: Vec<Node>,
    ticket: Ticket,
    result: Path3D,
}

impl Default for PathFinder {
    fn default() -> PathFinder {
        PathFinder::new()
    }
}

impl PathFinder {
    /// An idle path finder.
    pub fn new() -> PathFinder {
        PathFinder {
            busy: false,
            rerun: false,
            source: Vec3::ZERO,
            target: Vec3::ZERO,
            alternate_target: Vec3::ZERO,
            alternate_dist: f32::MAX,
            nodes: Vec::new(),
            ticket: Ticket(0),
            result: Path3D::new(),
        }
    }

    /// Whether a request is being computed (`!m_PFReady`).
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Opens a request from `source` to `target` (`GetPath`): `None` while
    /// another request is open, else the ticket to read the result with.
    /// Both ends are lifted to `floor + waypoint_height`, the floor
    /// measured with the waypoint height as tolerance (a point over no
    /// walk plane keeps its own `y`, plus the height).
    pub fn request(
        &mut self,
        geometry: &SceneGeometry,
        source: Vec3,
        target: Vec3,
    ) -> Option<Ticket> {
        if self.busy {
            return None;
        }
        self.ticket = Ticket(self.ticket.0.wrapping_add(1));
        self.start(geometry, source, target, false);
        Some(self.ticket)
    }

    /// The finished path of a request, once computed. `None` while the
    /// request is open, for a ticket that is not the latest, or after the
    /// path was taken.
    pub fn path(&self, ticket: Ticket) -> Option<&Path3D> {
        (!self.busy && ticket == self.ticket && self.result.is_ready()).then_some(&self.result)
    }

    /// Takes the finished path of a request, leaving an empty unready
    /// one behind; `None` as for [`Self::path`].
    pub fn take_path(&mut self, ticket: Ticket) -> Option<Path3D> {
        self.path(ticket)?;
        Some(std::mem::take(&mut self.result))
    }

    /// Runs the open request to the end (`InitLoop`); nothing when idle.
    pub fn run(&mut self, geometry: &SceneGeometry) {
        while self.busy {
            self.step(geometry);
        }
    }

    /// Requests a path and runs it at once; the result is ready, empty
    /// when there is no route.
    pub fn find_path(geometry: &SceneGeometry, source: Vec3, target: Vec3) -> Path3D {
        let mut finder = PathFinder::new();
        let ticket = finder
            .request(geometry, source, target)
            .expect("a new path finder is idle");
        finder.run(geometry);
        finder
            .take_path(ticket)
            .expect("the request just ran to the end")
    }

    /// The body of `GetPath` once accepted; `rerun` marks the second pass
    /// toward the alternate target.
    fn start(&mut self, geometry: &SceneGeometry, mut source: Vec3, mut target: Vec3, rerun: bool) {
        let height = geometry.waypoint_height;
        source.y = geometry.height_at(source, height).unwrap_or(source.y) + height;
        target.y = geometry.height_at(target, height).unwrap_or(target.y) + height;

        self.busy = true;
        self.rerun = rerun;
        self.source = source;
        self.target = target;
        self.alternate_target = Vec3::ZERO;
        self.alternate_dist = f32::MAX;
        self.result.reset();

        self.nodes.clear();
        self.nodes.push(Node::new(source, 0.0));
        self.nodes.push(Node::new(target, f32::MAX));
        for group in geometry.waypoint_groups.iter().filter(|g| g.active) {
            self.nodes
                .extend(group.points.iter().map(|&p| Node::new(p, f32::MAX)));
        }
    }

    /// One step of the search (`PathFinderStep`): marks the unmarked node
    /// nearest to the source and either builds the path (it is the
    /// target), relaxes its neighbours, or ends the request (none left).
    pub fn step(&mut self, geometry: &SceneGeometry) {
        if !self.busy {
            return;
        }
        // The lowest unmarked node; the first one wins ties, and a node
        // still at f32::MAX never qualifies.
        let mut lowest: Option<usize> = None;
        let mut lowest_dist = f32::MAX;
        for (i, node) in self.nodes.iter().enumerate() {
            if !node.marked && node.distance < lowest_dist {
                lowest_dist = node.distance;
                lowest = Some(i);
            }
        }

        let Some(current) = lowest else {
            // No route. Once, retry toward the alternate target.
            self.busy = false;
            if !self.rerun && self.alternate_target != Vec3::ZERO {
                let (source, alternate) = (self.source, self.alternate_target);
                self.start(geometry, source, alternate, true);
            } else {
                self.result.set_ready(true);
            }
            return;
        };

        self.nodes[current].marked = true;
        let current_pos = self.nodes[current].pos;
        let current_dist = self.nodes[current].distance;

        if current_pos == self.target {
            // Follow the origins back to the source, then drop the source.
            let mut points = Vec::new();
            let mut at = Some(current);
            while let Some(i) = at {
                points.push(self.nodes[i].pos);
                at = self.nodes[i].origin;
            }
            points.reverse();
            self.result.reset();
            for point in points.into_iter().skip(1) {
                self.result.add_point(point);
            }
            self.result.set_ready(true);
            self.busy = false;
            return;
        }

        for i in 0..self.nodes.len() {
            if self.nodes[i].marked {
                continue;
            }
            let pos = self.nodes[i].pos;
            let free = geometry.direct_path_exists(current_pos, pos);
            let dist = (pos - current_pos).length();
            if free && current_dist + dist < self.nodes[i].distance {
                self.nodes[i].distance = current_dist + dist;
                self.nodes[i].origin = Some(current);
            } else if !self.rerun && pos == self.target && dist < self.alternate_dist {
                // The shortest edge to the target that did not improve it
                // supplies the alternate target: the block hit, backed off
                // toward this node. As in the original, this also runs for
                // a free edge that was merely longer; it then yields a
                // point near the origin, which is never used because the
                // target was reached.
                self.alternate_dist = dist;
                let hit = geometry
                    .block_intersection(current_pos, pos)
                    .unwrap_or(Vec3::ZERO);
                let dir = (hit - current_pos).normalized();
                self.alternate_target = hit - dir * ALTERNATE_TARGET_BACKOFF;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_patterns::NodePatterns;
    use crate::scene_geometry::WaypointGroup;
    use crate::scene_geometry::tests::{box_mesh, floor, wall_scene};
    use crate::three_ds::Builder;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-2
    }

    /// A small waypoint box whose centre drops to `(x, 10, z)`.
    fn waypoint(name: &str, x: f32, z: f32) -> Vec<u8> {
        box_mesh(
            name,
            Vec3::new(x - 1.0, 40.0, z - 1.0),
            Vec3::new(x + 1.0, 42.0, z + 1.0),
        )
    }

    /// The wall scene with four waypoints around the wall, in the given
    /// order: `a` (30, 10), `b` (70, 10), `c` (30, 90), `d` (70, 90).
    fn wall_with_waypoints(order: &[&str]) -> SceneGeometry {
        let mut objects = vec![
            floor("walk_01", 0.0),
            box_mesh(
                "blk_wall",
                Vec3::new(40.0, 0.0, 20.0),
                Vec3::new(60.0, 30.0, 80.0),
            ),
        ];
        for name in order {
            let (x, z) = match *name {
                "a" => (30.0, 10.0),
                "b" => (70.0, 10.0),
                "c" => (30.0, 90.0),
                "d" => (70.0, 90.0),
                _ => unreachable!(),
            };
            objects.push(waypoint(&format!("wpt_{name}"), x, z));
        }
        let bytes = Builder::file(&objects, &[]);
        SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap()
    }

    const A: Vec3 = Vec3::new(30.0, 10.0, 10.0);
    const B: Vec3 = Vec3::new(70.0, 10.0, 10.0);
    const C: Vec3 = Vec3::new(30.0, 10.0, 90.0);
    const D: Vec3 = Vec3::new(70.0, 10.0, 90.0);

    #[test]
    fn the_path_consumer_walks_the_points_in_order() {
        let mut path = Path3D::new();
        assert!(!path.is_ready());
        assert_eq!(path.first(), None);
        assert_eq!(path.current(), None);
        assert_eq!(path.cursor(), None);
        path.add_point(Vec3::new(1.0, 0.0, 0.0));
        path.add_point(Vec3::new(2.0, 0.0, 0.0));
        assert!(!path.set_ready(true));
        assert!(path.set_ready(true));
        assert_eq!(path.current(), None);
        assert_eq!(path.first(), Some(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(path.current(), Some(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(path.advance(), Some(Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(path.advance(), None);
        assert_eq!(path.current(), None);
        assert_eq!(path.advance(), None);
        assert_eq!(path.cursor(), Some(3));
        // `advance` before `first` yields the first point, as in the original.
        path.reset();
        assert!(path.points().is_empty() && !path.is_ready());
        path.add_point(Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(path.advance(), Some(Vec3::new(5.0, 0.0, 0.0)));
        let saved = Path3D::from_parts(path.points().to_vec(), path.cursor(), true);
        assert_eq!(saved.current(), Some(Vec3::new(5.0, 0.0, 0.0)));
        assert!(saved.is_ready());
    }

    #[test]
    fn the_path_goes_around_the_wall_through_the_first_waypoints_in_file_order() {
        let g = wall_with_waypoints(&["a", "b", "c", "d"]);
        let s = Vec3::new(10.0, 0.0, 50.0);
        let t = Vec3::new(90.0, 0.0, 50.0);
        let path = PathFinder::find_path(&g, s, t);
        assert!(path.is_ready());
        // Both ways round are equally long; the waypoints listed first win
        // the ties, and the start is not in the path.
        assert_eq!(path.points(), &[A, B, Vec3::new(90.0, 10.0, 50.0)]);

        let g = wall_with_waypoints(&["c", "d", "a", "b"]);
        let path = PathFinder::find_path(&g, s, t);
        assert_eq!(path.points(), &[C, D, Vec3::new(90.0, 10.0, 50.0)]);

        // A free line needs no waypoint.
        let path = PathFinder::find_path(&g, s, Vec3::new(20.0, 0.0, 90.0));
        assert_eq!(path.points(), &[Vec3::new(20.0, 10.0, 90.0)]);
    }

    #[test]
    fn one_request_at_a_time_and_the_ticket_reads_it_back() {
        let g = wall_with_waypoints(&["a", "b", "c", "d"]);
        let mut finder = PathFinder::new();
        assert!(!finder.is_busy());
        let s = Vec3::new(10.0, 0.0, 50.0);
        let t = Vec3::new(90.0, 0.0, 50.0);
        let ticket = finder.request(&g, s, t).unwrap();
        assert!(finder.is_busy());
        assert_eq!(finder.request(&g, s, t), None);
        assert_eq!(finder.path(ticket), None);
        finder.step(&g);
        assert!(finder.is_busy());
        finder.run(&g);
        assert!(!finder.is_busy());
        assert_eq!(finder.path(ticket).unwrap().points().len(), 3);
        assert_eq!(finder.path(Ticket(99)), None);
        assert_eq!(finder.take_path(Ticket(99)), None);
        let path = finder.take_path(ticket).unwrap();
        assert_eq!(path.points().len(), 3);
        assert_eq!(finder.take_path(ticket), None);
        // Stepping an idle finder does nothing.
        finder.step(&g);
        assert!(!finder.is_busy());
        // A new request supersedes the old ticket.
        let second = finder.request(&g, t, s).unwrap();
        assert_ne!(second, ticket);
        finder.run(&g);
        assert_eq!(finder.path(ticket), None);
        assert_eq!(
            finder.path(second).unwrap().points(),
            &[B, A, Vec3::new(10.0, 10.0, 50.0)]
        );
    }

    #[test]
    fn an_unreachable_target_reruns_toward_the_nearest_block_hit() {
        let g = wall_with_waypoints(&["a", "b", "c", "d"]);
        // The target stands inside the wall, 5 units from its +x face.
        let s = Vec3::new(5.0, 0.0, 50.0);
        let t = Vec3::new(55.0, 0.0, 50.0);
        let path = PathFinder::find_path(&g, s, t);
        assert!(path.is_ready());
        // The shortest blocked edge to the target starts at b and pierces
        // the +x face at (60, 10, 36.67); the alternate target is 30 units
        // back from there toward b, and the rerun reaches it through a.
        assert_eq!(path.points().len(), 2, "{:?}", path.points());
        assert_eq!(path.points()[0], A);
        assert!(
            close(path.points()[1], Vec3::new(70.534, 10.0, 8.577)),
            "{:?}",
            path.points()[1]
        );
    }

    #[test]
    fn the_rerun_may_lead_nowhere() {
        // No waypoints: the only blocked edge is source -> target, its hit
        // is the -x face at x = 40, and 30 units back is the source
        // itself, so the rerun finds an empty path.
        let g = wall_scene();
        let s = Vec3::new(10.0, 0.0, 50.0);
        let t = Vec3::new(90.0, 0.0, 50.0);
        let path = PathFinder::find_path(&g, s, t);
        assert!(path.is_ready());
        assert!(path.points().is_empty());
        // From nearer the wall the alternate target lies behind the actor.
        let path = PathFinder::find_path(&g, Vec3::new(20.0, 0.0, 50.0), t);
        assert_eq!(path.points(), &[Vec3::new(10.0, 10.0, 50.0)]);
        // Source and target the same: an empty, ready path.
        let path = PathFinder::find_path(&g, s, s);
        assert!(path.is_ready() && path.points().is_empty());
    }

    #[test]
    fn an_inactive_waypoint_group_is_ignored() {
        let mut g = wall_with_waypoints(&["a", "b", "c", "d"]);
        let s = Vec3::new(20.0, 0.0, 50.0);
        let t = Vec3::new(90.0, 0.0, 50.0);
        g.waypoint_groups[0].active = false;
        let path = PathFinder::find_path(&g, s, t);
        assert_eq!(path.points(), &[Vec3::new(10.0, 10.0, 50.0)]);
        // A second, active group is used instead.
        let e = Vec3::new(35.0, 10.0, 5.0);
        let f = Vec3::new(65.0, 10.0, 5.0);
        g.waypoint_groups.push(WaypointGroup {
            points: vec![e, f],
            active: true,
        });
        let path = PathFinder::find_path(&g, s, t);
        assert_eq!(path.points(), &[e, f, Vec3::new(90.0, 10.0, 50.0)]);
        g.waypoint_groups[0].active = true;
        let path = PathFinder::find_path(&g, s, t);
        assert_eq!(path.points(), &[A, B, Vec3::new(90.0, 10.0, 50.0)]);
    }

    #[test]
    fn path_ends_are_lifted_over_a_sloped_floor() {
        // A floor rising from y = 0 at z = 0 to y = 50 at z = 100.
        let slope = Builder::mesh(
            "walk_slope",
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 50.0, 100.0),
                Vec3::new(100.0, 50.0, 100.0),
                Vec3::new(100.0, 0.0, 0.0),
            ],
            &[[0, 1, 2], [0, 2, 3]],
        );
        let bytes = Builder::file(&[slope], &[]);
        let mut g = SceneGeometry::load_3ds(&bytes, &NodePatterns::standard()).unwrap();
        let near = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 1e-4);
        assert!(near(g.height_at(Vec3::new(50.0, 100.0, 25.0), 0.0), 12.5));
        assert!(near(g.height_at(Vec3::new(50.0, 0.0, 25.0), 20.0), 12.5));
        assert_eq!(g.height_at(Vec3::new(50.0, 0.0, 25.0), 5.0), None);
        let single = |g: &SceneGeometry, target: Vec3| {
            let path = PathFinder::find_path(g, Vec3::new(50.0, 100.0, 20.0), target);
            assert_eq!(path.points().len(), 1, "{:?}", path.points());
            path.points()[0]
        };
        // The floor is 40 high at z = 80; the ends are lifted 10 over it.
        assert!(close(
            single(&g, Vec3::new(50.0, 100.0, 80.0)),
            Vec3::new(50.0, 50.0, 80.0)
        ));
        g.set_waypoint_height(2.0);
        assert!(close(
            single(&g, Vec3::new(50.0, 100.0, 80.0)),
            Vec3::new(50.0, 42.0, 80.0)
        ));
        // Off the floor an end keeps its own height plus the waypoint
        // height.
        assert!(close(
            single(&g, Vec3::new(500.0, 50.0, 80.0)),
            Vec3::new(500.0, 52.0, 80.0)
        ));
        // Off the floor and under the plane's extension: the segment from
        // the lifted source (50, 12, 20) dips through the floor, no block
        // is hit, and the alternate target is the original's bogus point
        // 30 units from the origin toward the source, lifted in turn.
        let bogus = Vec3::new(50.0, 12.0, 20.0).normalized() * 30.0;
        let got = single(&g, Vec3::new(500.0, 7.0, 80.0));
        assert!(
            close(got, Vec3::new(bogus.x, bogus.z / 2.0 + 2.0, bogus.z)),
            "{got:?}"
        );
    }
}
