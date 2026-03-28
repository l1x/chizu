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

    // Calculate positions with better spacing
    let margin_x = 100.0;
    let margin_y = 80.0;
    let node_width = 140.0;
    let _node_height = 60.0;
    
    let available_width = config.width - 2.0 * margin_x;
    let available_height = if config.include_legend {
        config.height - 100.0 - 2.0 * margin_y
    } else {
        config.height - 2.0 * margin_y
    };

    let level_height = if max_level > 0 {
        available_height / max_level as f64
    } else {
        available_height
    };

    let mut positions: HashMap<NodeIndex, (f64, f64)> = HashMap::new();

    for (level, nodes) in level_groups.iter().enumerate() {
        let y = margin_y + level as f64 * level_height;
        let node_count = nodes.len();

        if node_count == 0 {
            continue;
        }

        // Calculate spacing to fit all nodes with minimum gap
        let min_spacing = node_width + 40.0;
        let total_width_needed = node_count as f64 * min_spacing;
        
        let spacing = if total_width_needed > available_width {
            // Too many nodes - compress but keep minimum
            (available_width - node_width) / (node_count - 1).max(1) as f64
        } else {
            // Center the group
            min_spacing
        };

        let total_width = (node_count - 1) as f64 * spacing + node_width;
        let start_x = margin_x + (available_width - total_width) / 2.0 + node_width / 2.0;

        for (i, node) in nodes.iter().enumerate() {
            let x = start_x + i as f64 * spacing;
            positions.insert(*node, (x, y));
        }
    }

    Ok(positions)
}

/// Radial layout - places nodes in concentric circles
pub fn radial_layout<N, E>(
    graph: &Graph<N, E>,
    config: &VisualizeConfig,
) -> Result<HashMap<NodeIndex, (f64, f64)>> {
    if graph.node_count() == 0 {
        return Ok(HashMap::new());
    }

    let center_x = config.width / 2.0;
    let center_y = config.height / 2.0;
    let max_radius = (config.width.min(config.height) / 2.0) - 100.0;

    let mut positions: HashMap<NodeIndex, (f64, f64)> = HashMap::new();
    let count = graph.node_count();
    
    // Simple radial layout - distribute evenly in a circle
    for (i, node) in graph.node_indices().enumerate() {
        let angle = 2.0 * std::f64::consts::PI * (i as f64) / (count as f64);
        let radius = max_radius * (0.3 + 0.7 * ((i % 3) as f64) / 2.0);
        
        let x = center_x + radius * angle.cos();
        let y = center_y + radius * angle.sin();
        
        positions.insert(node, (x, y));
    }

    Ok(positions)
}
