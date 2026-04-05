//! Chizu CLI - Local code knowledge graph tool
//!
//! Usage: chizu [--repo <path>] <command>

mod visualize;

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

/// Generate graph visualization (SVG or interactive HTML)
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "visualize")]
struct VisualizeArgs {
    /// starting entity ID
    #[argh(option)]
    entity_id: Option<String>,

    /// traversal depth
    #[argh(option, default = "2")]
    depth: u32,

    /// maximum number of nodes
    #[argh(option, default = "100")]
    max_nodes: usize,

    /// filter by entity kind (comma-separated)
    #[argh(option)]
    kind: Option<String>,

    /// exclude entity IDs containing these substrings (comma-separated)
    #[argh(option)]
    exclude: Option<String>,

    /// emit an interactive HTML tree explorer instead of a static SVG
    #[argh(switch)]
    interactive: bool,

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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("chizu_index=info")),
        )
        .init();

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
    let store = open_store_only(repo)?;

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
            println!("  {} --{}--> {}", edge.src_id, edge.rel, edge.dst_id);
        }
        println!();
    }

    let incoming = store.get_edges_to(&args.id)?;
    if !incoming.is_empty() {
        println!("Incoming Edges:");
        for edge in incoming {
            println!("  {} --{}--> {}", edge.src_id, edge.rel, edge.dst_id);
        }
    }

    store.close()?;
    Ok(())
}

fn cmd_entities(repo: &Path, args: EntitiesArgs) -> Result<(), Box<dyn std::error::Error>> {
    let store = open_store_only(repo)?;

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
    let store = open_store_only(repo)?;

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
    let store = open_store_only(repo)?;

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

    println!(
        "{:<40} {:<20} {:<40} {}",
        "Source", "Rel", "Destination", "Provenance"
    );
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
    let (config, store) = open_store(repo)?;
    let repo_root = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());

    let kind_filter: Option<Vec<chizu_core::EntityKind>> = args
        .kind
        .as_ref()
        .map(|k| k.split(',').filter_map(|s| s.trim().parse().ok()).collect());
    let exclude_patterns: Vec<String> = args
        .exclude
        .as_ref()
        .map(|e| e.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let seed_ids: Vec<String> = if let Some(ref start_id) = args.entity_id {
        vec![start_id.clone()]
    } else if store.get_entity("repo::.")?.is_some() {
        vec!["repo::.".to_string()]
    } else {
        store
            .get_all_entities()?
            .into_iter()
            .map(|e| e.id)
            .collect()
    };

    let traversal = chizu_core::graph_traversal(
        &store,
        &seed_ids,
        &chizu_core::TraversalOptions {
            max_depth: args.depth,
            max_nodes: args.max_nodes,
            kind_filter: kind_filter.as_deref(),
            exclude_patterns: &exclude_patterns,
        },
    )?;

    if traversal.entities.is_empty() {
        println!("No entities to visualize.");
        store.close()?;
        return Ok(());
    }

    let output_text = if args.interactive {
        let mut summary_cache: std::collections::HashMap<String, chizu_core::Summary> =
            std::collections::HashMap::new();
        for entity_id in traversal.entities.keys() {
            if let Some(summary) = store.get_summary(entity_id)? {
                summary_cache.insert(entity_id.clone(), summary);
            }
        }
        visualize::render_focus_graph_html(
            &traversal.entities,
            &summary_cache,
            &traversal.edges,
            &repo_root,
            config.visualize.editor_link.as_deref(),
            args.entity_id.as_deref(),
        )
    } else {
        visualize::render_focus_graph_svg(
            &traversal.entities,
            &traversal.edges,
            args.entity_id.as_deref(),
        )
    };

    if let Some(ref path) = args.output {
        std::fs::write(path, &output_text)?;
        println!("Wrote visualization to {}", path.display());
    } else {
        print!("{}", output_text);
    }

    store.close()?;
    Ok(())
}

fn truncate(s: &str, max_len: usize) -> std::borrow::Cow<'_, str> {
    if s.chars().count() <= max_len {
        s.into()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...").into()
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

    println!(
        "Indexed {} files ({} walked)",
        stats.files_indexed, stats.files_walked
    );
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
        eprintln!(
            "Warning: {} LLM operations failed; index is degraded.",
            failures
        );
    }

    store.close()?;
    Ok(())
}

fn build_provider(
    config: &chizu_core::Config,
) -> Result<Option<Box<dyn chizu_core::Provider>>, Box<dyn std::error::Error>> {
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

    let provider_config = config
        .providers
        .get(name)
        .ok_or_else(|| format!("Provider '{}' not found in config", name))?;

    let completion_model = config
        .summary
        .model
        .clone()
        .unwrap_or_else(|| "llama3:8b".to_string());
    let embedding_model = config
        .embedding
        .model
        .clone()
        .unwrap_or_else(|| "nomic-embed-text-v2-moe:latest".to_string());

    let provider =
        chizu_core::OpenAiProvider::new(provider_config, completion_model, embedding_model)
            .map_err(|e| format!("Failed to create provider: {e}"))?;

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
        eprintln!(
            "Warning: embeddings are configured but provider is unavailable; semantic search disabled."
        );
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

fn open_store(
    repo: &Path,
) -> Result<(chizu_core::Config, chizu_core::ChizuStore), Box<dyn std::error::Error>> {
    let config = load_config(repo)?;
    let store = chizu_core::ChizuStore::open(&repo.join(".chizu"), &config)?;
    Ok((config, store))
}

fn open_store_only(repo: &Path) -> Result<chizu_core::ChizuStore, Box<dyn std::error::Error>> {
    let (_config, store) = open_store(repo)?;
    Ok(store)
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
batch_size = 4
concurrency = 1

[embedding]
provider = "ollama"
model = "nomic-embed-text-v2-moe:latest"
dimensions = 768
batch_size = 32

[visualize]
# Example:
# editor_link = "vscode://file/{abs_path}:{line}:{column}"
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
    print!("{}", include_str!("../guide.md"));
    Ok(())
}
