use chizu_core::model::edge::EdgeKind;
use chizu_core::model::entity::EntityKind;
use petgraph::graph::{Graph, NodeIndex};

use std::collections::HashMap;

pub mod layout;
pub mod style;

use layout::{hierarchical_layout, radial_layout};
use style::{EdgeStyle, ScandinavianTheme};

#[derive(Debug, thiserror::Error)]
pub enum VisualizeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Graph error: {0}")]
    Graph(String),
    #[error("Layout error: {0}")]
    Layout(String),
}

pub type Result<T> = std::result::Result<T, VisualizeError>;

/// Configuration for graph visualization
#[derive(Debug, Clone)]
pub struct VisualizeConfig {
    pub layout: LayoutType,
    pub max_nodes: usize,
    pub include_legend: bool,
    pub width: f64,
    pub height: f64,
    pub node_spacing: f64,
    pub layer_spacing: f64,
}

impl Default for VisualizeConfig {
    fn default() -> Self {
        Self {
            layout: LayoutType::Hierarchical,
            max_nodes: 100,
            include_legend: true,
            width: 1200.0,
            height: 800.0,
            node_spacing: 150.0,
            layer_spacing: 100.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum LayoutType {
    Hierarchical,
    ForceDirected,
    Radial,
}

impl std::str::FromStr for LayoutType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hierarchical" => Ok(Self::Hierarchical),
            "force-directed" | "force_directed" | "force" => Ok(Self::ForceDirected),
            "radial" => Ok(Self::Radial),
            _ => Err(format!("Unknown layout type: {}", s)),
        }
    }
}

/// A node in the visualization graph
#[derive(Debug, Clone)]
pub struct VizNode {
    pub id: String,
    pub name: String,
    pub kind: EntityKind,
    pub component: Option<String>,
}

/// An edge in the visualization graph
#[derive(Debug, Clone)]
pub struct VizEdge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
}

/// SVG Generator implementing the Scandinavian design system
pub struct SvgGenerator {
    theme: ScandinavianTheme,
    config: VisualizeConfig,
}

impl SvgGenerator {
    pub fn new(config: VisualizeConfig) -> Self {
        Self {
            theme: ScandinavianTheme::default(),
            config,
        }
    }

    /// Generate an SVG from the given graph data
    pub fn generate(&self, nodes: Vec<VizNode>, edges: Vec<VizEdge>) -> Result<String> {
        if nodes.is_empty() {
            return Ok(self.generate_empty_svg());
        }

        // Build petgraph for layout computation
        let mut graph = Graph::<VizNode, VizEdge>::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();

        for node in &nodes {
            let idx = graph.add_node(node.clone());
            node_indices.insert(node.id.clone(), idx);
        }

        for edge in &edges {
            if let (Some(&src), Some(&dst)) = (
                node_indices.get(&edge.source),
                node_indices.get(&edge.target),
            ) {
                graph.add_edge(src, dst, edge.clone());
            }
        }

        // Compute layout
        let positions = match self.config.layout {
            LayoutType::Hierarchical => hierarchical_layout(&graph, &self.config),
            LayoutType::Radial => radial_layout(&graph, &self.config),
            _ => hierarchical_layout(&graph, &self.config),
        }?;

        // Generate SVG
        self.render_svg(&graph, &positions, nodes.len())
    }

    fn generate_empty_svg(&self) -> String {
        let width = self.config.width;
        let height = self.config.height;
        let bg = self.theme.canvas;
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">
  <rect width="100%" height="100%" fill="{bg}"/>
  <text x="50%" y="50%" text-anchor="middle" font-family="system-ui" font-size="14" fill="#64748b">No entities to visualize</text>
</svg>"##
        )
    }

    fn render_svg(
        &self,
        graph: &Graph<VizNode, VizEdge>,
        positions: &HashMap<NodeIndex, (f64, f64)>,
        node_count: usize,
    ) -> Result<String> {
        let _margin = 40.0;
        let legend_height = if self.config.include_legend {
            80.0
        } else {
            0.0
        };
        let _effective_height = self.config.height - legend_height;

        let mut svg = String::new();

        // SVG header
        let width = self.config.width;
        let height = self.config.height;
        let bg = self.theme.canvas;
        svg.push_str(&format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" style="background: {bg}">
  <defs>
    <marker id="arrowhead" markerWidth="10" markerHeight="7" refX="9" refY="3.5" orient="auto">
      <polygon points="0 0, 10 3.5, 0 7" fill="#94a3b8"/>
    </marker>
  </defs>
"##
        ));

        // Render edges first (so they appear behind nodes)
        for edge in graph.edge_indices() {
            let (src, dst) = graph.edge_endpoints(edge).unwrap();
            let edge_data = graph.edge_weight(edge).unwrap();

            if let (Some(&(x1, y1)), Some(&(x2, y2))) = (positions.get(&src), positions.get(&dst)) {
                let style = self.theme.edge_style(&edge_data.kind);
                svg.push_str(&self.render_edge(x1, y1, x2, y2, &style));
            }
        }

        // Render nodes
        for node_idx in graph.node_indices() {
            let node = &graph[node_idx];
            if let Some(&(x, y)) = positions.get(&node_idx) {
                svg.push_str(&self.render_node(x, y, node));
            }
        }

        // Render legend
        if self.config.include_legend {
            svg.push_str(&self.render_legend(node_count));
        }

        // Close SVG
        svg.push_str("</svg>");

        Ok(svg)
    }

    fn render_node(&self, x: f64, y: f64, node: &VizNode) -> String {
        let style = self.theme.entity_style(&node.kind);
        let node_width = 140.0;
        let node_height = 55.0;
        let rx = 6.0;

        // Truncate name if too long - show more chars
        let display_name = if node.name.len() > 22 {
            format!("{}...", &node.name[..19])
        } else {
            node.name.clone()
        };

        let kind_label = format!("{:?}", node.kind).to_lowercase();
        let hw = node_width / 2.0;
        let hh = node_height / 2.0;
        let fill = &style.fill;
        let stroke = &style.border;
        
        format!(
            r##"  <g transform="translate({x}, {y})">
    <rect x="-{hw}" y="-{hh}" width="{node_width}" height="{node_height}" rx="{rx}" 
          fill="{fill}" stroke="{stroke}" stroke-width="1.5"/>
    <text y="-8" text-anchor="middle" font-family="system-ui" font-size="12" font-weight="600" fill="#1e293b">{display_name}</text>
    <text y="14" text-anchor="middle" font-family="monospace" font-size="10" fill="{stroke}">{kind_label}</text>
  </g>
"##
        )
    }

    fn render_edge(&self, x1: f64, y1: f64, x2: f64, y2: f64, style: &EdgeStyle) -> String {
        let stroke_dasharray = match style.line_type {
            style::LineType::Solid => "none",
            style::LineType::Dashed => "5,5",
            style::LineType::Dotted => "2,2",
        };

        // Use bezier curves for smoother edges
        let _dx = (x2 - x1).abs();
        let dy = (y2 - y1).abs();
        
        // Control points for bezier curve
        let cp1x = x1;
        let cp1y = y1 + dy * 0.5;
        let cp2x = x2;
        let cp2y = y2 - dy * 0.5;
        
        // If nodes are on same level, use horizontal curve
        let (cp1x, cp1y, cp2x, cp2y) = if dy < 10.0 {
            let mid_x = (x1 + x2) / 2.0;
            (mid_x, y1 - 30.0, mid_x, y2 - 30.0)
        } else {
            (cp1x, cp1y, cp2x, cp2y)
        };

        let color = &style.color;
        let width = style.width;
        let dash = stroke_dasharray;
        format!(
            r##"  <path d="M {x1} {y1} C {cp1x} {cp1y}, {cp2x} {cp2y}, {x2} {y2}" 
        stroke="{color}" stroke-width="{width}" stroke-dasharray="{dash}" 
        fill="none" marker-end="url(#arrowhead)"/>
"##
        )
    }

    fn render_legend(&self, node_count: usize) -> String {
        let legend_y = self.config.height - 70.0;
        let items = vec![
            ("symbol", "#94a3b8"),
            ("test", "#22c55e"),
            ("source_unit", "#3b82f6"),
            ("doc", "#f59e0b"),
            ("infra_root", "#a855f7"),
            ("containerized", "#06b6d4"),
        ];

        let legend_w = self.config.width - 80.0;
        let mut svg = format!(
            r##"  <g transform="translate(40, {legend_y})">
    <rect x="-10" y="-10" width="{legend_w}" height="60" fill="#ffffff" stroke="#e2e8f0" rx="4"/>
    <text x="0" y="5" font-family="monospace" font-size="11" font-weight="bold" fill="#1e293b">Legend ({node_count} nodes)</text>
"##
        );

        let mut x = 0.0;
        for (label, color) in items {
            let cx = x + 10.0;
            let tx = x + 22.0;
            svg.push_str(&format!(
                r##"    <circle cx="{cx}" cy="30" r="6" fill="{color}" fill-opacity="0.1" stroke="{color}" stroke-width="1.5"/>
    <text x="{tx}" y="34" font-family="system-ui" font-size="9" fill="#64748b">{label}</text>
"##
            ));
            x += 100.0;
        }

        svg.push_str("  </g>\n");
        svg
    }
}

/// Generate an SVG visualization from graph data
pub fn generate_svg(
    nodes: Vec<VizNode>,
    edges: Vec<VizEdge>,
    config: VisualizeConfig,
) -> Result<String> {
    let generator = SvgGenerator::new(config);
    generator.generate(nodes, edges)
}
