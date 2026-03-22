use argh::FromArgs;

/// Grakno — a code knowledge graph
#[derive(FromArgs)]
pub struct TopLevel {
    /// path to the SQLite database (default: grakno.db)
    #[argh(option, default = "String::from(\"grakno.db\")")]
    pub db: String,

    #[argh(subcommand)]
    pub command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Index(IndexCmd),
    Query(QueryCmd),
    Inspect(InspectCmd),
    Summarize(SummarizeCmd),
}

/// index a Rust workspace into the graph
#[derive(FromArgs)]
#[argh(subcommand, name = "index")]
pub struct IndexCmd {
    /// path to the workspace root (default: current directory)
    #[argh(positional, default = "String::from(\".\")")]
    pub path: String,
}

/// query the graph
#[derive(FromArgs)]
#[argh(subcommand, name = "query")]
pub struct QueryCmd {
    #[argh(subcommand)]
    pub sub: QuerySub,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum QuerySub {
    Entity(QueryEntityCmd),
    Entities(QueryEntitiesCmd),
    Routes(QueryRoutesCmd),
}

/// look up a single entity by id
#[derive(FromArgs)]
#[argh(subcommand, name = "entity")]
pub struct QueryEntityCmd {
    /// the entity id
    #[argh(positional)]
    pub id: String,
}

/// list entities
#[derive(FromArgs)]
#[argh(subcommand, name = "entities")]
pub struct QueryEntitiesCmd {
    /// filter by component id
    #[argh(option)]
    pub component: Option<String>,
}

/// list task routes
#[derive(FromArgs)]
#[argh(subcommand, name = "routes")]
pub struct QueryRoutesCmd {
    /// routes for a task name
    #[argh(option)]
    pub task: Option<String>,

    /// routes for an entity id
    #[argh(option)]
    pub entity: Option<String>,
}

/// inspect the graph
#[derive(FromArgs)]
#[argh(subcommand, name = "inspect")]
pub struct InspectCmd {
    /// entity id to inspect in detail
    #[argh(positional)]
    pub entity_id: Option<String>,
}

/// summarize entities using an LLM
#[derive(FromArgs)]
#[argh(subcommand, name = "summarize")]
pub struct SummarizeCmd {
    /// base URL for the OpenAI-compatible API
    #[argh(option)]
    pub base_url: String,

    /// API key for authentication
    #[argh(option)]
    pub api_key: String,

    /// model identifier (e.g. gpt-4o-mini)
    #[argh(option)]
    pub model: String,

    /// maximum tokens in the response (default: 512)
    #[argh(option, default = "512")]
    pub max_tokens: u32,

    /// sampling temperature (default: 0.2)
    #[argh(option, default = "0.2")]
    pub temperature: f32,

    /// only summarize this component
    #[argh(option)]
    pub component: Option<String>,

    /// re-summarize even if up to date
    #[argh(switch)]
    pub force: bool,
}
