mod cli;

use cli::{Command, QuerySub, SummarizeCmd, TopLevel};
use grakno_core::Store;

fn main() {
    let args: TopLevel = argh::from_env();

    let store = Store::open(&args.db).unwrap_or_else(|e| {
        eprintln!("error: failed to open store at {}: {e}", args.db);
        std::process::exit(1);
    });

    match args.command {
        Command::Index(cmd) => {
            let path = std::path::Path::new(&cmd.path);
            match grakno_index::index_project(&store, path) {
                Ok(stats) => {
                    println!("indexed successfully:\n{stats}");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Query(q) => match q.sub {
            QuerySub::Entity(cmd) => cmd_query_entity(&store, &cmd.id),
            QuerySub::Entities(cmd) => cmd_query_entities(&store, cmd.component.as_deref()),
            QuerySub::Routes(cmd) => {
                cmd_query_routes(&store, cmd.task.as_deref(), cmd.entity.as_deref())
            }
        },
        Command::Inspect(cmd) => match cmd.entity_id {
            Some(ref id) => cmd_inspect_entity(&store, id),
            None => cmd_inspect_overview(&store),
        },
        Command::Summarize(cmd) => cmd_summarize(&store, cmd),
    }
}

fn cmd_summarize(store: &Store, cmd: SummarizeCmd) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let config = grakno_summarize::SummarizeConfig {
        max_tokens: cmd.max_tokens,
        temperature: cmd.temperature,
        ..config
    };
    let options = grakno_summarize::summarizer::SummarizeOptions {
        component: cmd.component,
        force: cmd.force,
    };

    match grakno_summarize::summarize_graph(store, &config, &options) {
        Ok(stats) => {
            println!("summarization complete:\n{stats}");
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_entity(store: &Store, id: &str) {
    match store.get_entity(id) {
        Ok(e) => {
            println!("id:        {}", e.id);
            println!("kind:      {}", e.kind);
            println!("name:      {}", e.name);
            if let Some(ref c) = e.component_id {
                println!("component: {c}");
            }
            if let Some(ref p) = e.path {
                println!("path:      {p}");
            }
            if let Some(ref lang) = e.language {
                println!("language:  {lang}");
            }
            if let Some(start) = e.line_start {
                let end = e.line_end.map(|n| n.to_string()).unwrap_or_default();
                println!("lines:     {start}..{end}");
            }
            if let Some(ref vis) = e.visibility {
                println!("visibility: {vis}");
            }
            println!("exported:  {}", e.exported);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_entities(store: &Store, component: Option<&str>) {
    let entities = match component {
        Some(c) => store.list_entities_by_component(c),
        None => store.list_entities(),
    };
    match entities {
        Ok(list) if list.is_empty() => println!("no entities found"),
        Ok(list) => {
            for e in &list {
                println!("{:<12} {}", e.kind, e.id);
            }
            println!("\n{} entities", list.len());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_routes(store: &Store, task: Option<&str>, entity: Option<&str>) {
    let routes = match (task, entity) {
        (Some(t), _) => store.routes_for_task(t),
        (_, Some(e)) => store.routes_for_entity(e),
        (None, None) => {
            eprintln!("error: provide --task or --entity");
            std::process::exit(1);
        }
    };
    match routes {
        Ok(list) if list.is_empty() => println!("no routes found"),
        Ok(list) => {
            for r in &list {
                println!(
                    "task={:<16} entity={:<32} priority={}",
                    r.task_name, r.entity_id, r.priority
                );
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_inspect_overview(store: &Store) {
    let version = store.schema_version().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let stats = store.stats().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    println!("grakno graph (schema v{version})");
    println!("  entities:    {}", stats.entities);
    println!("  edges:       {}", stats.edges);
    println!("  files:       {}", stats.files);
    println!("  summaries:   {}", stats.summaries);
    println!("  task_routes: {}", stats.task_routes);
    println!("  embeddings:  {}", stats.embeddings);
}

fn cmd_inspect_entity(store: &Store, id: &str) {
    let entity = match store.get_entity(id) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== {} ({}) ===", entity.name, entity.kind);
    println!("id: {}", entity.id);
    if let Some(ref p) = entity.path {
        println!("path: {p}");
    }

    let outgoing = store.edges_from(id).unwrap_or_default();
    let incoming = store.edges_to(id).unwrap_or_default();

    if !outgoing.is_empty() {
        println!("\noutgoing edges ({}):", outgoing.len());
        for e in &outgoing {
            println!("  --[{}]--> {}", e.rel, e.dst_id);
        }
    }

    if !incoming.is_empty() {
        println!("\nincoming edges ({}):", incoming.len());
        for e in &incoming {
            println!("  <--[{}]-- {}", e.rel, e.src_id);
        }
    }

    if let Ok(s) = store.get_summary(id) {
        println!("\nsummary: {}", s.short_summary);
        if !s.keywords.is_empty() {
            println!("keywords: {}", s.keywords.join(", "));
        }
    }
}
