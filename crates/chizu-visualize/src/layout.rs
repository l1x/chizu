use crate::{Result, VisualizeConfig};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

/// Layout engine for positioning nodes in the visualization
/// Uses a wrapped hierarchical layout where wide levels are split into multiple rows
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

    // Calculate layout parameters
    let node_width = 160.0;
    let node_height = 60.0;
    let margin_x = 60.0;
    let margin_y = 80.0;
    let row_height = node_height + 60.0; // Vertical space between rows
    
    let available_width = config.width - 2.0 * margin_x;
    
    // Calculate max nodes per row based on available width
    let max_per_row = ((available_width / (node_width + 20.0)).floor() as usize).max(1);

    // Group nodes by level
    let max_level = *levels.values().max().unwrap_or(&0);
    let mut level_groups: Vec<Vec<NodeIndex>> = vec![Vec::new(); max_level + 1];

    for (node, level) in &levels {
        level_groups[*level].push(*node);
    }

    let mut positions: HashMap<NodeIndex, (f64, f64)> = HashMap::new();
    let mut current_y = margin_y;

    // Place each level, wrapping to multiple rows if needed
    for level_nodes in level_groups.iter() {
        if level_nodes.is_empty() {
            continue;
        }

        // Split into rows
        let chunks: Vec<Vec<NodeIndex>> = level_nodes
            .chunks(max_per_row)
            .map(|c| c.to_vec())
            .collect();

        for (row_idx, row_nodes) in chunks.iter().enumerate() {
            let row_y = current_y + row_idx as f64 * row_height;
            let node_count = row_nodes.len();
            
            // Center this row
            let total_width = node_count as f64 * node_width + (node_count - 1) as f64 * 40.0;
            let start_x = margin_x + (available_width - total_width) / 2.0 + node_width / 2.0;

            for (i, node) in row_nodes.iter().enumerate() {
                let x = start_x + i as f64 * (node_width + 40.0);
                positions.insert(*node, (x, row_y));
            }
        }

        // Move to next level position (account for wrapped rows)
        let rows_in_this_level = ((level_nodes.len() - 1) / max_per_row) + 1;
        current_y += rows_in_this_level as f64 * row_height + 40.0;
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
