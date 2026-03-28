use chizu_core::model::edge::EdgeKind;
use chizu_core::model::entity::EntityKind;

/// Scandinavian design system for technical diagrams
#[derive(Debug, Clone)]
pub struct ScandinavianTheme {
    /// Page background
    pub canvas: &'static str,
    /// Card/container backgrounds
    pub surface: &'static str,
    /// Default borders
    pub border_light: &'static str,
    /// Active/important borders
    pub border_emphasis: &'static str,
    /// Primary text
    pub text_primary: &'static str,
    /// Secondary text
    pub text_secondary: &'static str,
    /// Muted/disabled text
    pub text_muted: &'static str,
}

impl Default for ScandinavianTheme {
    fn default() -> Self {
        Self {
            canvas: "#fafafa",
            surface: "#ffffff",
            border_light: "#e2e8f0",
            border_emphasis: "#cbd5e1",
            text_primary: "#1e293b",
            text_secondary: "#64748b",
            text_muted: "#94a3b8",
        }
    }
}

/// Styling for entity nodes
#[derive(Debug, Clone)]
pub struct EntityStyle {
    /// Border color (hex)
    pub border: String,
    /// Light fill color (hex with opacity)
    pub fill: String,
    /// Text color
    pub text: String,
}

/// Line types for edges
#[derive(Debug, Clone, Copy)]
pub enum LineType {
    Solid,
    Dashed,
    Dotted,
}

/// Styling for edges
#[derive(Debug, Clone)]
pub struct EdgeStyle {
    /// Line color
    pub color: String,
    /// Line width
    pub width: f64,
    /// Line type (solid, dashed, dotted)
    pub line_type: LineType,
}

impl ScandinavianTheme {
    /// Get the style for an entity kind
    /// Follows the principle: different things = different colors
    pub fn entity_style(&self, kind: &EntityKind) -> EntityStyle {
        match kind {
            EntityKind::Symbol => EntityStyle {
                border: "#94a3b8".to_string(), // slate
                fill: "#f1f5f9".to_string(),   // slate-100
                text: self.text_primary.to_string(),
            },
            EntityKind::Test => EntityStyle {
                border: "#22c55e".to_string(), // green
                fill: "#f0fdf4".to_string(),   // green-50
                text: "#166534".to_string(),   // green-800
            },
            EntityKind::SourceUnit => EntityStyle {
                border: "#3b82f6".to_string(), // blue
                fill: "#eff6ff".to_string(),   // blue-50
                text: "#1e40af".to_string(),   // blue-800
            },
            EntityKind::Doc => EntityStyle {
                border: "#f59e0b".to_string(), // amber
                fill: "#fffbeb".to_string(),   // amber-50
                text: "#92400e".to_string(),   // amber-800
            },
            EntityKind::InfraRoot => EntityStyle {
                border: "#a855f7".to_string(), // purple
                fill: "#faf5ff".to_string(),   // purple-50
                text: "#6b21a8".to_string(),   // purple-800
            },
            EntityKind::Containerized => EntityStyle {
                border: "#06b6d4".to_string(), // cyan
                fill: "#ecfeff".to_string(),   // cyan-50
                text: "#155e75".to_string(),   // cyan-800
            },
            EntityKind::Repo => EntityStyle {
                border: "#1e293b".to_string(), // slate-800
                fill: "#f8fafc".to_string(),   // slate-50
                text: "#0f172a".to_string(),   // slate-900
            },
            EntityKind::Directory => EntityStyle {
                border: "#64748b".to_string(), // slate-500
                fill: "#f8fafc".to_string(),   // slate-50
                text: "#334155".to_string(),   // slate-700
            },
            EntityKind::Component => EntityStyle {
                border: "#6366f1".to_string(), // indigo
                fill: "#eef2ff".to_string(),   // indigo-50
                text: "#3730a3".to_string(),   // indigo-800
            },
            EntityKind::Bench => EntityStyle {
                border: "#f97316".to_string(), // orange
                fill: "#fff7ed".to_string(),   // orange-50
                text: "#9a3412".to_string(),   // orange-800
            },
            EntityKind::Task => EntityStyle {
                border: "#ec4899".to_string(), // pink
                fill: "#fdf2f8".to_string(),   // pink-50
                text: "#9d174d".to_string(),   // pink-800
            },
            EntityKind::Command => EntityStyle {
                border: "#14b8a6".to_string(), // teal
                fill: "#f0fdfa".to_string(),   // teal-50
                text: "#115e59".to_string(),   // teal-800
            },
            EntityKind::Feature => EntityStyle {
                border: "#8b5cf6".to_string(), // violet
                fill: "#f5f3ff".to_string(),   // violet-50
                text: "#5b21b6".to_string(),   // violet-800
            },
            EntityKind::ContentPage => EntityStyle {
                border: "#f43f5e".to_string(), // rose
                fill: "#fff1f2".to_string(),   // rose-50
                text: "#9f1239".to_string(),   // rose-800
            },
            EntityKind::Template => EntityStyle {
                border: "#0ea5e9".to_string(), // sky
                fill: "#f0f9ff".to_string(),   // sky-50
                text: "#075985".to_string(),   // sky-800
            },
            EntityKind::Site => EntityStyle {
                border: "#84cc16".to_string(), // lime
                fill: "#f7fee7".to_string(),   // lime-50
                text: "#3f6212".to_string(),   // lime-800
            },
            EntityKind::Migration => EntityStyle {
                border: "#d946ef".to_string(), // fuchsia
                fill: "#fdf4ff".to_string(),   // fuchsia-50
                text: "#86198f".to_string(),   // fuchsia-800
            },
            EntityKind::Spec => EntityStyle {
                border: "#eab308".to_string(), // yellow
                fill: "#fefce8".to_string(),   // yellow-50
                text: "#854d0e".to_string(),   // yellow-800
            },
            EntityKind::Workflow => EntityStyle {
                border: "#10b981".to_string(), // emerald
                fill: "#ecfdf5".to_string(),   // emerald-50
                text: "#065f46".to_string(),   // emerald-800
            },
            EntityKind::AgentConfig => EntityStyle {
                border: "#6b7280".to_string(), // gray
                fill: "#f9fafb".to_string(),   // gray-50
                text: "#374151".to_string(),   // gray-700
            },
        }
    }

    /// Get the style for an edge kind
    pub fn edge_style(&self, kind: &EdgeKind) -> EdgeStyle {
        match kind {
            EdgeKind::Defines => EdgeStyle {
                color: "#94a3b8".to_string(), // slate
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::DependsOn => EdgeStyle {
                color: "#64748b".to_string(), // slate-500
                width: 1.0,
                line_type: LineType::Dashed,
            },
            EdgeKind::TestedBy => EdgeStyle {
                color: "#22c55e".to_string(), // green
                width: 1.0,
                line_type: LineType::Dashed,
            },
            EdgeKind::Mentions => EdgeStyle {
                color: "#f59e0b".to_string(), // amber
                width: 1.0,
                line_type: LineType::Dotted,
            },
            EdgeKind::Deploys => EdgeStyle {
                color: "#a855f7".to_string(), // purple
                width: 1.5,
                line_type: LineType::Solid,
            },
            EdgeKind::Contains => EdgeStyle {
                color: "#cbd5e1".to_string(), // slate-300
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::Reexports => EdgeStyle {
                color: "#3b82f6".to_string(), // blue
                width: 1.0,
                line_type: LineType::Dashed,
            },
            EdgeKind::DocumentedBy => EdgeStyle {
                color: "#f59e0b".to_string(), // amber
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::BenchmarkedBy => EdgeStyle {
                color: "#f97316".to_string(), // orange
                width: 1.0,
                line_type: LineType::Dashed,
            },
            EdgeKind::RelatedTo => EdgeStyle {
                color: "#a855f7".to_string(), // purple
                width: 1.0,
                line_type: LineType::Dotted,
            },
            EdgeKind::ConfiguredBy => EdgeStyle {
                color: "#06b6d4".to_string(), // cyan
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::Builds => EdgeStyle {
                color: "#22c55e".to_string(), // green
                width: 1.5,
                line_type: LineType::Solid,
            },
            EdgeKind::Implements => EdgeStyle {
                color: "#6366f1".to_string(), // indigo
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::OwnsTask => EdgeStyle {
                color: "#ec4899".to_string(), // pink
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::DeclaresFeature => EdgeStyle {
                color: "#8b5cf6".to_string(), // violet
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::FeatureEnables => EdgeStyle {
                color: "#8b5cf6".to_string(), // violet
                width: 1.0,
                line_type: LineType::Dashed,
            },
            EdgeKind::Migrates => EdgeStyle {
                color: "#d946ef".to_string(), // fuchsia
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::Specifies => EdgeStyle {
                color: "#eab308".to_string(), // yellow
                width: 1.0,
                line_type: LineType::Solid,
            },
            EdgeKind::Renders => EdgeStyle {
                color: "#0ea5e9".to_string(), // sky
                width: 1.0,
                line_type: LineType::Solid,
            },
        }
    }
}

/// Typography configuration
pub struct Typography {
    /// Title font family
    pub title_font: &'static str,
    /// Body font family
    pub body_font: &'static str,
    /// Title size
    pub title_size: f64,
    /// Subtitle size
    pub subtitle_size: f64,
    /// Label size
    pub label_size: f64,
    /// Body size
    pub body_size: f64,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            title_font: "monospace",
            body_font: "system-ui, -apple-system, sans-serif",
            title_size: 18.0,
            subtitle_size: 14.0,
            label_size: 12.0,
            body_size: 10.0,
        }
    }
}

/// Layout spacing constants
pub struct Spacing {
    /// Minimum padding inside containers
    pub container_padding: f64,
    /// Minimum space between major sections
    pub section_gap: f64,
    /// Accent line thickness
    pub accent_line_thickness: f64,
    /// Default corner radius for cards
    pub card_radius: f64,
    /// Default corner radius for containers
    pub container_radius: f64,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            container_padding: 15.0,
            section_gap: 20.0,
            accent_line_thickness: 4.0,
            card_radius: 4.0,
            container_radius: 8.0,
        }
    }
}
