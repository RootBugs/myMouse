const EDGE_THRESHOLD: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crossing {
    pub target: String,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Default)]
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

    pub fn cross(&self, from: &str, x: u32, y: u32) -> Option<Crossing> {
        let source = self.nodes.iter().find(|node| node.device_id == from)?;
        let edge = self
            .edges
            .iter()
            .find(|edge| edge.from == from && self.side_at(source, edge.side, x, y))?;
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
            Side::Right => x + EDGE_THRESHOLD > source.width,
            Side::Top => y < EDGE_THRESHOLD,
            Side::Bottom => y + EDGE_THRESHOLD > source.height,
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
        layout.set_edge(Edge { from: "pc-a".into(), to: "pc-b".into(), side: Side::Right });
        assert!(layout.cross("pc-a", 1919, 500).is_some());
        assert!(layout.cross("pc-a", 0, 500).is_none());
    }

    #[test]
    fn no_wrap_by_default() {
        let layout = Layout::new();
        assert!(layout.cross("pc-a", 0, 0).is_none());
    }
}
