//! Chizu CLI - Local code knowledge graph tool
//!
//! Usage: chizu [--repo <path>] <command>

use argh::FromArgs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum TaskCategory {
    Understand,
    Debug,
    Build,
    Test,
    Deploy,
    Configure,
    General,
}

impl std::str::FromStr for TaskCategory {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "understand" => Ok(Self::Understand),
            "debug" => Ok(Self::Debug),
            "build" => Ok(Self::Build),
            "test" => Ok(Self::Test),
            "deploy" => Ok(Self::Deploy),
            "configure" => Ok(Self::Configure),
            "general" => Ok(Self::General),
            _ => Err(format!(
                "unknown category '{s}': expected understand|debug|build|test|deploy|configure|general"
            )),
        }
    }
}

#[allow(dead_code)]
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
#[allow(dead_code)]
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "index")]
struct IndexArgs {
    /// force re-index all files
    #[argh(switch)]
    force: bool,
}

/// Search for entities and return a ranked reading plan
#[allow(dead_code)]
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
    category: Option<TaskCategory>,

    /// output format (text, json)
    #[argh(option, default = "OutputFormat::Text")]
    format: OutputFormat,
}

/// Look up a single entity by ID
#[allow(dead_code)]
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "entity")]
struct EntityArgs {
    /// entity ID (e.g., symbol::src/main.rs::main)
    #[argh(positional)]
    id: String,
}

/// List entities with optional filtering
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        Command::Index(_args) => not_yet_implemented("index"),
        Command::Search(_args) => not_yet_implemented("search"),
        Command::Entity(_args) => not_yet_implemented("entity"),
        Command::Entities(_args) => not_yet_implemented("entities"),
        Command::Routes(_args) => not_yet_implemented("routes"),
        Command::Edges(_args) => not_yet_implemented("edges"),
        Command::Visualize(_args) => not_yet_implemented("visualize"),
        Command::Config(args) => cmd_config(&cli.repo, args),
        Command::Guide(_) => cmd_guide(),
    }
}

fn not_yet_implemented(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!("'chizu {command}' is not yet implemented").into())
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

            let default_config = chizu_core::Config::default()
                .to_toml()
                .map_err(|e| format!("failed to serialize default config: {}", e))?;

            std::fs::write(&config_path, &default_config)?;
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
