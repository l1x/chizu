use crate::{Result, VisualizeConfig};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

/// Layout engine for positioning nodes in the visualization
pub fn hierarchical_layout<N, E>(
    graph: &Graph<N, E>,
    config: &VisualizeConfig,
) -> Result<HashMap<NodeIndex, (f64, f64)>> {
    if graph.node_count() == 0 {
        return Ok(HashMap::new());
    }

    // Find root nodes (nodes with no incoming edges)
    let roots: Vec<NodeIndex> = graph
        .node_indices()
        .filter(|n| {
            graph
                .edges_directed(*n, Direction::Incoming)
                .next()
                .is_none()
        })
        .collect();

    // If no clear roots, use nodes with minimum in-degree
    let start_nodes = if roots.is_empty() {
        vec![graph.node_indices().next().unwrap()]
    } else {
        roots
    };

    // BFS to assign levels
    let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();

    for root in start_nodes {
        queue.push_back((root, 0));
        visited.insert(root);
    }

    while let Some((node, level)) = queue.pop_front() {
        levels.insert(node, level);

        for edge in graph.edges_directed(node, Direction::Outgoing) {
            let target = edge.target();
            if !visited.contains(&target) {
                visited.insert(target);
                queue.push_back((target, level + 1));
            }
        }
    }

    // Handle any unvisited nodes (disconnected components)
    for node in graph.node_indices() {
        if !levels.contains_key(&node) {
            levels.insert(node, 0);
        }
    }

    // Group nodes by level
    let max_level = *levels.values().max().unwrap_or(&0);
    let mut level_groups: Vec<Vec<NodeIndex>> = vec![Vec::new(); max_level + 1];

    for (node, level) in &levels {
        level_groups[*level].push(*node);
    }

    // Calculate positions
    let margin = 80.0;
    let available_width = config.width - 2.0 * margin;
    let available_height = if config.include_legend {
        config.height - 100.0 - 2.0 * margin
    } else {
        config.height - 2.0 * margin
    };

    let level_height = if max_level > 0 {
        available_height / max_level as f64
    } else {
        available_height
    };

    let mut positions: HashMap<NodeIndex, (f64, f64)> = HashMap::new();

    for (level, nodes) in level_groups.iter().enumerate() {
        let y = margin + level as f64 * level_height;
        let node_count = nodes.len();

        if node_count == 0 {
            continue;
        }

        let spacing = if node_count > 1 {
            available_width / (node_count - 1) as f64
        } else {
            available_width / 2.0
        };

        // Clamp spacing to prevent nodes from getting too close
        let spacing = spacing.max(config.node_spacing);

        // Recalculate with clamped spacing
        let total_width = (node_count - 1) as f64 * spacing;
        let start_x = (config.width - total_width) / 2.0;

        for (i, node) in nodes.iter().enumerate() {
            let x = start_x + i as f64 * spacing;
            positions.insert(*node, (x, y));
        }
    }

    Ok(positions)
}

/// Simple force-directed layout (simplified implementation)
pub fn force_directed_layout<N, E>(
    graph: &Graph<N, E>,
    config: &VisualizeConfig,
    iterations: usize,
) -> Result<HashMap<NodeIndex, (f64, f64)>> {
    let mut positions: HashMap<NodeIndex, (f64, f64)> = HashMap::new();
    let mut velocities: HashMap<NodeIndex, (f64, f64)> = HashMap::new();

    // Initialize with random positions
    let mut rng = simple_rng(42);
    for node in graph.node_indices() {
        let x = rng.next_f64() * config.width * 0.8 + config.width * 0.1;
        let y = rng.next_f64() * config.height * 0.8 + config.height * 0.1;
        positions.insert(node, (x, y));
        velocities.insert(node, (0.0, 0.0));
    }

    let repulsion_constant = 10000.0;
    let attraction_constant = 0.01;
    let damping = 0.9;
    let center_force = 0.05;

    for _ in 0..iterations {
        // Calculate repulsive forces
        let nodes: Vec<NodeIndex> = graph.node_indices().collect();
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let node_a = nodes[i];
                let node_b = nodes[j];

                let (x1, y1) = positions[&node_a];
                let (x2, y2) = positions[&node_b];

                let dx = x1 - x2;
                let dy = y1 - y2;
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                let force = repulsion_constant / dist_sq;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if let Some((vx, vy)) = velocities.get_mut(&node_a) {
                    *vx += fx;
                    *vy += fy;
                }
                if let Some((vx, vy)) = velocities.get_mut(&node_b) {
                    *vx -= fx;
                    *vy -= fy;
                }
            }
        }

        // Calculate attractive forces along edges
        for edge in graph.edge_indices() {
            let (source, target) = graph.edge_endpoints(edge).unwrap();
            let (x1, y1) = positions[&source];
            let (x2, y2) = positions[&target];

            let dx = x2 - x1;
            let dy = y2 - y1;
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);

            let force = attraction_constant * dist;
            let fx = (dx / dist) * force;
            let fy = (dy / dist) * force;

            if let Some((vx, vy)) = velocities.get_mut(&source) {
                *vx += fx;
                *vy += fy;
            }
            if let Some((vx, vy)) = velocities.get_mut(&target) {
                *vx -= fx;
                *vy -= fy;
            }
        }

        // Apply centering force and update positions
        let center_x = config.width / 2.0;
        let center_y = config.height / 2.0;

        for node in graph.node_indices() {
            let (x, y) = positions[&node];
            let (vx, vy) = velocities[&node];

            // Centering force
            let dx = center_x - x;
            let dy = center_y - y;

            let new_vx = (vx + dx * center_force) * damping;
            let new_vy = (vy + dy * center_force) * damping;

            let new_x = (x + new_vx).clamp(50.0, config.width - 50.0);
            let new_y = (y + new_vy).clamp(50.0, config.height - 50.0);

            positions.insert(node, (new_x, new_y));
            velocities.insert(node, (new_vx, new_vy));
        }
    }

    Ok(positions)
}

/// Simple deterministic RNG for consistent layouts
struct SimpleRng {
    state: u64,
}

fn simple_rng(seed: u64) -> SimpleRng {
    SimpleRng { state: seed }
}

impl SimpleRng {
    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }
}

/// Layout options for fine-tuning
#[derive(Debug, Clone)]
pub struct LayoutOptions {
    /// Minimum node spacing
    pub min_node_spacing: f64,
    /// Minimum level spacing (for hierarchical)
    pub min_level_spacing: f64,
    /// Number of iterations for force-directed
    pub iterations: usize,
    /// Whether to center the layout
    pub center: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            min_node_spacing: 100.0,
            min_level_spacing: 80.0,
            iterations: 100,
            center: true,
        }
    }
}
