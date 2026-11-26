use crate::trust::is_valid_id;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, ErrorKind};
use std::path::Path;

const EDGE_THRESHOLD: u32 = 2;
const MAX_NODES: usize = 64;
const MAX_EDGES: usize = 256;
const MAX_MONITOR_INDEX: u32 = 15;
const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub device_id: String,
    pub monitor_index: u32,
    pub width: u32,
    pub height: u32,
}

impl Node {
    pub fn monitor(device_id: &str, monitor_index: u32, width: u32, height: u32) -> Self {
        Self {
            device_id: device_id.to_string(),
            monitor_index,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Crossing {
    pub target: String,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl Layout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    pub fn set_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Replace the live topology. Callers must validate first so invalid data
    /// can never become the state used by the input hook.
    pub fn replace(&mut self, other: Layout) {
        self.nodes = other.nodes;
        self.edges = other.edges;
    }

    /// Reject layouts that cannot be represented safely: hostile IDs, zero or
    /// enormous dimensions, duplicate monitor slots, unknown/self edges and
    /// duplicate source edges on the same side.
    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.len() > MAX_NODES {
            return Err(format!("too many layout nodes (max {MAX_NODES})"));
        }
        if self.edges.len() > MAX_EDGES {
            return Err(format!("too many layout edges (max {MAX_EDGES})"));
        }

        let mut monitor_slots = HashSet::new();
        for node in &self.nodes {
            if !is_valid_id(&node.device_id) {
                return Err(format!("invalid device ID in layout: {}", node.device_id));
            }
            if node.monitor_index > MAX_MONITOR_INDEX {
                return Err(format!(
                    "monitor index out of range: {}",
                    node.monitor_index
                ));
            }
            if node.width == 0
                || node.height == 0
                || node.width > MAX_DIMENSION
                || node.height > MAX_DIMENSION
            {
                return Err(format!(
                    "invalid dimensions for {}: {}x{}",
                    node.device_id, node.width, node.height
                ));
            }
            if !monitor_slots.insert((node.device_id.clone(), node.monitor_index)) {
                return Err(format!(
                    "duplicate monitor slot: {}:{}",
                    node.device_id, node.monitor_index
                ));
            }
        }

        let node_ids: HashSet<&str> = self
            .nodes
            .iter()
            .map(|node| node.device_id.as_str())
            .collect();
        let mut source_sides = HashSet::new();
        for edge in &self.edges {
            if !is_valid_id(&edge.from) || !is_valid_id(&edge.to) {
                return Err(format!(
                    "invalid device ID in layout edge: {} -> {}",
                    edge.from, edge.to
                ));
            }
            if edge.from == edge.to {
                return Err(format!("self edge is not allowed: {}", edge.from));
            }
            if !node_ids.contains(edge.from.as_str()) {
                return Err(format!("layout edge source is unknown: {}", edge.from));
            }
            if !node_ids.contains(edge.to.as_str()) {
                return Err(format!("layout edge target is unknown: {}", edge.to));
            }
            if !source_sides.insert((edge.from.clone(), edge.side)) {
                return Err(format!(
                    "duplicate layout edge from {} on side {:?}",
                    edge.from, edge.side
                ));
            }
        }
        Ok(())
    }

    /// Missing file is an empty layout. Malformed or invalid files are errors;
    /// silently replacing them would hide a bad configuration from the user.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::new());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let layout: Self = serde_json::from_str(&raw)
            .map_err(|error| format!("invalid layout file {}: {error}", path.display()))?;
        layout.validate()?;
        Ok(layout)
    }

    /// Atomic save: a crash or a failed write leaves the previous layout file
    /// intact rather than a truncated configuration.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        self.validate()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let tmp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
            .map_err(|error| format!("could not encode layout: {error}"))?;
        std::fs::write(&tmp, json)
            .map_err(|error| format!("could not write {}: {error}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(())
    }

    pub fn cross(&self, from: &str, x: u32, y: u32) -> Option<Crossing> {
        let source = self.nodes.iter().find(|node| node.device_id == from)?;
        self.cross_from_node(source, x, y)
    }

    /// Resolve a crossing for one specific local monitor. The topology uses
    /// `(device_id, monitor_index)` as its unique node key, so production must
    /// not accidentally resolve another monitor of the same device.
    pub fn cross_from_index(
        &self,
        from: &str,
        monitor_index: u32,
        x: u32,
        y: u32,
    ) -> Option<Crossing> {
        let source = self.nodes.iter().find(|node| {
            node.device_id == from && node.monitor_index == monitor_index
        })?;
        self.cross_from_node(source, x, y)
    }

    fn cross_from_node(&self, source: &Node, x: u32, y: u32) -> Option<Crossing> {
        if x > source.width || y > source.height {
            return None;
        }
        let edge = self.edges.iter().find(|edge| {
            edge.from == source.device_id && self.side_at(source, edge.side, x, y)
        })?;
        let target = self.nodes.iter().find(|node| node.device_id == edge.to)?;

        let (target_x, target_y) = match edge.side {
            Side::Right => (0, scale(y, source.height, target.height)),
            Side::Left => (
                target.width.saturating_sub(1),
                scale(y, source.height, target.height),
            ),
            Side::Bottom => (scale(x, source.width, target.width), 0),
            Side::Top => (
                scale(x, source.width, target.width),
                target.height.saturating_sub(1),
            ),
        };

        Some(Crossing {
            target: target.device_id.clone(),
            x: target_x,
            y: target_y,
        })
    }

    fn side_at(&self, source: &Node, side: Side, x: u32, y: u32) -> bool {
        match side {
            Side::Left => x < EDGE_THRESHOLD,
            Side::Right => x.saturating_add(EDGE_THRESHOLD) > source.width,
            Side::Top => y < EDGE_THRESHOLD,
            Side::Bottom => y.saturating_add(EDGE_THRESHOLD) > source.height,
        }
    }
}

fn scale(value: u32, from_max: u32, to_max: u32) -> u32 {
    if from_max == 0 || to_max == 0 {
        return 0;
    }
    let scaled = (value as u64 * to_max as u64 / from_max as u64) as u32;
    scaled.min(to_max.saturating_sub(1))
}

pub fn escape_hotkey() -> &'static str {
    "Ctrl+Alt+Shift+M"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crosses_only_configured_edge() {
        let mut layout = Layout::new();
        layout.add_node(Node::monitor("pc-a", 0, 1920, 1080));
        layout.add_node(Node::monitor("pc-b", 1, 1920, 1080));
        layout.set_edge(Edge {
            from: "pc-a".into(),
            to: "pc-b".into(),
            side: Side::Right,
        });
        assert!(layout.cross("pc-a", 1919, 500).is_some());
        assert!(layout.cross("pc-a", 0, 500).is_none());
    }

    #[test]
    fn no_wrap_by_default() {
        let layout = Layout::new();
        assert!(layout.cross("pc-a", 0, 0).is_none());
    }

    #[test]
    fn layout_json_round_trip_uses_camel_case() {
        let json = r#"{
            "nodes": [
                { "deviceId": "pc-a", "monitorIndex": 0, "width": 1920, "height": 1080 },
                { "deviceId": "pc-b", "monitorIndex": 1, "width": 1280, "height": 720 }
            ],
            "edges": [
                { "from": "pc-a", "to": "pc-b", "side": "right" }
            ]
        }"#;
        let layout: Layout = serde_json::from_str(json).unwrap();
        assert_eq!(layout.nodes[0].device_id, "pc-a");
        assert_eq!(layout.nodes[1].monitor_index, 1);
        assert_eq!(layout.edges[0].side, Side::Right);

        let encoded = serde_json::to_string(&layout).unwrap();
        assert!(encoded.contains("\"deviceId\""), "{encoded}");
        assert!(encoded.contains("\"monitorIndex\""), "{encoded}");
        assert!(encoded.contains("\"side\":\"right\""), "{encoded}");
    }

    #[test]
    fn invalid_layouts_are_rejected() {
        fn valid_layout() -> Layout {
            let mut layout = Layout::new();
            layout.add_node(Node::monitor("pc-a", 0, 1920, 1080));
            layout.add_node(Node::monitor("pc-b", 0, 1920, 1080));
            layout.set_edge(Edge {
                from: "pc-a".into(),
                to: "pc-b".into(),
                side: Side::Right,
            });
            layout
        }

        assert!(valid_layout().validate().is_ok());

        let mut zero_dimension = valid_layout();
        zero_dimension.nodes[1].height = 0;
        assert!(zero_dimension.validate().is_err());

        let mut duplicate_node = valid_layout();
        duplicate_node.add_node(Node::monitor("pc-a", 0, 800, 600));
        assert!(duplicate_node.validate().is_err());

        let mut bad_id = valid_layout();
        bad_id.nodes[0].device_id = "bad id".into();
        assert!(bad_id.validate().is_err());

        let mut unknown_edge = valid_layout();
        unknown_edge.edges[0].to = "pc-c".into();
        assert!(unknown_edge.validate().is_err());

        let mut self_edge = valid_layout();
        self_edge.edges[0].to = "pc-a".into();
        assert!(self_edge.validate().is_err());

        let mut duplicate_edge = valid_layout();
        duplicate_edge.set_edge(Edge {
            from: "pc-a".into(),
            to: "pc-b".into(),
            side: Side::Right,
        });
        assert!(duplicate_edge.validate().is_err());
    }
}
