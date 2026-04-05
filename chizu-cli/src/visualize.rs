use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;
use std::path::Path;

use chizu_core::{EdgeKind, Entity, EntityKind, Summary};

const HORIZONTAL_GAP: f64 = 122.0;
const ORPHAN_GAP: f64 = 102.0;
const SIBLING_GAP_UNITS: f64 = 0.36;
const COMPACT_CHILD_GAP_UNITS: f64 = 0.18;
const COMPACT_ROW_GAP: f64 = 46.0;

#[derive(Clone)]
struct VisualEdge {
    src_id: String,
    rel: EdgeKind,
    dst_id: String,
}

#[derive(Clone, Copy)]
struct KindPalette {
    core: &'static str,
    glow: &'static str,
    ring: &'static str,
}

#[derive(Clone)]
struct PositionedNode {
    id: String,
    name: String,
    kind: EntityKind,
    depth: u32,
    degree: usize,
    x: f64,
    y: f64,
    radius: f64,
    is_focus: bool,
    is_layout_root: bool,
    palette: KindPalette,
}

#[derive(Clone)]
struct LabelPlacement {
    title: String,
    subtitle: String,
    x: f64,
    y: f64,
    anchor: &'static str,
    prominent: bool,
}

pub fn render_focus_graph_svg(
    entity_cache: &HashMap<String, Entity>,
    visited_edges: &HashSet<(String, EdgeKind, String)>,
    requested_focus_id: Option<&str>,
) -> String {
    let mut edges: Vec<_> = visited_edges
        .iter()
        .map(|(src_id, rel, dst_id)| VisualEdge {
            src_id: src_id.clone(),
            rel: *rel,
            dst_id: dst_id.clone(),
        })
        .filter(|edge| {
            entity_cache.contains_key(&edge.src_id) && entity_cache.contains_key(&edge.dst_id)
        })
        .collect();
    edges.sort_by(|a, b| (&a.src_id, &a.rel, &a.dst_id).cmp(&(&b.src_id, &b.rel, &b.dst_id)));

    let focus_id = choose_focus_entity(entity_cache, &edges, requested_focus_id);
    let overview_mode = focus_id == "repo::.";
    let (working_entities, edges) = if overview_mode {
        simplify_repo_overview(entity_cache, edges)
    } else {
        (entity_cache.clone(), edges)
    };
    let layout_root_id = choose_layout_root(&working_entities, &focus_id);
    let mut nodes = position_nodes(
        &working_entities,
        &edges,
        &focus_id,
        &layout_root_id,
        overview_mode,
    );

    let (shift_x, shift_y, width, height) = scene_bounds(&nodes);
    for node in &mut nodes {
        node.x += shift_x;
        node.y += shift_y;
    }

    let node_map: HashMap<_, _> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.clone()))
        .collect();

    let focus_node = node_map
        .get(&focus_id)
        .expect("focus node should be part of the rendered graph");
    let layout_root = node_map
        .get(&layout_root_id)
        .expect("layout root should be part of the rendered graph");
    let focus_edge_count = edges
        .iter()
        .filter(|edge| edge.src_id == focus_id || edge.dst_id == focus_id)
        .count();

    let mut out = String::new();
    let svg_width = width.ceil() as usize;
    let svg_height = height.ceil() as usize;

    writeln!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<svg width="{svg_width}" height="{svg_height}" viewBox="0 0 {svg_width} {svg_height}" xmlns="http://www.w3.org/2000/svg">"#
    )
    .unwrap();
    if overview_mode {
        out.push_str(
            r##"<defs>
  <linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
    <stop offset="0%" stop-color="#f6f4ee" />
    <stop offset="100%" stop-color="#efeee8" />
  </linearGradient>
  <radialGradient id="canopy-glow" cx="50%" cy="16%" r="62%">
    <stop offset="0%" stop-color="#dfe8e7" stop-opacity="0.58" />
    <stop offset="55%" stop-color="#f2f0ea" stop-opacity="0.12" />
    <stop offset="100%" stop-color="#f6f4ee" stop-opacity="0" />
  </radialGradient>
  <pattern id="grid-pattern" width="128" height="128" patternUnits="userSpaceOnUse">
    <path d="M 0 127 L 127 0" stroke="#d7d9d5" stroke-opacity="0.28" stroke-width="1"/>
  </pattern>
  <filter id="soft-glow" x="-200%" y="-200%" width="400%" height="400%">
    <feGaussianBlur stdDeviation="8"/>
  </filter>
  <filter id="edge-glow" x="-200%" y="-200%" width="400%" height="400%">
    <feGaussianBlur stdDeviation="3"/>
  </filter>
</defs>"##,
        );
        out.push_str(
            r#"<style>
svg {
  background: #f6f4ee;
  font-family: "SF Pro Display", "Segoe UI", Helvetica, Arial, sans-serif;
}
.hud-card {
  fill: rgba(255, 255, 255, 0.92);
  stroke: rgba(127, 149, 149, 0.48);
  stroke-width: 1.1;
}
.hud-title,
.node-title,
.edge-label {
  fill: #18313a;
}
.hud-title {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 0.01em;
}
.hud-subtitle,
.node-kind {
  fill: #61757b;
}
.hud-subtitle {
  font-size: 13px;
}
.node-title {
  font-size: 16px;
  font-weight: 600;
}
.node-title.compact {
  font-size: 11px;
}
.node-kind {
  font-size: 10px;
}
.edge-label {
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.03em;
  fill: #4b6769;
}
</style>"#,
        );
    } else {
        out.push_str(
            r##"<defs>
  <linearGradient id="bg-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
    <stop offset="0%" stop-color="#060c15" />
    <stop offset="42%" stop-color="#0a1322" />
    <stop offset="100%" stop-color="#08131d" />
  </linearGradient>
  <radialGradient id="canopy-glow" cx="50%" cy="16%" r="62%">
    <stop offset="0%" stop-color="#1b4e6b" stop-opacity="0.54" />
    <stop offset="48%" stop-color="#0d2031" stop-opacity="0.18" />
    <stop offset="100%" stop-color="#050911" stop-opacity="0" />
  </radialGradient>
  <pattern id="grid-pattern" width="96" height="96" patternUnits="userSpaceOnUse">
    <path d="M 0 95 L 95 0" stroke="#123147" stroke-opacity="0.22" stroke-width="1"/>
    <path d="M 0 63 L 63 0" stroke="#123147" stroke-opacity="0.15" stroke-width="1"/>
  </pattern>
  <filter id="soft-glow" x="-200%" y="-200%" width="400%" height="400%">
    <feGaussianBlur stdDeviation="8"/>
  </filter>
  <filter id="edge-glow" x="-200%" y="-200%" width="400%" height="400%">
    <feGaussianBlur stdDeviation="3"/>
  </filter>
</defs>"##,
        );
        out.push_str(
            r#"<style>
svg {
  background: #050911;
  font-family: "SF Pro Display", "Segoe UI", Helvetica, Arial, sans-serif;
}
.hud-card {
  fill: rgba(7, 14, 24, 0.90);
  stroke: rgba(111, 211, 255, 0.34);
  stroke-width: 1.2;
}
.hud-title,
.node-title,
.edge-label {
  fill: #f6fdff;
  paint-order: stroke fill;
  stroke: rgba(5, 9, 17, 0.68);
  stroke-width: 2.2px;
  stroke-linejoin: round;
}
.hud-title {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 0.01em;
}
.hud-subtitle,
.node-kind {
  fill: #b7cfdf;
  paint-order: stroke fill;
  stroke: rgba(5, 9, 17, 0.62);
  stroke-width: 1.7px;
  stroke-linejoin: round;
}
.hud-subtitle {
  font-size: 13px;
}
.node-title {
  font-size: 18px;
  font-weight: 600;
}
.node-title.compact {
  font-size: 15px;
}
.node-kind {
  font-size: 12px;
}
.edge-label {
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.03em;
  fill: #b6ffe8;
}
</style>"#,
        );
    }
    writeln!(
        out,
        r#"<rect width="{svg_width}" height="{svg_height}" fill="url(#bg-gradient)"/>"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<rect width="{svg_width}" height="{svg_height}" fill="url(#grid-pattern)" opacity="0.28"/>"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="url(#canopy-glow)" opacity="0.96"/>"#,
        layout_root.x,
        layout_root.y + height * 0.06,
        width.max(height) * 0.36
    )
    .unwrap();

    render_hud(&mut out, focus_node, nodes.len(), edges.len());

    out.push_str(r#"<g id="graph">"#);
    render_atmospheric_rays(&mut out, &nodes, layout_root, overview_mode);
    render_edges(
        &mut out,
        &edges,
        &node_map,
        &focus_id,
        focus_edge_count,
        overview_mode,
    );
    render_nodes(&mut out, &nodes, overview_mode);
    render_labels(&mut out, &nodes);
    out.push_str("</g>");
    out.push_str(pan_zoom_script());
    out.push_str("</svg>");

    out
}

pub fn render_focus_graph_html(
    entity_cache: &HashMap<String, Entity>,
    summary_cache: &HashMap<String, Summary>,
    visited_edges: &HashSet<(String, EdgeKind, String)>,
    repo_root: &Path,
    editor_link_template: Option<&str>,
    requested_focus_id: Option<&str>,
) -> String {
    let mut edges: Vec<_> = visited_edges
        .iter()
        .map(|(src_id, rel, dst_id)| VisualEdge {
            src_id: src_id.clone(),
            rel: *rel,
            dst_id: dst_id.clone(),
        })
        .filter(|edge| {
            entity_cache.contains_key(&edge.src_id) && entity_cache.contains_key(&edge.dst_id)
        })
        .collect();
    edges.sort_by(|a, b| (&a.src_id, &a.rel, &a.dst_id).cmp(&(&b.src_id, &b.rel, &b.dst_id)));

    let focus_id = choose_focus_entity(entity_cache, &edges, requested_focus_id);
    let home_id = requested_focus_id
        .filter(|id| entity_cache.contains_key(*id))
        .map(str::to_string)
        .or_else(|| {
            entity_cache
                .contains_key("repo::.")
                .then(|| "repo::.".to_string())
        })
        .unwrap_or_else(|| focus_id.clone());

    let mut entities: Vec<_> = entity_cache.values().collect();
    entities.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = serde_json::json!({
        "focus_id": focus_id,
        "home_id": home_id,
        "node_count": entity_cache.len(),
        "edge_count": edges.len(),
        "nodes": entities
            .into_iter()
            .map(|entity| {
                let summary = summary_cache.get(&entity.id);
                serde_json::json!({
                    "id": entity.id,
                    "name": entity.name,
                    "display_name": display_name(entity),
                    "kind": entity.kind.to_string(),
                    "path": entity.path,
                    "component_id": entity.component_id.as_ref().map(ToString::to_string),
                    "language": entity.language,
                    "line_start": entity.line_start,
                    "line_end": entity.line_end,
                    "visibility": entity.visibility.map(|visibility| visibility.to_string()),
                    "exported": entity.exported,
                    "summary_short": summary.map(|summary| summary.short_summary.as_str()),
                    "summary_detailed": summary.and_then(|summary| summary.detailed_summary.as_deref()),
                    "editor_url": editor_link_for_entity(repo_root, entity, editor_link_template),
                })
            })
            .collect::<Vec<_>>(),
        "edges": edges
            .iter()
            .map(|edge| {
                serde_json::json!({
                    "src_id": edge.src_id,
                    "rel": edge.rel,
                    "dst_id": edge.dst_id,
                })
            })
            .collect::<Vec<_>>(),
    });
    let payload_json = escape_json_for_html(
        &serde_json::to_string(&payload).expect("interactive graph payload should serialize"),
    );

    let template_source = include_str!("../templates/explorer.html.j2");
    let mut env = minijinja::Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.add_template("explorer", template_source)
        .expect("explorer template should parse");
    let tmpl = env.get_template("explorer").unwrap();
    tmpl.render(minijinja::context! { payload_json => payload_json })
        .expect("explorer template should render")
}

fn simplify_repo_overview(
    entity_cache: &HashMap<String, Entity>,
    edges: Vec<VisualEdge>,
) -> (HashMap<String, Entity>, Vec<VisualEdge>) {
    let filtered_entities: HashMap<_, _> = entity_cache
        .iter()
        .filter(|(_, entity)| {
            matches!(
                entity.kind,
                EntityKind::Repo | EntityKind::Component | EntityKind::Doc | EntityKind::SourceUnit
            )
        })
        .map(|(id, entity)| (id.clone(), entity.clone()))
        .collect();

    let filtered_edges = edges
        .into_iter()
        .filter(|edge| {
            filtered_entities.contains_key(&edge.src_id)
                && filtered_entities.contains_key(&edge.dst_id)
                && matches!(edge.rel, EdgeKind::Contains | EdgeKind::DocumentedBy)
        })
        .collect();

    (filtered_entities, filtered_edges)
}

fn choose_focus_entity(
    entity_cache: &HashMap<String, Entity>,
    edges: &[VisualEdge],
    requested_focus_id: Option<&str>,
) -> String {
    if let Some(requested) = requested_focus_id
        && entity_cache.contains_key(requested)
    {
        return requested.to_string();
    }
    if entity_cache.contains_key("repo::.") {
        return "repo::.".to_string();
    }

    let mut degrees: HashMap<String, usize> = HashMap::new();
    for edge in edges {
        *degrees.entry(edge.src_id.clone()).or_insert(0) += 1;
        *degrees.entry(edge.dst_id.clone()).or_insert(0) += 1;
    }

    entity_cache
        .values()
        .max_by(|a, b| {
            focus_rank(a, *degrees.get(&a.id).unwrap_or(&0))
                .cmp(&focus_rank(b, *degrees.get(&b.id).unwrap_or(&0)))
        })
        .map(|entity| entity.id.clone())
        .unwrap_or_else(|| {
            entity_cache
                .keys()
                .next()
                .expect("at least one entity should exist")
                .clone()
        })
}

fn choose_layout_root(entity_cache: &HashMap<String, Entity>, focus_id: &str) -> String {
    if entity_cache.contains_key("repo::.") {
        "repo::.".to_string()
    } else {
        focus_id.to_string()
    }
}

fn focus_rank(entity: &Entity, degree: usize) -> (usize, usize, usize, &str) {
    let kind_priority = match entity.kind {
        EntityKind::Repo => 5,
        EntityKind::Component => 4,
        EntityKind::SourceUnit => 3,
        EntityKind::Symbol => 2,
        _ => 1,
    };
    (
        kind_priority,
        degree,
        entity.exported as usize,
        entity.id.as_str(),
    )
}

fn position_nodes(
    entity_cache: &HashMap<String, Entity>,
    edges: &[VisualEdge],
    focus_id: &str,
    layout_root_id: &str,
    overview_mode: bool,
) -> Vec<PositionedNode> {
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for entity_id in entity_cache.keys() {
        adjacency.entry(entity_id.clone()).or_default();
    }
    for edge in edges {
        adjacency
            .entry(edge.src_id.clone())
            .or_default()
            .push(edge.dst_id.clone());
        adjacency
            .entry(edge.dst_id.clone())
            .or_default()
            .push(edge.src_id.clone());
    }
    for neighbors in adjacency.values_mut() {
        neighbors.sort();
        neighbors.dedup();
    }

    let degree_by_id: HashMap<String, usize> = adjacency
        .iter()
        .map(|(id, neighbors)| (id.clone(), neighbors.len()))
        .collect();

    let mut depth_by_id = HashMap::new();
    let mut parent_by_id = HashMap::new();
    let mut queue = VecDeque::new();

    depth_by_id.insert(layout_root_id.to_string(), 0_u32);
    queue.push_back(layout_root_id.to_string());

    while let Some(node_id) = queue.pop_front() {
        let depth = depth_by_id[&node_id];
        let mut neighbors = adjacency.get(&node_id).cloned().unwrap_or_default();
        neighbors.sort_by(|a, b| {
            node_sort_key(a, entity_cache, &degree_by_id).cmp(&node_sort_key(
                b,
                entity_cache,
                &degree_by_id,
            ))
        });

        for neighbor_id in neighbors {
            if depth_by_id.contains_key(&neighbor_id) {
                continue;
            }
            depth_by_id.insert(neighbor_id.clone(), depth + 1);
            parent_by_id.insert(neighbor_id.clone(), node_id.clone());
            queue.push_back(neighbor_id);
        }
    }

    let mut children_by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for (child_id, parent_id) in &parent_by_id {
        children_by_parent
            .entry(parent_id.clone())
            .or_default()
            .push(child_id.clone());
    }
    for children in children_by_parent.values_mut() {
        children.sort_by(|a, b| {
            node_sort_key(a, entity_cache, &degree_by_id).cmp(&node_sort_key(
                b,
                entity_cache,
                &degree_by_id,
            ))
        });
    }

    let mut max_depth = depth_by_id.values().copied().max().unwrap_or(0);
    let orphan_depth = max_depth + 1;
    let mut orphans: Vec<_> = entity_cache
        .keys()
        .filter(|id| !depth_by_id.contains_key(*id))
        .cloned()
        .collect();
    orphans.sort_by(|a, b| {
        node_sort_key(a, entity_cache, &degree_by_id).cmp(&node_sort_key(
            b,
            entity_cache,
            &degree_by_id,
        ))
    });
    for orphan_id in &orphans {
        depth_by_id.insert(orphan_id.clone(), orphan_depth);
    }
    if !orphans.is_empty() {
        max_depth = orphan_depth;
    }

    let compact_parents = collect_compact_parents(&children_by_parent, &depth_by_id, overview_mode);

    let mut subtree_widths = HashMap::new();
    let root_width_units = subtree_width_units(
        &children_by_parent,
        layout_root_id,
        &compact_parents,
        &mut subtree_widths,
    );

    let mut x_units_by_id = HashMap::new();
    let mut y_by_id = HashMap::new();
    let mut layout_ctx = TreeLayoutContext {
        max_depth,
        children_by_parent: &children_by_parent,
        compact_parents: &compact_parents,
        subtree_widths: &subtree_widths,
        depth_by_id: &depth_by_id,
        x_by_id: &mut x_units_by_id,
        y_by_id: &mut y_by_id,
    };
    assign_tree_positions(&mut layout_ctx, layout_root_id, 0.0, root_width_units);

    let mut orphan_left = root_width_units + 1.4;
    for orphan_id in &orphans {
        x_units_by_id.insert(orphan_id.clone(), orphan_left);
        y_by_id.insert(
            orphan_id.clone(),
            vertical_position(orphan_depth, max_depth) + 36.0,
        );
        orphan_left += ORPHAN_GAP / HORIZONTAL_GAP;
    }

    let root_x_units = x_units_by_id.get(layout_root_id).copied().unwrap_or(0.0);
    let mut positioned = Vec::with_capacity(entity_cache.len());

    for entity_id in entity_cache.keys() {
        let entity = entity_cache
            .get(entity_id)
            .expect("entity id should resolve from cache");
        let depth = *depth_by_id.get(entity_id).unwrap_or(&orphan_depth);
        let x_units = x_units_by_id.get(entity_id).copied().unwrap_or(0.0) - root_x_units;
        let x = x_units * HORIZONTAL_GAP;
        let y = y_by_id
            .get(entity_id)
            .copied()
            .unwrap_or_else(|| vertical_position(depth, max_depth));
        let degree = *degree_by_id.get(entity_id).unwrap_or(&0);
        let is_focus = entity_id == focus_id;
        let is_layout_root = entity_id == layout_root_id;

        positioned.push(PositionedNode {
            id: entity_id.clone(),
            name: display_name(entity),
            kind: entity.kind,
            depth,
            degree,
            x,
            y,
            radius: node_radius(depth, degree, is_focus, is_layout_root),
            is_focus,
            is_layout_root,
            palette: palette_for(entity.kind, overview_mode),
        });
    }

    positioned.sort_by(|a, b| {
        b.depth
            .cmp(&a.depth)
            .then_with(|| a.degree.cmp(&b.degree))
            .then_with(|| a.name.cmp(&b.name))
    });
    positioned
}

fn collect_compact_parents(
    children_by_parent: &HashMap<String, Vec<String>>,
    depth_by_id: &HashMap<String, u32>,
    overview_mode: bool,
) -> HashSet<String> {
    children_by_parent
        .iter()
        .filter_map(|(parent_id, children)| {
            let depth = *depth_by_id.get(parent_id).unwrap_or(&0);
            let min_depth = if overview_mode { 1 } else { 2 };
            let should_compact = depth >= min_depth
                && !children.is_empty()
                && children.iter().all(|child_id| {
                    children_by_parent
                        .get(child_id)
                        .map(|grandchildren| grandchildren.is_empty())
                        .unwrap_or(true)
                });
            should_compact.then(|| parent_id.clone())
        })
        .collect()
}

fn subtree_width_units(
    children_by_parent: &HashMap<String, Vec<String>>,
    node_id: &str,
    compact_parents: &HashSet<String>,
    memo: &mut HashMap<String, f64>,
) -> f64 {
    if let Some(width) = memo.get(node_id) {
        return *width;
    }

    let width = match children_by_parent.get(node_id) {
        None => 1.0,
        Some(children) if children.is_empty() => 1.0,
        Some(children) if compact_parents.contains(node_id) => {
            compact_cluster_width_units(children.len())
        }
        Some(children) => {
            let mut total = 0.0;
            for (index, child_id) in children.iter().enumerate() {
                if index > 0 {
                    total += SIBLING_GAP_UNITS;
                }
                total += subtree_width_units(children_by_parent, child_id, compact_parents, memo);
            }
            total.max(1.0)
        }
    };

    memo.insert(node_id.to_string(), width);
    width
}

struct TreeLayoutContext<'a> {
    max_depth: u32,
    children_by_parent: &'a HashMap<String, Vec<String>>,
    compact_parents: &'a HashSet<String>,
    subtree_widths: &'a HashMap<String, f64>,
    depth_by_id: &'a HashMap<String, u32>,
    x_by_id: &'a mut HashMap<String, f64>,
    y_by_id: &'a mut HashMap<String, f64>,
}

fn assign_tree_positions(ctx: &mut TreeLayoutContext, node_id: &str, left: f64, width: f64) {
    let depth = *ctx.depth_by_id.get(node_id).unwrap_or(&0);
    let center = left + width * 0.5;
    ctx.x_by_id.insert(node_id.to_string(), center);
    ctx.y_by_id
        .insert(node_id.to_string(), vertical_position(depth, ctx.max_depth));

    let Some(children) = ctx.children_by_parent.get(node_id) else {
        return;
    };
    if children.is_empty() {
        return;
    }

    if ctx.compact_parents.contains(node_id) {
        let children = children.clone();
        position_compact_children(
            node_id,
            center,
            &children,
            ctx.max_depth,
            depth,
            ctx.x_by_id,
            ctx.y_by_id,
        );
        return;
    }

    let children: Vec<_> = children.clone();
    let mut cursor = left;
    for child_id in &children {
        let child_width = ctx.subtree_widths.get(child_id).copied().unwrap_or(1.0);
        assign_tree_positions(ctx, child_id, cursor, child_width);
        cursor += child_width + SIBLING_GAP_UNITS;
    }
}

fn position_compact_children(
    _parent_id: &str,
    center_x_units: f64,
    children: &[String],
    max_depth: u32,
    parent_depth: u32,
    x_by_id: &mut HashMap<String, f64>,
    y_by_id: &mut HashMap<String, f64>,
) {
    let columns = compact_columns(children.len());
    let base_y = vertical_position(parent_depth + 1, max_depth);

    for (index, child_id) in children.iter().enumerate() {
        let row = index / columns;
        let row_index = index % columns;
        let row_count = (children.len() - row * columns).min(columns);
        let row_center = (row_count as f64 - 1.0) * 0.5;
        let offset = row_index as f64 - row_center;

        let x_units = center_x_units + offset * (COMPACT_CHILD_GAP_UNITS + row as f64 * 0.02);
        let y = base_y + row as f64 * COMPACT_ROW_GAP + offset.abs() * 6.0;

        x_by_id.insert(child_id.clone(), x_units);
        y_by_id.insert(child_id.clone(), y);
    }
}

fn compact_columns(child_count: usize) -> usize {
    child_count.clamp(1, 6)
}

fn compact_cluster_width_units(child_count: usize) -> f64 {
    let columns = compact_columns(child_count);
    1.2 + (columns.saturating_sub(1) as f64 * 0.24)
}

fn node_sort_key<'a>(
    entity_id: &'a str,
    entity_cache: &'a HashMap<String, Entity>,
    degree_by_id: &'a HashMap<String, usize>,
) -> (std::cmp::Reverse<usize>, std::cmp::Reverse<usize>, &'a str) {
    let entity = entity_cache
        .get(entity_id)
        .expect("entity id should resolve from cache");
    (
        std::cmp::Reverse(kind_priority(entity.kind)),
        std::cmp::Reverse(*degree_by_id.get(entity_id).unwrap_or(&0)),
        entity.name.as_str(),
    )
}

fn kind_priority(kind: EntityKind) -> usize {
    match kind {
        EntityKind::Repo => 6,
        EntityKind::Component => 5,
        EntityKind::SourceUnit => 4,
        EntityKind::Symbol => 3,
        EntityKind::Doc => 2,
        _ => 1,
    }
}

fn vertical_position(depth: u32, max_depth: u32) -> f64 {
    let y = match depth {
        0 => 0.0,
        1 => 176.0,
        2 => 338.0,
        3 => 492.0,
        _ => 492.0 + (depth.saturating_sub(3) as f64 * 146.0),
    };

    if depth == max_depth && max_depth >= 4 {
        y + 24.0
    } else {
        y
    }
}

fn node_radius(depth: u32, degree: usize, is_focus: bool, is_layout_root: bool) -> f64 {
    if is_layout_root && is_focus {
        return 22.0;
    }
    if is_layout_root {
        return 18.0;
    }
    if is_focus {
        return 17.0;
    }

    match depth {
        0 => 16.0,
        1 => (10.5 + degree.min(8) as f64 * 1.1).clamp(11.0, 18.0),
        2 => (6.0 + degree.min(6) as f64 * 0.8).clamp(6.0, 11.0),
        3 => {
            if degree <= 1 {
                4.8
            } else {
                6.2
            }
        }
        _ => {
            if degree <= 1 {
                3.8
            } else {
                4.8
            }
        }
    }
}

fn palette_for(kind: EntityKind, overview_mode: bool) -> KindPalette {
    if overview_mode {
        return match kind {
            EntityKind::Repo | EntityKind::Component => KindPalette {
                core: "#55757c",
                glow: "#9db7bb",
                ring: "#d7e3e5",
            },
            EntityKind::SourceUnit | EntityKind::Symbol | EntityKind::Test | EntityKind::Bench => {
                KindPalette {
                    core: "#708b93",
                    glow: "#bfd0d4",
                    ring: "#dfe7e8",
                }
            }
            EntityKind::Doc | EntityKind::AgentConfig | EntityKind::Spec | EntityKind::Workflow => {
                KindPalette {
                    core: "#7f958d",
                    glow: "#c8d7d0",
                    ring: "#e4ece7",
                }
            }
            EntityKind::Task
            | EntityKind::Feature
            | EntityKind::Command
            | EntityKind::Migration => KindPalette {
                core: "#9a8d76",
                glow: "#d8cfbf",
                ring: "#ece6dc",
            },
            EntityKind::Site
            | EntityKind::Template
            | EntityKind::ContentPage
            | EntityKind::Directory => KindPalette {
                core: "#809a8b",
                glow: "#c7d8ce",
                ring: "#e4ebe7",
            },
            EntityKind::Containerized | EntityKind::InfraRoot => KindPalette {
                core: "#8b9980",
                glow: "#d1d8c7",
                ring: "#e7ebe1",
            },
        };
    }

    match kind {
        EntityKind::Repo | EntityKind::Component => KindPalette {
            core: "#72f6ff",
            glow: "#2ed1ff",
            ring: "#123d57",
        },
        EntityKind::SourceUnit | EntityKind::Symbol | EntityKind::Test | EntityKind::Bench => {
            KindPalette {
                core: "#78b7ff",
                glow: "#4388ff",
                ring: "#162d64",
            }
        }
        EntityKind::Doc | EntityKind::AgentConfig | EntityKind::Spec | EntityKind::Workflow => {
            KindPalette {
                core: "#7ef7cf",
                glow: "#2bd4a7",
                ring: "#134838",
            }
        }
        EntityKind::Task | EntityKind::Feature | EntityKind::Command | EntityKind::Migration => {
            KindPalette {
                core: "#ffd476",
                glow: "#ffb347",
                ring: "#5c3610",
            }
        }
        EntityKind::Site
        | EntityKind::Template
        | EntityKind::ContentPage
        | EntityKind::Directory => KindPalette {
            core: "#92f7b9",
            glow: "#42d890",
            ring: "#14452d",
        },
        EntityKind::Containerized | EntityKind::InfraRoot => KindPalette {
            core: "#bbf26d",
            glow: "#95d93a",
            ring: "#38511a",
        },
    }
}

fn scene_bounds(nodes: &[PositionedNode]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for node in nodes {
        min_x = min_x.min(node.x - node.radius - 32.0);
        max_x = max_x.max(node.x + node.radius + 32.0);
        min_y = min_y.min(node.y - node.radius - 32.0);
        max_y = max_y.max(node.y + node.radius + 32.0);

        if let Some(label) = label_for(node) {
            let title_width =
                estimate_text_width(&label.title, if label.prominent { 18.0 } else { 15.0 });
            let subtitle_width = estimate_text_width(&label.subtitle, 12.0);
            let label_width = title_width.max(subtitle_width);
            let (left, right) = match label.anchor {
                "start" => (label.x, label.x + label_width),
                "end" => (label.x - label_width, label.x),
                _ => (label.x - label_width / 2.0, label.x + label_width / 2.0),
            };
            min_x = min_x.min(left - 20.0);
            max_x = max_x.max(right + 20.0);
            min_y = min_y.min(label.y - 22.0);
            max_y = max_y.max(label.y + 28.0);
        }
    }

    let padding = 180.0;
    let width = (max_x - min_x + padding * 2.0).max(1220.0);
    let height = (max_y - min_y + padding * 2.0).max(860.0);
    (-min_x + padding, -min_y + padding, width, height)
}

fn render_hud(out: &mut String, focus_node: &PositionedNode, node_count: usize, edge_count: usize) {
    let hud_x = 48.0;
    let hud_y = 42.0;
    let hud_width = 340.0;
    let hud_height = 96.0;
    writeln!(
        out,
        r#"<g><rect class="hud-card" x="{hud_x}" y="{hud_y}" width="{hud_width}" height="{hud_height}" rx="18"/>"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<text class="hud-title" x="{}" y="{}">{}</text>"#,
        hud_x + 24.0,
        hud_y + 34.0,
        escape_xml(&focus_node.name)
    )
    .unwrap();
    writeln!(
        out,
        r#"<text class="hud-subtitle" x="{}" y="{}">focus node · {} · {} nodes · {} edges</text></g>"#,
        hud_x + 24.0,
        hud_y + 62.0,
        escape_xml(&focus_node.kind.to_string()),
        node_count,
        edge_count
    )
    .unwrap();
}

fn render_atmospheric_rays(
    out: &mut String,
    nodes: &[PositionedNode],
    layout_root: &PositionedNode,
    overview_mode: bool,
) {
    if overview_mode {
        return;
    }
    for node in nodes.iter().filter(|node| node.depth == 1).take(14) {
        let (c1x, c1y, c2x, c2y) = edge_control_points(layout_root, node, true);
        writeln!(
            out,
            r#"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" stroke="{}" stroke-width="{:.1}" opacity="0.08" fill="none" filter="url(#soft-glow)"/>"#,
            layout_root.x,
            layout_root.y,
            c1x,
            c1y,
            c2x,
            c2y,
            node.x,
            node.y,
            node.palette.glow,
            node.radius * 1.6,
        )
        .unwrap();
    }
}

fn render_edges(
    out: &mut String,
    edges: &[VisualEdge],
    node_map: &HashMap<String, PositionedNode>,
    focus_id: &str,
    focus_edge_count: usize,
    overview_mode: bool,
) {
    let mut ordered = edges.to_vec();
    ordered.sort_by_key(|edge| {
        let emphasis = (edge.src_id == focus_id || edge.dst_id == focus_id) as usize;
        std::cmp::Reverse(emphasis)
    });

    for edge in &ordered {
        let Some(src) = node_map.get(&edge.src_id) else {
            continue;
        };
        let Some(dst) = node_map.get(&edge.dst_id) else {
            continue;
        };

        let emphasis = src.id == focus_id || dst.id == focus_id;
        let (c1x, c1y, c2x, c2y) = edge_control_points(src, dst, emphasis);
        let (stroke, width, opacity) = if overview_mode {
            if src.is_layout_root || dst.is_layout_root {
                ("#7f979b", 1.7, 0.88)
            } else if emphasis {
                (src.palette.core, 1.6, 0.90)
            } else {
                ("#b7c5c7", 1.0, 0.72)
            }
        } else {
            let stroke = if emphasis {
                src.palette.glow
            } else if src.is_layout_root || dst.is_layout_root {
                "#56dced"
            } else if src.depth <= 2 && dst.depth <= 2 {
                "#4ccfc7"
            } else {
                "#365d78"
            };
            let width = if emphasis {
                2.8
            } else if src.is_layout_root || dst.is_layout_root {
                1.9
            } else if src.depth <= 2 && dst.depth <= 2 {
                1.5
            } else {
                1.0
            };
            let opacity = if emphasis {
                0.94
            } else if src.is_layout_root || dst.is_layout_root {
                0.66
            } else if src.depth <= 2 && dst.depth <= 2 {
                0.46
            } else {
                0.22
            };
            (stroke, width, opacity)
        };

        writeln!(
            out,
            r#"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" stroke="{}" stroke-width="{:.1}" stroke-linecap="round" stroke-linejoin="round" opacity="{:.2}" fill="none" vector-effect="non-scaling-stroke"/>"#,
            src.x,
            src.y,
            c1x,
            c1y,
            c2x,
            c2y,
            dst.x,
            dst.y,
            stroke,
            width,
            opacity,
        )
        .unwrap();
        if !overview_mode {
            writeln!(
                out,
                r#"<path d="M {:.1} {:.1} C {:.1} {:.1} {:.1} {:.1} {:.1} {:.1}" stroke="{}" stroke-width="{:.1}" stroke-linecap="round" opacity="{:.2}" fill="none" filter="url(#edge-glow)"/>"#,
                src.x,
                src.y,
                c1x,
                c1y,
                c2x,
                c2y,
                dst.x,
                dst.y,
                stroke,
                width * 1.8,
                opacity * 0.25,
            )
            .unwrap();
        }

        if !overview_mode && should_label_edge(src, dst, focus_edge_count) {
            let (label_x, label_y) = cubic_midpoint(src.x, src.y, c1x, c1y, c2x, c2y, dst.x, dst.y);
            let rel_str = edge.rel.to_string();
            let label = truncate_label(&rel_str, 18);
            let label_width = estimate_text_width(&label, 11.0) + 18.0;
            writeln!(
                out,
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="20" rx="10" fill="rgba(6, 11, 18, 0.82)" stroke="{}" stroke-opacity="0.32" stroke-width="1"/>"#,
                label_x - label_width / 2.0,
                label_y - 15.0,
                label_width,
                stroke,
            )
            .unwrap();
            writeln!(
                out,
                r#"<text class="edge-label" text-anchor="middle" x="{:.1}" y="{:.1}">{}</text>"#,
                label_x,
                label_y - 1.0,
                escape_xml(&label)
            )
            .unwrap();
        }
    }
}

fn render_nodes(out: &mut String, nodes: &[PositionedNode], overview_mode: bool) {
    for node in nodes {
        if overview_mode {
            if node.is_layout_root || node.is_focus {
                writeln!(
                    out,
                    r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" opacity="0.14"/>"#,
                    node.x,
                    node.y,
                    node.radius * 2.4,
                    node.palette.glow
                )
                .unwrap();
            }
            writeln!(
                out,
                r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="#fbfaf7" stroke="{}" stroke-width="{:.1}"/>"##,
                node.x,
                node.y,
                node.radius,
                node.palette.core,
                if node.is_layout_root { 2.0 } else if node.is_focus { 1.6 } else { 1.1 }
            )
            .unwrap();
            writeln!(
                out,
                r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" opacity="{:.2}"/>"#,
                node.x,
                node.y,
                node.radius * 0.28,
                node.palette.core,
                if node.is_layout_root || node.is_focus {
                    0.90
                } else {
                    0.72
                }
            )
            .unwrap();
            continue;
        }

        let halo_multiplier = if node.is_layout_root {
            4.0
        } else if node.is_focus {
            3.2
        } else {
            2.35
        };

        writeln!(
            out,
            r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" opacity="0.16" filter="url(#soft-glow)"/>"#,
            node.x,
            node.y,
            node.radius * halo_multiplier,
            node.palette.glow
        )
        .unwrap();

        if node.is_layout_root || node.is_focus {
            writeln!(
                out,
                r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="none" stroke="{}" stroke-width="{:.1}" opacity="{:.2}"/>"#,
                node.x,
                node.y,
                node.radius * if node.is_layout_root { 2.15 } else { 1.75 },
                node.palette.core,
                if node.is_layout_root { 1.8 } else { 1.4 },
                if node.is_layout_root { 0.34 } else { 0.26 },
            )
            .unwrap();
        }

        writeln!(
            out,
            r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" fill-opacity="0.10" stroke="none"/>"#,
            node.x,
            node.y,
            node.radius * 1.45,
            node.palette.ring
        )
        .unwrap();
        writeln!(
            out,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" stroke="#e9fbff" stroke-opacity="0.42" stroke-width="{:.1}"/>"##,
            node.x,
            node.y,
            node.radius,
            node.palette.core,
            if node.is_layout_root { 2.2 } else if node.is_focus { 1.8 } else { 1.2 }
        )
        .unwrap();
        writeln!(
            out,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="#f8feff" opacity="{:.2}"/>"##,
            node.x,
            node.y,
            node.radius * 0.36,
            if node.is_layout_root || node.is_focus {
                0.94
            } else {
                0.78
            }
        )
        .unwrap();
    }
}

fn render_labels(out: &mut String, nodes: &[PositionedNode]) {
    for node in nodes {
        let Some(label) = label_for(node) else {
            continue;
        };
        let title_class = if label.prominent {
            "node-title"
        } else {
            "node-title compact"
        };
        writeln!(
            out,
            r#"<text class="{title_class}" text-anchor="{}" x="{:.1}" y="{:.1}">{}</text>"#,
            label.anchor,
            label.x,
            label.y,
            escape_xml(&label.title)
        )
        .unwrap();
        if !label.subtitle.is_empty() {
            writeln!(
                out,
                r#"<text class="node-kind" text-anchor="{}" x="{:.1}" y="{:.1}">{}</text>"#,
                label.anchor,
                label.x,
                label.y + 19.0,
                escape_xml(&label.subtitle)
            )
            .unwrap();
        }
    }
}

fn label_for(node: &PositionedNode) -> Option<LabelPlacement> {
    if !should_label_node(node) {
        return None;
    }

    if node.is_layout_root {
        return Some(LabelPlacement {
            title: truncate_label(&node.name, 28),
            subtitle: node.kind.to_string(),
            x: node.x,
            y: node.y - node.radius - 44.0,
            anchor: "middle",
            prominent: true,
        });
    }

    if node.is_focus {
        return Some(LabelPlacement {
            title: truncate_label(&node.name, 26),
            subtitle: node.kind.to_string(),
            x: node.x,
            y: node.y + node.radius + 28.0,
            anchor: "middle",
            prominent: true,
        });
    }

    let subtitle = if matches!(node.kind, EntityKind::SourceUnit) {
        String::new()
    } else {
        node.kind.to_string()
    };

    Some(LabelPlacement {
        title: truncate_label(&node.name, if node.depth == 1 { 22 } else { 18 }),
        subtitle,
        x: node.x,
        y: node.y + node.radius + if node.depth == 1 { 28.0 } else { 18.0 },
        anchor: "middle",
        prominent: node.depth == 1,
    })
}

fn should_label_node(node: &PositionedNode) -> bool {
    if node.is_layout_root || node.is_focus {
        return true;
    }

    match node.depth {
        0 | 1 => true,
        2 => {
            node.degree > 1
                || matches!(
                    node.kind,
                    EntityKind::Component | EntityKind::Doc | EntityKind::SourceUnit
                )
        }
        3 => node.degree > 2 && !matches!(node.kind, EntityKind::Symbol),
        _ => false,
    }
}

fn should_label_edge(src: &PositionedNode, dst: &PositionedNode, focus_edge_count: usize) -> bool {
    if src.is_focus || dst.is_focus {
        return focus_edge_count <= 8;
    }
    if src.is_layout_root || dst.is_layout_root {
        return focus_edge_count <= 4;
    }
    src.depth <= 1 && dst.depth <= 2 && (src.degree <= 4 || dst.degree <= 4)
}

fn edge_control_points(
    src: &PositionedNode,
    dst: &PositionedNode,
    emphasis: bool,
) -> (f64, f64, f64, f64) {
    let dx = dst.x - src.x;
    let dy = dst.y - src.y;

    if dy.abs() >= 40.0 {
        let bend = (dy.abs() * if emphasis { 0.44 } else { 0.36 }).clamp(34.0, 150.0);
        let direction = dy.signum();
        (
            src.x,
            src.y + direction * bend,
            dst.x,
            dst.y - direction * bend,
        )
    } else {
        let arch = if emphasis { 66.0 } else { 40.0 };
        (
            src.x + dx * 0.18,
            src.y - arch,
            dst.x - dx * 0.18,
            dst.y - arch,
        )
    }
}

fn cubic_midpoint(
    x1: f64,
    y1: f64,
    c1x: f64,
    c1y: f64,
    c2x: f64,
    c2y: f64,
    x2: f64,
    y2: f64,
) -> (f64, f64) {
    (
        0.125 * x1 + 0.375 * c1x + 0.375 * c2x + 0.125 * x2,
        0.125 * y1 + 0.375 * c1y + 0.375 * c2y + 0.125 * y2,
    )
}

fn editor_link_for_entity(
    repo_root: &Path,
    entity: &Entity,
    template: Option<&str>,
) -> Option<String> {
    let template = template?;
    let repo_path = entity.path.as_deref().unwrap_or(".");
    let abs_path = if repo_path == "." {
        repo_root.to_path_buf()
    } else {
        repo_root.join(repo_path)
    };
    let abs_path = abs_path.to_string_lossy();
    let line = entity.line_start.unwrap_or(1).to_string();
    let column = "1".to_string();

    Some(
        template
            .replace("{abs_path}", abs_path.as_ref())
            .replace("{repo_path}", repo_path)
            .replace("{line}", &line)
            .replace("{column}", &column)
            .replace("{entity_id}", &entity.id),
    )
}

fn display_name(entity: &Entity) -> String {
    if entity.kind == EntityKind::Repo {
        return "Repository".to_string();
    }
    if entity.kind == EntityKind::SourceUnit {
        let path = entity.path.as_deref().unwrap_or(&entity.name);
        return compact_path_label(path);
    }
    if entity.name.trim().is_empty() {
        return entity.id.clone();
    }
    entity.name.clone()
}

fn compact_path_label(path: &str) -> String {
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    parts[parts.len().saturating_sub(2)..].join("/")
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn estimate_text_width(text: &str, font_size: f64) -> f64 {
    text.chars().count() as f64 * font_size * 0.56
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn escape_json_for_html(value: &str) -> String {
    value
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

fn pan_zoom_script() -> &'static str {
    r#"
<script><![CDATA[
(function() {
  var svg = document.querySelector('svg');
  var g = document.getElementById('graph');
  if (!svg || !g) return;
  var pt = svg.createSVGPoint();
  var tx = 0, ty = 0, scale = 1;
  var dragging = false, startX = 0, startY = 0, startTx = 0, startTy = 0;

  function applyTransform() {
    g.setAttribute('transform', 'translate(' + tx + ',' + ty + ') scale(' + scale + ')');
  }

  svg.addEventListener('wheel', function(e) {
    e.preventDefault();
    pt.x = e.clientX;
    pt.y = e.clientY;
    var loc = pt.matrixTransform(svg.getScreenCTM().inverse());
    var factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
    var nextScale = scale * factor;
    if (nextScale < 0.08 || nextScale > 24) return;
    tx = loc.x - (loc.x - tx) * factor;
    ty = loc.y - (loc.y - ty) * factor;
    scale = nextScale;
    applyTransform();
  }, { passive: false });

  svg.addEventListener('mousedown', function(e) {
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    startTx = tx;
    startTy = ty;
    svg.style.cursor = 'grabbing';
  });

  svg.addEventListener('mousemove', function(e) {
    if (!dragging) return;
    var ctm = svg.getScreenCTM();
    tx = startTx + (e.clientX - startX) / ctm.a;
    ty = startTy + (e.clientY - startY) / ctm.d;
    applyTransform();
  });

  function stopDrag(cursor) {
    dragging = false;
    svg.style.cursor = cursor;
  }

  svg.addEventListener('mouseup', function() { stopDrag('grab'); });
  svg.addEventListener('mouseleave', function() { stopDrag('default'); });
  svg.style.cursor = 'grab';
})();
]]></script>
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_focus_graph_svg_with_custom_scene() {
        let repo = Entity::new("repo::.", EntityKind::Repo, "repo");
        let component = Entity::new(
            "component::cargo::fixture",
            EntityKind::Component,
            "fixture",
        );
        let symbol = Entity::new("symbol::src/lib.rs::handler", EntityKind::Symbol, "handler")
            .with_exported(true);

        let entity_cache = HashMap::from([
            (repo.id.clone(), repo),
            (component.id.clone(), component),
            (symbol.id.clone(), symbol),
        ]);
        let visited_edges = HashSet::from([
            (
                "repo::.".to_string(),
                EdgeKind::Contains,
                "component::cargo::fixture".to_string(),
            ),
            (
                "component::cargo::fixture".to_string(),
                EdgeKind::Contains,
                "symbol::src/lib.rs::handler".to_string(),
            ),
        ]);

        let svg = render_focus_graph_svg(&entity_cache, &visited_edges, Some("repo::."));
        assert!(svg.contains("id=\"graph\""));
        assert!(svg.contains("Repository"));
        assert!(svg.contains("fixture"));
        assert!(svg.contains("bg-gradient"));
    }

    #[test]
    fn ignores_edges_to_missing_nodes() {
        let repo = Entity::new("repo::.", EntityKind::Repo, "repo");
        let component = Entity::new(
            "component::cargo::fixture",
            EntityKind::Component,
            "fixture",
        );

        let entity_cache =
            HashMap::from([(repo.id.clone(), repo), (component.id.clone(), component)]);
        let visited_edges = HashSet::from([
            (
                "repo::.".to_string(),
                EdgeKind::Contains,
                "component::cargo::fixture".to_string(),
            ),
            (
                "component::cargo::fixture".to_string(),
                EdgeKind::Contains,
                "source_unit::missing.rs".to_string(),
            ),
        ]);

        let svg = render_focus_graph_svg(&entity_cache, &visited_edges, Some("repo::."));
        assert!(svg.contains("Repository"));
        assert!(svg.contains("fixture"));
        assert!(!svg.contains("missing.rs"));
    }

    #[test]
    fn renders_interactive_html_tree_explorer() {
        let repo = Entity::new("repo::.", EntityKind::Repo, "repo");
        let component = Entity::new(
            "component::cargo::fixture",
            EntityKind::Component,
            "fixture",
        );
        let source_unit = Entity::new(
            "source_unit::src/lib.rs",
            EntityKind::SourceUnit,
            "src/lib.rs",
        )
        .with_path("src/lib.rs");

        let entity_cache = HashMap::from([
            (repo.id.clone(), repo),
            (component.id.clone(), component),
            (source_unit.id.clone(), source_unit),
        ]);
        let summary_cache = HashMap::from([(
            "component::cargo::fixture".to_string(),
            Summary::new(
                "component::cargo::fixture",
                "Core fixture component for the example repository.",
            ),
        )]);
        let visited_edges = HashSet::from([
            (
                "repo::.".to_string(),
                EdgeKind::Contains,
                "component::cargo::fixture".to_string(),
            ),
            (
                "component::cargo::fixture".to_string(),
                EdgeKind::Contains,
                "source_unit::src/lib.rs".to_string(),
            ),
        ]);

        let html = render_focus_graph_html(
            &entity_cache,
            &summary_cache,
            &visited_edges,
            Path::new("/tmp/fixture-repo"),
            Some("vscode://file/{abs_path}:{line}:{column}"),
            Some("repo::."),
        );
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("tree explorer"));
        assert!(html.contains("chizu-data"));
        assert!(html.contains("search-input"));
        assert!(html.contains("theme-btn"));
        assert!(html.contains("chizu-theme"));
        assert!(html.contains("source_unit::src/lib.rs"));
        assert!(html.contains("Core fixture component for the example repository."));
        assert!(html.contains("vscode://file//tmp/fixture-repo/src/lib.rs:1:1"));
    }

    #[test]
    fn long_symbol_names_are_clamped_in_interactive_cards() {
        let source_unit = Entity::new(
            "source_unit::src/config/mod.rs",
            EntityKind::SourceUnit,
            "src/config/mod.rs",
        )
        .with_path("src/config/mod.rs");
        let symbol = Entity::new(
            "symbol::src/config/mod.rs::default_exclude_patterns",
            EntityKind::Symbol,
            "default_exclude_patterns",
        )
        .with_path("src/config/mod.rs")
        .with_lines(1, 12)
        .with_exported(true);

        let entity_cache = HashMap::from([
            (source_unit.id.clone(), source_unit),
            (symbol.id.clone(), symbol),
        ]);
        let summary_cache = HashMap::new();
        let visited_edges = HashSet::from([(
            "source_unit::src/config/mod.rs".to_string(),
            EdgeKind::Defines,
            "symbol::src/config/mod.rs::default_exclude_patterns".to_string(),
        )]);

        let html = render_focus_graph_html(
            &entity_cache,
            &summary_cache,
            &visited_edges,
            Path::new("/tmp/fixture-repo"),
            None,
            Some("source_unit::src/config/mod.rs"),
        );
        assert!(html.contains("default_exclude_patterns"));
        assert!(html.contains(".children-grid .card-title"));
        assert!(html.contains(".children-grid .card-path"));
        assert!(html.contains("class=\"card-title\" title=\"${title}\""));
        assert!(html.contains("overflow-wrap: anywhere;"));
    }
}
