//! The `.geometry` sidecar: which mesh of the `.3ds` file is a walk plane,
//! a blocked mesh, a waypoint or a generic shadow receiver.
//!
//! The file is a [definition file](https://github.com/gmlabs-ch/luma-engine/blob/main/docs/formats/definition-files.md)
//! with a `GEOMETRY` root block of `NODE` blocks, each a name pattern and
//! a role. After the file's nodes come three **built-in** nodes (`walk_*`,
//! `blk_*`, `wpt_*`); a mesh takes the role of the first node whose pattern
//! matches its name, and a mesh matching nothing is dropped. A file that
//! stops on an error keeps the nodes read before it. The format is
//! described in `docs/formats/geometry.md`.
//!
//! Written with reference to the MIT-licensed Wintermute Engine 1.x sources
//! (`AdGeomExt.cpp`, `AdGeomExtNode.cpp`, `utils.cpp`, Copyright (c)
//! 2003-2013 Jan Nedoma). See `NOTICE` at the root of the repository.

use luma_formats::def::{Document, Node, keys};
use luma_formats::{Error, Result};

/// The keys of `CAdGeomExt::LoadBuffer`, in the order of its table.
const GEOMETRY_KEYS: &[&str] = &["GEOMETRY", "NODE"];

/// The keys of `CAdGeomExtNode::LoadBuffer`, in the order of its table.
const NODE_KEYS: &[&str] = &[
    "NODE",
    "NAME",
    "WALKPLANE",
    "BLOCKED",
    "WAYPOINT",
    "RECEIVE_SHADOWS",
];

/// The role of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A mesh that only receives shadows (when flagged) and is drawn by
    /// the debug overlay.
    Generic,
    /// A surface actors stand and walk on.
    Walkplane,
    /// An obstacle that stops paths.
    Blocked,
    /// A marker whose bounding-box centre is a waypoint.
    Waypoint,
}

/// One `NODE` of the sidecar, or a built-in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePattern {
    /// The name pattern: `*` matches any run of characters, `?` any one,
    /// case-insensitive.
    pub pattern: String,
    /// The role of the meshes it matches.
    pub kind: NodeKind,
    /// Whether the meshes receive stencil shadows.
    pub receive_shadows: bool,
}

impl NodePattern {
    /// A node from its parts.
    pub fn new(pattern: &str, kind: NodeKind, receive_shadows: bool) -> NodePattern {
        NodePattern {
            pattern: pattern.to_string(),
            kind,
            receive_shadows,
        }
    }

    /// Whether a mesh name matches the pattern.
    pub fn matches(&self, name: &str) -> bool {
        matches_pattern(&self.pattern, name)
    }
}

/// The ordered node list a geometry file is classified with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NodePatterns {
    /// The nodes, first match wins.
    pub nodes: Vec<NodePattern>,
}

impl NodePatterns {
    /// The three built-in nodes alone: `walk_*` walk planes that receive
    /// shadows, `blk_*` blocks and `wpt_*` waypoints that do not. This is
    /// what a geometry without a sidecar gets.
    pub fn standard() -> NodePatterns {
        let mut patterns = NodePatterns::default();
        patterns.add_standard_nodes();
        patterns
    }

    /// Appends the three built-in nodes.
    pub fn add_standard_nodes(&mut self) {
        self.nodes
            .push(NodePattern::new("walk_*", NodeKind::Walkplane, true));
        self.nodes
            .push(NodePattern::new("blk_*", NodeKind::Blocked, false));
        self.nodes
            .push(NodePattern::new("wpt_*", NodeKind::Waypoint, false));
    }

    /// Reads a `.geometry` file as the original does
    /// (`CAdGeomExt::LoadBuffer`, then
    /// `CAdSceneGeometry::GetGeometryExtension`) and returns the node
    /// list a geometry gets from it, with the error that ended the
    /// reading when there is one.
    ///
    /// The first statement must be the `GEOMETRY` block. Its `NODE`
    /// blocks are read in order; one that fails (a key a node does not
    /// know) is left out alone, since the error code it sets is
    /// overwritten when the next statement is read. A `GEOMETRY` key
    /// inside the block is ignored. Any other key stops the reading with
    /// an error. In every case the built-in nodes come after the nodes
    /// read: the original appends them itself when the file loads, and
    /// its caller does when it fails, without dropping what was read.
    pub fn read(bytes: &[u8]) -> (NodePatterns, Option<Error>) {
        match Document::parse(bytes) {
            Ok(doc) => Self::read_document(&doc),
            Err(err) => (Self::standard(), Some(err.into())),
        }
    }

    /// [`Self::read`] for a parsed document.
    pub fn read_document(doc: &Document) -> (NodePatterns, Option<Error>) {
        let mut patterns = NodePatterns::default();
        let mut error = None;
        match doc.root("GEOMETRY") {
            None => {
                error = Some(Error::Invalid {
                    reason: "'GEOMETRY' keyword expected".into(),
                });
            }
            Some(root) => {
                for item in keys(root, GEOMETRY_KEYS, "geometry description") {
                    match item {
                        Ok(("NODE", node)) => {
                            patterns.nodes.extend(NodePattern::from_node(node).ok());
                        }
                        Ok(_) => {}
                        Err(err) => {
                            error = Some(err.into());
                            break;
                        }
                    }
                }
            }
        }
        patterns.add_standard_nodes();
        (patterns, error)
    }

    /// Parses a `.geometry` file: the nodes of [`Self::read`] when the
    /// original's loader succeeds, its error otherwise (a syntax error, a
    /// first statement other than `GEOMETRY`, an unknown key in the root
    /// block). A geometry is classified with the nodes read even then:
    /// see [`Self::load`].
    pub fn parse(bytes: &[u8]) -> Result<NodePatterns> {
        match Self::read(bytes) {
            (patterns, None) => Ok(patterns),
            (_, Some(err)) => Err(err),
        }
    }

    /// [`Self::parse`] for a parsed document.
    pub fn from_document(doc: &Document) -> Result<NodePatterns> {
        match Self::read_document(doc) {
            (patterns, None) => Ok(patterns),
            (_, Some(err)) => Err(err),
        }
    }

    /// The list for a geometry: the nodes of the sidecar when it exists,
    /// as far as it could be read ([`Self::read`]), then the built-in
    /// nodes; without a sidecar the built-in nodes alone.
    pub fn load(sidecar: Option<&[u8]>) -> NodePatterns {
        match sidecar {
            Some(bytes) => Self::read(bytes).0,
            None => Self::standard(),
        }
    }

    /// The first node whose pattern matches a mesh name, `None` when the
    /// mesh belongs to no node and is dropped.
    pub fn classify(&self, name: &str) -> Option<&NodePattern> {
        self.nodes.iter().find(|node| node.matches(name))
    }
}

impl NodePattern {
    /// Builds a node from a `NODE` block (`CAdGeomExtNode::LoadBuffer`).
    /// Each role key that is `TRUE` sets the role, so the last true one
    /// wins; none means generic. `RECEIVE_SHADOWS` with an empty value
    /// keeps the flag. A `NODE` key inside the block is ignored; any
    /// other key fails the node. A node without `NAME` has an empty
    /// pattern, which only matches an empty name (the original would
    /// dereference a null pattern).
    pub fn from_node(node: &Node) -> Result<NodePattern> {
        let mut pattern = NodePattern::new("", NodeKind::Generic, false);
        for item in keys(node, NODE_KEYS, "geometry description") {
            let (key, child) = item?;
            let is_true = || child.as_bool() == Some(true);
            match key {
                "NAME" => pattern.pattern = child.text(),
                "RECEIVE_SHADOWS" => child.read_bool(&mut pattern.receive_shadows),
                "WALKPLANE" if is_true() => pattern.kind = NodeKind::Walkplane,
                "BLOCKED" if is_true() => pattern.kind = NodeKind::Blocked,
                "WAYPOINT" if is_true() => pattern.kind = NodeKind::Waypoint,
                _ => {}
            }
        }
        Ok(pattern)
    }
}

/// The engine's wildcard matcher (`CBUtils::MatchesPattern`): `*` matches
/// any run of characters, `?` exactly one, letters match ignoring ASCII
/// case. A `.` in the pattern has the DOS file-name rules of the original:
/// a trailing `.` matches the end of a name without extension, and `*.`
/// followed by an extension matches names with that extension.
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    matches(pattern.as_bytes(), name.as_bytes())
}

fn matches(mut pattern: &[u8], mut name: &[u8]) -> bool {
    loop {
        let name_char = name.first().map(u8::to_ascii_uppercase);
        let Some(&first) = pattern.first() else {
            return name_char.is_none();
        };
        let pattern_char = first.to_ascii_uppercase();
        pattern = &pattern[1..];
        match pattern_char {
            b'?' => {
                if name_char.is_none() {
                    return false;
                }
            }
            b'*' => {
                if pattern.is_empty() {
                    return true;
                }
                if pattern[0] == b'.' {
                    if pattern.len() == 2 && pattern[1] == b'*' {
                        return true;
                    }
                    let dot = name.iter().position(|&c| c == b'.');
                    if pattern.len() == 1 {
                        return match dot {
                            None => true,
                            Some(i) => i + 1 == name.len(),
                        };
                    }
                    if let Some(i) = dot {
                        name = &name[i..];
                        let plain = !pattern.iter().any(|c| matches!(c, b'*' | b'?' | b'['));
                        if plain && !name[1..].contains(&b'.') {
                            return pattern[1..].eq_ignore_ascii_case(&name[1..]);
                        }
                    }
                }
                let mut rest = name;
                while !rest.is_empty() {
                    if matches(pattern, rest) {
                        return true;
                    }
                    rest = &rest[1..];
                }
                return false;
            }
            _ => {
                if Some(pattern_char) != name_char {
                    if pattern_char == b'.' && name_char.is_none() {
                        return matches(pattern, name);
                    }
                    return false;
                }
            }
        }
        name = &name[1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_match_like_the_original() {
        assert!(matches_pattern("walk_*", "walk_01"));
        assert!(matches_pattern("walk_*", "WALK_floor"));
        assert!(matches_pattern("walk_*", "walk_"));
        assert!(!matches_pattern("walk_*", "blk_01"));
        assert!(matches_pattern("*", ""));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("shadow_Bo?", "shadow_Bo0"));
        assert!(!matches_pattern("shadow_Bo?", "shadow_Bo"));
        assert!(!matches_pattern("shadow_Bo?", "shadow_Bo00"));
        assert!(matches_pattern("*_01", "wpt_01"));
        assert!(!matches_pattern("*_01", "wpt_02"));
        assert!(matches_pattern("Camera01", "camera01"));
        assert!(!matches_pattern("Camera01", "camera012"));
        assert!(!matches_pattern("", "x"));
        assert!(matches_pattern("", ""));
    }

    #[test]
    fn dots_follow_the_dos_rules() {
        // A trailing dot matches a name without extension.
        assert!(matches_pattern("walk.", "walk"));
        assert!(!matches_pattern("walk.", "walk.x"));
        // `*.` matches names without an extension (or ending with the dot).
        assert!(matches_pattern("*.", "name"));
        assert!(matches_pattern("*.", "name."));
        assert!(!matches_pattern("*.", "name.ext"));
        // `*.*` matches everything.
        assert!(matches_pattern("*.*", "name"));
        assert!(matches_pattern("*.*", "name.ext"));
        // `*.ext` matches that extension.
        assert!(matches_pattern("*.3ds", "geometry.3DS"));
        assert!(!matches_pattern("*.3ds", "geometry.x"));
        assert!(!matches_pattern("*.3ds", "geometry"));
    }

    #[test]
    fn the_standard_nodes_classify_the_conventional_names() {
        let std = NodePatterns::standard();
        assert_eq!(std.classify("walk_01").unwrap().kind, NodeKind::Walkplane);
        assert!(std.classify("walk_01").unwrap().receive_shadows);
        assert_eq!(std.classify("blk_wareho").unwrap().kind, NodeKind::Blocked);
        assert!(!std.classify("blk_wareho").unwrap().receive_shadows);
        assert_eq!(std.classify("WPT_03").unwrap().kind, NodeKind::Waypoint);
        assert!(std.classify("shadow_Bo0").is_none());
    }

    #[test]
    fn a_sidecar_adds_nodes_before_the_standard_ones() {
        let text = r#"GEOMETRY
{
  NODE
  {
    NAME = "shadow_*"
    RECEIVE_SHADOWS = TRUE
  }
  NODE
  {
    NAME = "walk_special"
    WALKPLANE = TRUE
    BLOCKED = TRUE
    RECEIVE_SHADOWS = FALSE
  }
}
"#;
        let patterns = NodePatterns::parse(text.as_bytes()).unwrap();
        assert_eq!(patterns.nodes.len(), 5);
        let shadow = patterns.classify("shadow_Bo0").unwrap();
        assert_eq!(shadow.kind, NodeKind::Generic);
        assert!(shadow.receive_shadows);
        // The last true role wins, and the file's node comes before the
        // built-in `walk_*`.
        let special = patterns.classify("walk_special").unwrap();
        assert_eq!(special.kind, NodeKind::Blocked);
        assert!(!special.receive_shadows);
        assert_eq!(
            patterns.classify("walk_01").unwrap().kind,
            NodeKind::Walkplane
        );
    }

    /// `CAdGeomExt::LoadBuffer` and `GetGeometryExtension`: whatever
    /// happens, the nodes read come first and the built-in ones after.
    #[test]
    fn a_sidecar_that_fails_keeps_the_nodes_read_before_the_error() {
        let names = |patterns: &NodePatterns| -> Vec<String> {
            patterns.nodes.iter().map(|n| n.pattern.clone()).collect()
        };
        // An unknown key in the root block stops the file there.
        let text = b"GEOMETRY { NODE { NAME = \"a_*\" BLOCKED = TRUE } OTHER = 1 NODE { NAME = \"b_*\" } }";
        assert!(NodePatterns::parse(text).is_err());
        let (patterns, error) = NodePatterns::read(text);
        assert!(error.unwrap().to_string().contains("OTHER"));
        assert_eq!(names(&patterns), ["a_*", "walk_*", "blk_*", "wpt_*"]);
        assert_eq!(NodePatterns::load(Some(text)), patterns);
        assert_eq!(patterns.classify("a_1").unwrap().kind, NodeKind::Blocked);
        assert!(patterns.classify("b_1").is_none());

        // A node that fails is left out alone and the file loads.
        let text = b"GEOMETRY { NODE { NAME = \"x_*\" COLOR = 1 } NODE { NAME = \"y_*\" } }";
        let patterns = NodePatterns::parse(text).unwrap();
        assert_eq!(names(&patterns), ["y_*", "walk_*", "blk_*", "wpt_*"]);

        // A GEOMETRY key inside the block is ignored.
        let text = b"GEOMETRY { GEOMETRY { NODE { NAME = \"in_*\" } } NODE { NAME = \"z_*\" } }";
        let patterns = NodePatterns::parse(text).unwrap();
        assert_eq!(names(&patterns), ["z_*", "walk_*", "blk_*", "wpt_*"]);

        // The GEOMETRY block must come first; a file that is not a
        // definition file at all gives the built-in nodes.
        assert!(NodePatterns::parse(b"NOT_GEOMETRY { }").is_err());
        assert!(NodePatterns::parse(b"NODE { NAME = \"n\" } GEOMETRY { }").is_err());
        assert!(NodePatterns::parse(b"{ GEOMETRY").is_err());
        assert_eq!(
            NodePatterns::load(Some(b"NOT_GEOMETRY { }")),
            NodePatterns::standard()
        );
        assert_eq!(
            NodePatterns::load(Some(b"{ GEOMETRY")),
            NodePatterns::standard()
        );
        assert_eq!(NodePatterns::load(None), NodePatterns::standard());
        let loaded = NodePatterns::load(Some(b"GEOMETRY { NODE { NAME = \"g_*\" } }"));
        assert_eq!(loaded.nodes.len(), 4);
        assert_eq!(loaded.classify("g_1").unwrap().kind, NodeKind::Generic);
    }

    #[test]
    fn node_values_are_scanned_as_the_original_does() {
        let node = |text: &str| {
            let doc = Document::parse(format!("NODE {{ {text} }}").as_bytes()).unwrap();
            NodePattern::from_node(&doc.nodes[0])
        };
        // An empty RECEIVE_SHADOWS keeps the flag; the last one counts.
        let n =
            node("RECEIVE_SHADOWS = TRUE RECEIVE_SHADOWS { } NAME = \"a\" NAME = \"b\"").unwrap();
        assert!(n.receive_shadows);
        assert_eq!(n.pattern, "b");
        // A role key that is false, or empty, changes nothing.
        let n = node("WAYPOINT = TRUE BLOCKED = FALSE WALKPLANE { }").unwrap();
        assert_eq!(n.kind, NodeKind::Waypoint);
        // A nested NODE key is ignored; the keys match by prefix in the
        // table's order, so a longer word is no key.
        assert!(node("NODE { NAME = \"x\" }").is_ok());
        assert!(node("NAMES = \"x\"").is_err());
        assert!(node("NAME = \"x\" COLOR = 1").is_err());
    }

    #[test]
    fn a_node_without_a_name_matches_only_an_empty_name() {
        let patterns = NodePatterns::parse(b"GEOMETRY { NODE { WAYPOINT = TRUE } }").unwrap();
        assert_eq!(patterns.nodes[0].pattern, "");
        assert_eq!(patterns.classify("").unwrap().kind, NodeKind::Waypoint);
        assert_eq!(patterns.classify("wpt_1").unwrap().kind, NodeKind::Waypoint);
        assert!(patterns.classify("x").is_none());
    }
}
