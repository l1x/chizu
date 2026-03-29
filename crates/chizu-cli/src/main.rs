//! Chizu CLI - Local code knowledge graph tool
//!
//! Usage: chizu [--repo <path>] <command>

use argh::FromArgs;
use std::path::{Path, PathBuf};

/// Chizu - Local repository understanding engine
#[derive(FromArgs, Debug)]
struct Cli {
    /// repository path (defaults to current directory)
    #[argh(option, short = 'r', default = "PathBuf::from(\".\")")]
    repo: PathBuf,

    /// subcommand to run
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
    category: Option<String>,

    /// output format (text, json)
    #[argh(option, default = "String::from(\"text\")")]
    format: String,
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
    kind: Option<String>,
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
    rel: Option<String>,
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

    /// filter by entity kind
    #[argh(option)]
    _kind: Option<String>,

    /// exclude patterns (comma-separated)
    #[argh(option)]
    _exclude: Option<String>,

    /// layout algorithm (dot, neato, fdp)
    #[argh(option, default = "String::from(\"dot\")")]
    layout: String,

    /// maximum number of nodes
    #[argh(option, default = "100")]
    max_nodes: usize,

    /// output file path
    #[argh(option, short = 'o')]
    output: Option<PathBuf>,

    /// include legend
    #[argh(switch)]
    legend: bool,
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

fn cmd_index(repo: &Path, args: IndexArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Indexing repository: {}", repo.display());
    if args.force {
        println!("Force re-index enabled");
    }

    // TODO: Implement indexing pipeline
    // 1. Load config
    // 2. Discover components
    // 3. Extract entities and edges
    // 4. Generate summaries
    // 5. Generate embeddings

    println!("Index complete!");
    Ok(())
}

fn cmd_search(repo: &Path, args: SearchArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Searching in {}: {}", repo.display(), args.query);
    println!("Limit: {}, Format: {}", args.limit, args.format);

    if let Some(category) = args.category {
        println!("Category: {}", category);
    }

    // TODO: Implement search pipeline
    // 1. Classify query
    // 2. Retrieve candidates
    // 3. Expand graph
    // 4. Rerank
    // 5. Output reading plan

    Ok(())
}

fn cmd_entity(repo: &Path, args: EntityArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Looking up entity in {}: {}", repo.display(), args.id);

    // TODO: Query entity by ID and display details

    Ok(())
}

fn cmd_entities(repo: &Path, args: EntitiesArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Listing entities in {}", repo.display());

    if let Some(component) = args.component {
        println!("Component filter: {}", component);
    }
    if let Some(kind) = args.kind {
        println!("Kind filter: {}", kind);
    }

    // TODO: Query and list entities

    Ok(())
}

fn cmd_routes(repo: &Path, args: RoutesArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Listing task routes in {}", repo.display());

    if let Some(task) = args.task {
        println!("Task filter: {}", task);
    }
    if let Some(entity) = args.entity {
        println!("Entity filter: {}", entity);
    }

    // TODO: Query and list task routes

    Ok(())
}

fn cmd_edges(repo: &Path, args: EdgesArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Listing edges in {}", repo.display());

    if let Some(from) = args.from {
        println!("From filter: {}", from);
    }
    if let Some(to) = args.to {
        println!("To filter: {}", to);
    }
    if let Some(rel) = args.rel {
        println!("Relationship filter: {}", rel);
    }

    // TODO: Query and list edges

    Ok(())
}

fn cmd_visualize(repo: &Path, args: VisualizeArgs) -> Result<(), Box<dyn std::error::Error>> {
    println!("Generating visualization for {}", repo.display());
    println!(
        "Depth: {}, Layout: {}, Max nodes: {}",
        args.depth, args.layout, args.max_nodes
    );

    if let Some(entity_id) = args.entity_id {
        println!("Starting from: {}", entity_id);
    }
    if let Some(output) = args.output {
        println!("Output to: {}", output.display());
    }
    if args.legend {
        println!("Including legend");
    }

    // TODO: Generate SVG visualization

    Ok(())
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

            let default_config = r#"[index]
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

            std::fs::write(&config_path, default_config)?;
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
