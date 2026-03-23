mod cli;

use cli::{Command, EmbedCmd, PlanCmd, QuerySub, SearchCmd, SummarizeCmd, TopLevel, WatchCmd};
use grakno_core::Store;

fn main() {
    let args: TopLevel = argh::from_env();

    let store = open_store(&args.backend, &args.db);

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
        Command::Embed(cmd) => cmd_embed(&store, cmd),
        Command::Search(cmd) => cmd_search(&store, cmd),
        Command::Watch(cmd) => cmd_watch(&store, cmd),
        Command::Plan(cmd) => cmd_plan(&store, cmd),
    }
}

fn open_store(backend: &str, db: &str) -> Store {
    match backend {
        "sqlite" => Store::open(db).unwrap_or_else(|e| {
            eprintln!("error: failed to open sqlite store at {db}: {e}");
            std::process::exit(1);
        }),
        "grafeo" => {
            #[cfg(feature = "grafeo")]
            {
                Store::open_grafeo(db).unwrap_or_else(|e| {
                    eprintln!("error: failed to open grafeo store at {db}: {e}");
                    std::process::exit(1);
                })
            }
            #[cfg(not(feature = "grafeo"))]
            {
                eprintln!("error: grafeo backend not available; rebuild with --features grafeo");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("error: unknown backend '{other}'; expected 'sqlite' or 'grafeo'");
            std::process::exit(1);
        }
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
        workspace_root: Some(std::env::current_dir().unwrap_or_default()),
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

    match version {
        Some(v) => println!("grakno graph (schema v{v})"),
        None => println!("grakno graph (grafeo backend)"),
    }
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

fn cmd_embed(store: &Store, cmd: EmbedCmd) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let client = match grakno_summarize::EmbeddingClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let options = grakno_summarize::EmbedOptions {
        component: cmd.component,
        force: cmd.force,
    };

    match grakno_summarize::embedding::embed_graph(store, &client, &options) {
        Ok(stats) => {
            println!("embedding complete:\n{stats}");
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_watch(store: &Store, cmd: WatchCmd) {
    use notify::{Event, RecursiveMode, Watcher};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let root = Path::new(&cmd.path).canonicalize().unwrap_or_else(|e| {
        eprintln!("error: invalid path '{}': {e}", cmd.path);
        std::process::exit(1);
    });

    // Initial index
    println!("running initial index of {}…", root.display());
    match grakno_index::index_project(store, &root) {
        Ok(stats) => println!("initial index complete:\n{stats}"),
        Err(e) => {
            eprintln!("error during initial index: {e}");
            std::process::exit(1);
        }
    }

    let debounce = Duration::from_millis(cmd.debounce_ms);
    let (tx, rx) = mpsc::channel::<Event>();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .unwrap_or_else(|e| {
        eprintln!("error: failed to create watcher: {e}");
        std::process::exit(1);
    });

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .unwrap_or_else(|e| {
            eprintln!("error: failed to watch '{}': {e}", root.display());
            std::process::exit(1);
        });

    println!(
        "watching {} (debounce {}ms, Ctrl+C to stop)",
        root.display(),
        cmd.debounce_ms
    );

    let relevant_ext = |p: &Path| -> bool {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("rs" | "toml" | "md")
        )
    };

    let ignored = |p: &Path| -> bool {
        for component in p.components() {
            let s = component.as_os_str().to_string_lossy();
            if s == "target" || s == ".git" {
                return true;
            }
        }
        matches!(p.extension().and_then(|e| e.to_str()), Some("db"))
    };

    loop {
        // Block until we get the first relevant event
        loop {
            match rx.recv() {
                Ok(event) => {
                    if event.paths.iter().any(|p| !ignored(p) && relevant_ext(p)) {
                        break;
                    }
                }
                Err(_) => {
                    // Channel closed, watcher dropped
                    return;
                }
            }
        }

        // Debounce: drain events for the debounce window
        let deadline = Instant::now() + debounce;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(_) => {} // coalesce
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        println!("\nchange detected, re-indexing…");
        match grakno_index::index_project(store, &root) {
            Ok(stats) => println!("re-index complete:\n{stats}"),
            Err(e) => eprintln!("re-index error: {e}"),
        }
    }
}

fn cmd_plan(store: &Store, cmd: PlanCmd) {
    let config = grakno_query::PipelineConfig {
        limit: cmd.limit,
        ..Default::default()
    };

    // Optionally embed the query if all three embedding options are provided
    let query_embedding = match (&cmd.base_url, &cmd.api_key, &cmd.model) {
        (Some(base_url), Some(api_key), Some(model)) => {
            let embed_config = grakno_summarize::SummarizeConfig::new(
                base_url.clone(),
                api_key.clone(),
                model.clone(),
            );
            match grakno_summarize::EmbeddingClient::new(&embed_config) {
                Ok(client) => match client.embed(&[&cmd.query]) {
                    Ok(mut vecs) if !vecs.is_empty() => Some(vecs.remove(0)),
                    Ok(_) => {
                        eprintln!(
                            "warning: embedding returned no vectors, falling back to keyword-only"
                        );
                        None
                    }
                    Err(e) => {
                        eprintln!("warning: embedding failed ({e}), falling back to keyword-only");
                        None
                    }
                },
                Err(e) => {
                    eprintln!("warning: failed to create embedding client ({e}), falling back to keyword-only");
                    None
                }
            }
        }
        _ => None,
    };

    let embedding_ref = query_embedding.as_deref();

    match grakno_query::QueryPipeline::run(store, &cmd.query, embedding_ref, &config) {
        Ok(plan) => match cmd.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&plan).unwrap_or_else(|e| {
                    eprintln!("error serializing plan: {e}");
                    std::process::exit(1);
                });
                println!("{json}");
            }
            _ => {
                print!("{}", plan.display());
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_search(store: &Store, cmd: SearchCmd) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let client = match grakno_summarize::EmbeddingClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    match grakno_summarize::embedding::search(store, &client, &cmd.query, cmd.k) {
        Ok(results) if results.is_empty() => {
            println!("no results found");
        }
        Ok(results) => {
            for (i, r) in results.iter().enumerate() {
                println!("{}. {}  (distance: {:.3})", i + 1, r.entity_id, r.distance);
                let location = match (&r.path, r.line_start) {
                    (Some(p), Some(l)) => format!("{p}:{l}"),
                    (Some(p), None) => p.clone(),
                    _ => String::new(),
                };
                if !location.is_empty() {
                    println!("   [{}] {}", r.entity_kind, location);
                } else {
                    println!("   [{}]", r.entity_kind);
                }
                if !r.short_summary.is_empty() {
                    println!("   {}", r.short_summary);
                }
                println!();
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
