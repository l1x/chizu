//! Chizu CLI - Local code knowledge graph tool
//!
//! Usage: chizu [--repo <path>] <command>

use argh::FromArgs;
use chizu_core::Store;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Text,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!("unknown format '{s}': expected 'text' or 'json'")),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => f.write_str("text"),
            Self::Json => f.write_str("json"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LayoutAlgorithm {
    Dot,
    Neato,
    Fdp,
}

impl std::str::FromStr for LayoutAlgorithm {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dot" => Ok(Self::Dot),
            "neato" => Ok(Self::Neato),
            "fdp" => Ok(Self::Fdp),
            _ => Err(format!(
                "unknown layout '{s}': expected 'dot', 'neato', or 'fdp'"
            )),
        }
    }
}

/// Chizu - Local repository understanding engine
#[derive(FromArgs, Debug)]
struct Cli {
    /// repository path (defaults to current directory)
    #[argh(option, short = 'r', default = "PathBuf::from(\".\")")]
    repo: PathBuf,

    #[argh(subcommand)]
    command: Command,
}

/// Available commands
#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum Command {
    /// Index the repository
    Index(IndexArgs),
    /// Search for entities
    Search(SearchArgs),
    /// Look up a single entity
    Entity(EntityArgs),
    /// List entities
    Entities(EntitiesArgs),
    /// List task routes
    Routes(RoutesArgs),
    /// List edges
    Edges(EdgesArgs),
    /// Generate graph visualization
    Visualize(VisualizeArgs),
    /// Configuration management
    Config(ConfigArgs),
    /// Show usage guide
    Guide(GuideArgs),
}

/// Index the repository (parse + summarize + embed)
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "index")]
struct IndexArgs {
    /// force re-index all files
    #[argh(switch)]
    force: bool,
}

/// Search for entities and return a ranked reading plan
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "search")]
struct SearchArgs {
    /// natural language query
    #[argh(positional)]
    query: String,

    /// maximum number of results
    #[argh(option, default = "15")]
    limit: usize,

    /// task category (understand, debug, build, test, deploy, configure, general)
    #[argh(option)]
    category: Option<chizu_core::TaskCategory>,

    /// output format (text, json)
    #[argh(option, default = "OutputFormat::Text")]
    format: OutputFormat,
}

/// Look up a single entity by ID
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "entity")]
struct EntityArgs {
    /// entity ID (e.g., symbol::src/main.rs::main)
    #[argh(positional)]
    id: String,
}

/// List entities with optional filtering
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "entities")]
struct EntitiesArgs {
    /// filter by component ID
    #[argh(option)]
    component: Option<String>,

    /// filter by entity kind
    #[argh(option)]
    kind: Option<chizu_core::EntityKind>,
}

/// List task routes
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "routes")]
struct RoutesArgs {
    /// filter by task name
    #[argh(option)]
    task: Option<String>,

    /// filter by entity ID
    #[argh(option)]
    entity: Option<String>,
}

/// List edges with optional filtering
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "edges")]
struct EdgesArgs {
    /// filter by source entity ID
    #[argh(option)]
    from: Option<String>,

    /// filter by destination entity ID
    #[argh(option)]
    to: Option<String>,

    /// filter by relationship kind
    #[argh(option)]
    rel: Option<chizu_core::EdgeKind>,
}

/// Generate graph visualization (SVG)
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "visualize")]
struct VisualizeArgs {
    /// starting entity ID
    #[argh(option)]
    entity_id: Option<String>,

    /// traversal depth
    #[argh(option, default = "2")]
    depth: u32,

    /// layout algorithm (dot, neato, fdp)
    #[argh(option, default = "LayoutAlgorithm::Dot")]
    layout: LayoutAlgorithm,

    /// maximum number of nodes
    #[argh(option, default = "100")]
    max_nodes: usize,

    /// filter by entity kind (comma-separated)
    #[argh(option)]
    kind: Option<String>,

    /// exclude entity IDs containing these substrings (comma-separated)
    #[argh(option)]
    exclude: Option<String>,

    /// include a legend in the output
    #[argh(switch)]
    legend: bool,

    /// output file path
    #[argh(option, short = 'o')]
    output: Option<PathBuf>,
}

/// Configuration management
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "config")]
struct ConfigArgs {
    /// subcommand
    #[argh(subcommand)]
    command: ConfigCommand,
}

/// Config subcommands
#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum ConfigCommand {
    /// Initialize configuration file
    Init(ConfigInitArgs),
    /// Validate configuration
    Validate(ConfigValidateArgs),
}

/// Initialize configuration file
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "init")]
struct ConfigInitArgs {
    /// overwrite existing config
    #[argh(switch, short = 'f')]
    force: bool,
}

/// Validate configuration
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "validate")]
struct ConfigValidateArgs {}

/// Show usage guide
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "guide")]
struct GuideArgs {}

fn main() {
    let cli: Cli = argh::from_env();

    tracing_subscriber::fmt::init();

    match run(cli) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!("Running command: {:?}", cli.command);

    match cli.command {
        Command::Index(args) => cmd_index(&cli.repo, args),
        Command::Search(args) => cmd_search(&cli.repo, args),
        Command::Entity(args) => cmd_entity(&cli.repo, args),
        Command::Entities(args) => cmd_entities(&cli.repo, args),
        Command::Routes(args) => cmd_routes(&cli.repo, args),
        Command::Edges(args) => cmd_edges(&cli.repo, args),
        Command::Visualize(args) => cmd_visualize(&cli.repo, args),
        Command::Config(args) => cmd_config(&cli.repo, args),
        Command::Guide(_) => cmd_guide(),
    }
}

fn cmd_entity(repo: &Path, args: EntityArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;

    let entity = store
        .get_entity(&args.id)?
        .ok_or_else(|| format!("Entity '{}' not found", args.id))?;

    println!("ID:        {}", entity.id);
    println!("Kind:      {}", entity.kind);
    println!("Name:      {}", entity.name);
    if let Some(ref path) = entity.path {
        println!("Path:      {}", path);
    }
    if let Some(ref lang) = entity.language {
        println!("Language:  {}", lang);
    }
    if let (Some(start), Some(end)) = (entity.line_start, entity.line_end) {
        println!("Lines:     {}-{}", start, end);
    }
    if let Some(ref vis) = entity.visibility {
        println!("Visibility: {}", vis);
    }
    println!("Exported:  {}", entity.exported);
    println!();

    if let Some(summary) = store.get_summary(&args.id)? {
        println!("Summary:");
        println!("  Short: {}", summary.short_summary);
        if let Some(ref detailed) = summary.detailed_summary {
            println!("  Detailed: {}", detailed);
        }
        if let Some(ref keywords) = summary.keywords {
            println!("  Keywords: {}", keywords.join(", ").as_str());
        }
        println!();
    }

    let routes = store.get_entity_task_routes(&args.id)?;
    if !routes.is_empty() {
        println!("Task Routes:");
        for route in routes {
            println!("  {} -> priority {}", route.task_name, route.priority);
        }
        println!();
    }

    let outgoing = store.get_edges_from(&args.id)?;
    if !outgoing.is_empty() {
        println!("Outgoing Edges:");
        for edge in outgoing {
            println!(
                "  {} --{}--> {}",
                edge.src_id, edge.rel, edge.dst_id
            );
        }
        println!();
    }

    let incoming = store.get_edges_to(&args.id)?;
    if !incoming.is_empty() {
        println!("Incoming Edges:");
        for edge in incoming {
            println!(
                "  {} --{}--> {}",
                edge.src_id, edge.rel, edge.dst_id
            );
        }
    }

    store.close()?;
    Ok(())
}

fn cmd_entities(repo: &Path, args: EntitiesArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;

    let entities = if let Some(ref component_str) = args.component {
        let component_id = chizu_core::ComponentId::parse(component_str)
            .ok_or_else(|| format!("Invalid component ID: {}", component_str))?;
        store.get_entities_by_component(&component_id)?
    } else if let Some(kind) = args.kind {
        store.get_entities_by_kind(kind)?
    } else {
        store.get_all_entities()?
    };

    println!("{:<40} {:<15} {:<30} {}", "ID", "Kind", "Name", "Path");
    println!("{}", "-".repeat(100));
    for entity in entities {
        let path = entity.path.as_deref().unwrap_or("-");
        println!(
            "{:<40} {:<15} {:<30} {}",
            truncate(&entity.id, 40),
            entity.kind.to_string(),
            truncate(&entity.name, 30),
            path
        );
    }

    store.close()?;
    Ok(())
}

fn cmd_routes(repo: &Path, args: RoutesArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;

    let routes = if let Some(ref task) = args.task {
        store.get_task_routes(task)?
    } else if let Some(ref entity_id) = args.entity {
        store.get_entity_task_routes(entity_id)?
    } else {
        return Err("Provide --task or --entity".into());
    };

    println!("{:<20} {:<40} {}", "Task", "Entity ID", "Priority");
    println!("{}", "-".repeat(80));
    for route in routes {
        println!(
            "{:<20} {:<40} {}",
            route.task_name, route.entity_id, route.priority
        );
    }

    store.close()?;
    Ok(())
}

fn cmd_edges(repo: &Path, args: EdgesArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;

    let mut edges = match (&args.from, &args.to, args.rel) {
        (Some(from), _, _) => store.get_edges_from(from)?,
        (_, Some(to), _) => store.get_edges_to(to)?,
        (_, _, Some(rel)) => store.get_edges_by_rel(rel)?,
        _ => return Err("Provide --from, --to, or --rel".into()),
    };

    // Cross-filter: if multiple criteria given, narrow the primary result.
    if let Some(ref to) = args.to {
        if args.from.is_some() {
            edges.retain(|e| &e.dst_id == to);
        }
    }
    if let Some(rel) = args.rel {
        if args.from.is_some() || args.to.is_some() {
            edges.retain(|e| e.rel == rel);
        }
    }

    println!("{:<40} {:<20} {:<40} {}", "Source", "Rel", "Destination", "Provenance");
    println!("{}", "-".repeat(120));
    for edge in edges {
        let provenance = edge
            .provenance_path
            .as_deref()
            .map(|p| format!("{}:{}", p, edge.provenance_line.unwrap_or(0)))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<40} {:<20} {:<40} {}",
            truncate(&edge.src_id, 40),
            edge.rel.to_string(),
            truncate(&edge.dst_id, 40),
            provenance
        );
    }

    store.close()?;
    Ok(())
}

fn cmd_visualize(repo: &Path, args: VisualizeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;

    let kind_filter: Option<Vec<String>> = args
        .kind
        .as_ref()
        .map(|k| k.split(',').map(|s| s.trim().to_string()).collect());
    let exclude_patterns: Vec<String> = args
        .exclude
        .as_ref()
        .map(|e| e.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let mut entity_cache: std::collections::HashMap<String, chizu_core::Entity> = std::collections::HashMap::new();
    let mut visited_edges: std::collections::HashSet<(String, String, String)> = std::collections::HashSet::new();
    let mut queue: Vec<(String, u32)> = Vec::new();

    if let Some(ref start_id) = args.entity_id {
        queue.push((start_id.clone(), 0));
    } else {
        for entity in store.get_all_entities()? {
            queue.push((entity.id, 0));
        }
    }

    while let Some((entity_id, depth)) = queue.pop() {
        if entity_cache.contains_key(&entity_id) {
            continue;
        }
        if entity_cache.len() >= args.max_nodes {
            break;
        }

        let Some(entity) = store.get_entity(&entity_id)? else { continue };

        if let Some(ref kinds) = kind_filter {
            if !kinds.contains(&entity.kind.to_string()) {
                continue;
            }
        }
        if exclude_patterns.iter().any(|p| entity.id.contains(p)) {
            continue;
        }

        entity_cache.insert(entity_id.clone(), entity);

        if depth < args.depth {
            for edge in store.get_edges_from(&entity_id)? {
                let key = (edge.src_id.clone(), edge.rel.to_string(), edge.dst_id.clone());
                if visited_edges.insert(key) {
                    queue.push((edge.dst_id.clone(), depth + 1));
                }
            }
            for edge in store.get_edges_to(&entity_id)? {
                let key = (edge.src_id.clone(), edge.rel.to_string(), edge.dst_id.clone());
                if visited_edges.insert(key) {
                    queue.push((edge.src_id.clone(), depth + 1));
                }
            }
        }
    }

    let mut dot = String::from("digraph chizu {\n");
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=box, style=\"rounded,filled\", fontname=\"Helvetica\"];\n");
    dot.push_str("  edge [fontname=\"Helvetica\", fontsize=10];\n\n");

    for (id, entity) in &entity_cache {
        let label = format!("{}\\n({})", escape_dot(&entity.name), entity.kind);
        let color = kind_color(entity.kind);
        dot.push_str(&format!(
            "  \"{}\" [label=\"{}\", fillcolor=\"{}\"];\n",
            escape_dot(id),
            label,
            color
        ));
    }

    dot.push_str("\n");

    for (src, rel, dst) in &visited_edges {
        if entity_cache.contains_key(src) && entity_cache.contains_key(dst) {
            dot.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                escape_dot(src),
                escape_dot(dst),
                escape_dot(rel)
            ));
        }
    }

    if args.legend {
        dot.push_str("\n  subgraph cluster_legend {\n");
        dot.push_str("    label=\"Legend\";\n");
        dot.push_str("    style=filled;\n");
        dot.push_str("    color=lightgrey;\n");
        let kinds = [
            ("Component", kind_color(chizu_core::EntityKind::Component)),
            ("Symbol", kind_color(chizu_core::EntityKind::Symbol)),
            ("Test", kind_color(chizu_core::EntityKind::Test)),
            ("Doc", kind_color(chizu_core::EntityKind::Doc)),
        ];
        for (i, (name, color)) in kinds.iter().enumerate() {
            dot.push_str(&format!(
                "    legend{} [label=\"{}\", fillcolor=\"{}\", shape=box];\n",
                i, name, color
            ));
        }
        dot.push_str("  }\n");
    }

    dot.push_str("}\n");

    // Try to use graphviz dot binary
    let layout = match args.layout {
        LayoutAlgorithm::Dot => "dot",
        LayoutAlgorithm::Neato => "neato",
        LayoutAlgorithm::Fdp => "fdp",
    };

    let output = match std::process::Command::new(layout)
        .arg("-Tsvg")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(dot.as_bytes())?;
            let result = child.wait_with_output()?;
            if result.status.success() {
                result.stdout
            } else {
                let err = String::from_utf8_lossy(&result.stderr);
                return Err(format!("Graphviz {} failed: {}", layout, err).into());
            }
        }
        Err(_) => {
            println!("{}", dot);
            return Err("Graphviz not found. Install it to generate SVG, or use the DOT output above.".into());
        }
    };

    if let Some(ref path) = args.output {
        std::fs::write(path, &output)?;
        println!("Wrote SVG to {}", path.display());
    } else {
        println!("{}", String::from_utf8_lossy(&output));
    }

    store.close()?;
    Ok(())
}

fn kind_color(kind: chizu_core::EntityKind) -> &'static str {
    use chizu_core::EntityKind::*;
    match kind {
        Component => "#a5b4fc",
        SourceUnit => "#fde047",
        Symbol => "#86efac",
        Test => "#fca5a5",
        Doc => "#c4b5fd",
        Feature => "#fdba74",
        Task => "#93c5fd",
        Site => "#d8b4fe",
        Template => "#fcd34d",
        Migration => "#fdba74",
        Workflow => "#bfdbfe",
        AgentConfig => "#ddd6fe",
        Bench => "#fca5a5",
        Containerized => "#93c5fd",
        InfraRoot => "#93c5fd",
        Command => "#86efac",
        ContentPage => "#c4b5fd",
        Spec => "#c4b5fd",
        Repo | Directory => "#e2e8f0",
    }
}

fn escape_dot(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn truncate(s: &str, max_len: usize) -> std::borrow::Cow<'_, str> {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3]).into()
    } else {
        s.into()
    }
}

fn cmd_index(repo: &Path, args: IndexArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.force {
        let chizu_dir = repo.join(".chizu");
        if chizu_dir.exists() {
            std::fs::remove_dir_all(&chizu_dir)?;
        }
    }

    let (config, store) = open_store(repo)?;
    let provider = build_provider(&config)?;
    let stats = chizu_index::IndexPipeline::run(repo, &store, &config, provider.as_deref())?;

    println!("Indexed {} files ({} walked)", stats.files_indexed, stats.files_walked);
    println!("Discovered {} components", stats.components_discovered);
    println!(
        "Inserted {} entities and {} edges",
        stats.entities_inserted, stats.edges_inserted
    );
    if config.summary.provider.is_some() {
        println!(
            "Summaries: {} generated, {} skipped, {} failed",
            stats.summaries_generated, stats.summaries_skipped, stats.summaries_failed
        );
    }
    if config.embedding.provider.is_some() {
        println!(
            "Embeddings: {} generated, {} skipped, {} failed",
            stats.embeddings_generated, stats.embeddings_skipped, stats.embeddings_failed
        );
    }

    let failures = stats.summaries_failed + stats.embeddings_failed;
    if failures > 0 {
        eprintln!("Warning: {} LLM operations failed; index is degraded.", failures);
    }

    store.close()?;
    Ok(())
}

fn build_provider(config: &chizu_core::Config) -> Result<Option<Box<dyn chizu_core::Provider>>, Box<dyn std::error::Error>> {
    let summary_provider = config.summary.provider.as_ref();
    let embedding_provider = config.embedding.provider.as_ref();

    let provider_name = match (summary_provider, embedding_provider) {
        (Some(s), Some(e)) if s == e => Some(s.as_str()),
        (Some(s), None) => Some(s.as_str()),
        (None, Some(e)) => Some(e.as_str()),
        (Some(s), Some(e)) => {
            return Err(format!(
                "Different providers for summary ({}) and embedding ({}) are not yet supported. Please use the same provider.",
                s, e
            ).into());
        }
        (None, None) => None,
    };

    let Some(name) = provider_name else {
        return Ok(None);
    };

    let provider_config = config.providers.get(name)
        .ok_or_else(|| format!("Provider '{}' not found in config", name))?;

    let completion_model = config.summary.model.clone().unwrap_or_else(|| "llama3:8b".to_string());
    let embedding_model = config.embedding.model.clone().unwrap_or_else(|| "nomic-embed-text-v2-moe:latest".to_string());

    let provider = chizu_core::OpenAiProvider::new(
        provider_config,
        completion_model,
        embedding_model,
    ).map_err(|e| format!("Failed to create provider: {e}"))?;

    Ok(Some(Box::new(provider)))
}

fn cmd_search(repo: &Path, args: SearchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (config, store) = open_store(repo)?;
    let provider = build_provider(&config)?;

    let plan = chizu_query::SearchPipeline::run(
        &store,
        &args.query,
        args.category,
        args.limit,
        &config,
        provider.as_deref(),
    )?;

    match args.format {
        OutputFormat::Text => println!("{}", plan.to_text()),
        OutputFormat::Json => println!("{}", plan.to_json()?),
    }

    // Warn if embeddings are configured but no provider could be built.
    if config.embedding.provider.is_some() && provider.is_none() {
        eprintln!("Warning: embeddings are configured but provider is unavailable; semantic search disabled.");
    }

    store.close()?;
    Ok(())
}

fn load_config(repo: &Path) -> Result<chizu_core::Config, Box<dyn std::error::Error>> {
    let config_path = repo.join(".chizu.toml");
    if config_path.exists() {
        let config_str = std::fs::read_to_string(&config_path)?;
        Ok(chizu_core::Config::from_toml(&config_str)?)
    } else {
        Ok(chizu_core::Config::default())
    }
}

fn open_store(repo: &Path) -> Result<(chizu_core::Config, chizu_core::ChizuStore), Box<dyn std::error::Error>> {
    let config = load_config(repo)?;
    let store = chizu_core::ChizuStore::open(&repo.join(".chizu"), &config)?;
    Ok((config, store))
}

fn cmd_config(repo: &Path, args: ConfigArgs) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        ConfigCommand::Init(init_args) => {
            let config_path = repo.join(".chizu.toml");

            if config_path.exists() && !init_args.force {
                return Err(format!(
                    "Config already exists at {}. Use --force to overwrite.",
                    config_path.display()
                )
                .into());
            }

            let toml = r#"[index]
exclude_patterns = [
    "**/target/**",
    "**/.git/**",
    "**/node_modules/**",
    "**/.venv/**",
    "**/fuzz/**",
    "**/*.lock",
]

[search]
default_limit = 15

[search.rerank_weights]
task_route = 0.00
keyword = 0.25
name_match = 0.20
vector = 0.25
kind_preference = 0.10
exported = 0.10
path_match = 0.10

[providers.ollama]
base_url = "http://localhost:11434/v1"
timeout_secs = 120
retry_attempts = 3

[summary]
provider = "ollama"
model = "llama3:8b"
max_tokens = 512
temperature = 0.2

[embedding]
provider = "ollama"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32
"#;

            std::fs::write(&config_path, toml)?;
            println!("Created config at {}", config_path.display());
            Ok(())
        }
        ConfigCommand::Validate(_) => {
            let config_path = repo.join(".chizu.toml");

            if !config_path.exists() {
                println!(
                    "No config file found at {}. Using defaults.",
                    config_path.display()
                );
                return Ok(());
            }

            let config_str = std::fs::read_to_string(&config_path)?;
            match chizu_core::Config::from_toml(&config_str) {
                Ok(_) => {
                    println!("Configuration is valid!");
                    Ok(())
                }
                Err(e) => Err(format!("Configuration error: {}", e).into()),
            }
        }
    }
}

fn cmd_guide() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r#"Chizu - Local Repository Understanding Engine

USAGE:
    chizu [--repo <path>] <command>

COMMANDS:
    index       Parse repository, generate summaries and embeddings
    search      Natural language search for relevant entities
    entity      Look up a specific entity by ID
    entities    List entities with optional filters
    routes      List task route assignments
    edges       List edges/relationships
    visualize   Generate SVG graph visualization
    config      Initialize or validate configuration
    guide       Show this help message

EXAMPLES:
    # Index current directory
    chizu index

    # Search for routing-related code
    chizu search "how does routing work"

    # Look up a specific symbol
    chizu entity "symbol::src/main.rs::main"

    # List all test entities
    chizu entities --kind test

    # Generate visualization
    chizu visualize --entity-id "component::cargo::." --output graph.svg

For more information, see the documentation at:
    https://github.com/l1x/chizu
"#
    );
    Ok(())
}
