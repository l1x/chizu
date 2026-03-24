use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use grakno_core::model::*;
use grakno_core::store::Store;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_entity(i: usize) -> Entity {
    Entity {
        id: format!("component::bench-{i}"),
        kind: EntityKind::Component,
        name: format!("bench-{i}"),
        component_id: None,
        path: Some(format!("crates/bench-{i}")),
        language: Some("rust".to_string()),
        line_start: None,
        line_end: None,
        visibility: Some("pub".to_string()),
        exported: true,
    }
}

fn make_edge(i: usize) -> Edge {
    Edge {
        src_id: format!("component::bench-{i}"),
        rel: EdgeKind::DependsOn,
        dst_id: format!("component::bench-{}", i + 1),
        provenance_path: None,
        provenance_line: None,
    }
}

fn make_embedding(i: usize, dims: usize) -> EmbeddingRecord {
    EmbeddingRecord {
        entity_id: format!("component::emb-{i}"),
        model: "bench-model".to_string(),
        dimensions: dims as i64,
        vector: (0..dims)
            .map(|j| ((i * dims + j) % 1000) as f32 / 1000.0)
            .collect(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

fn make_summary(i: usize) -> Summary {
    Summary {
        entity_id: format!("component::bench-{i}"),
        short_summary: format!("Summary for bench component {i}"),
        detailed_summary: None,
        keywords: vec!["bench".to_string(), format!("kw-{i}")],
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        source_hash: None,
    }
}

fn make_file(i: usize) -> FileRecord {
    FileRecord {
        path: format!("crates/bench-{i}/src/lib.rs"),
        component_id: Some(format!("component::bench-{i}")),
        kind: "rust".to_string(),
        hash: format!("sha256:bench{i:04}"),
        indexed: true,
        ignore_reason: None,
    }
}

/// Seed a store with `n` entities, `n-1` edges, and `n` embeddings (128-dim).
fn seed_store(store: &Store, n: usize) {
    for i in 0..n {
        store.insert_entity(&make_entity(i)).unwrap();
    }
    for i in 0..n.saturating_sub(1) {
        store.insert_edge(&make_edge(i)).unwrap();
    }
    for i in 0..n {
        store.upsert_embedding(&make_embedding(i, 128)).unwrap();
    }
}

/// Build a richer graph for traversal benchmarks.
///
/// Topology (n_components=10, symbols_per=10):
///   - 10 component entities in a DependsOn chain: comp-0 → comp-1 → … → comp-9
///   - Each component contains 10 symbol entities via Contains edges
///   - Each symbol has a summary
///   - Total: 110 entities, 10 chain edges + 100 contains edges, 100 summaries
fn seed_graph(store: &Store, n_components: usize, symbols_per: usize) {
    // Components
    for c in 0..n_components {
        store
            .insert_entity(&Entity {
                id: format!("component::comp-{c}"),
                kind: EntityKind::Component,
                name: format!("comp-{c}"),
                component_id: None,
                path: Some(format!("crates/comp-{c}")),
                language: Some("rust".to_string()),
                line_start: None,
                line_end: None,
                visibility: Some("pub".to_string()),
                exported: true,
            })
            .unwrap();
    }
    // DependsOn chain: comp-0 → comp-1 → … → comp-(n-1)
    for c in 0..n_components.saturating_sub(1) {
        store
            .insert_edge(&Edge {
                src_id: format!("component::comp-{c}"),
                rel: EdgeKind::DependsOn,
                dst_id: format!("component::comp-{}", c + 1),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();
    }
    // Symbols inside each component
    for c in 0..n_components {
        for s in 0..symbols_per {
            let sym_id = format!("symbol::comp-{c}::sym-{s}");
            store
                .insert_entity(&Entity {
                    id: sym_id.clone(),
                    kind: EntityKind::Symbol,
                    name: format!("Sym{s}"),
                    component_id: Some(format!("component::comp-{c}")),
                    path: Some(format!("crates/comp-{c}/src/sym_{s}.rs")),
                    language: Some("rust".to_string()),
                    line_start: Some(1),
                    line_end: Some(50),
                    visibility: Some("pub".to_string()),
                    exported: true,
                })
                .unwrap();
            // Contains edge
            store
                .insert_edge(&Edge {
                    src_id: format!("component::comp-{c}"),
                    rel: EdgeKind::Contains,
                    dst_id: sym_id.clone(),
                    provenance_path: None,
                    provenance_line: None,
                })
                .unwrap();
            // Summary
            store
                .upsert_summary(&Summary {
                    entity_id: sym_id,
                    short_summary: format!("Symbol {s} of component {c}"),
                    detailed_summary: None,
                    keywords: vec![format!("comp-{c}"), format!("sym-{s}")],
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                    source_hash: None,
                })
                .unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark functions
// ---------------------------------------------------------------------------

fn bench_insert_entities(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_entities");
    group.bench_function("sqlite", |b| {
        b.iter_batched(
            || Store::open_in_memory().unwrap(),
            |store| {
                for i in 0..100 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("grafeo", |b| {
        b.iter_batched(
            || Store::open_grafeo_in_memory().unwrap(),
            |store| {
                for i in 0..100 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_get_entity(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_entity");
    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.get_entity("component::bench-50").unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.get_entity("component::bench-50").unwrap());
    });
    group.finish();
}

fn bench_list_entities(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_entities");
    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.list_entities().unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.list_entities().unwrap());
    });
    group.finish();
}

fn bench_insert_edges(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_edges");
    group.bench_function("sqlite", |b| {
        b.iter_batched(
            || {
                let store = Store::open_in_memory().unwrap();
                for i in 0..101 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
                store
            },
            |store| {
                for i in 0..100 {
                    store.insert_edge(&make_edge(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("grafeo", |b| {
        b.iter_batched(
            || {
                let store = Store::open_grafeo_in_memory().unwrap();
                for i in 0..101 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
                store
            },
            |store| {
                for i in 0..100 {
                    store.insert_edge(&make_edge(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_edges_from(c: &mut Criterion) {
    let mut group = c.benchmark_group("edges_from");
    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.edges_from("component::bench-0").unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.edges_from("component::bench-0").unwrap());
    });
    group.finish();
}

fn bench_edges_to(c: &mut Criterion) {
    let mut group = c.benchmark_group("edges_to");
    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.edges_to("component::bench-50").unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.edges_to("component::bench-50").unwrap());
    });
    group.finish();
}

fn bench_upsert_embedding(c: &mut Criterion) {
    let mut group = c.benchmark_group("upsert_embedding");
    group.bench_function("sqlite", |b| {
        b.iter_batched(
            || Store::open_in_memory().unwrap(),
            |store| {
                for i in 0..50 {
                    store.upsert_embedding(&make_embedding(i, 128)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("grafeo", |b| {
        b.iter_batched(
            || Store::open_grafeo_in_memory().unwrap(),
            |store| {
                for i in 0..50 {
                    store.upsert_embedding(&make_embedding(i, 128)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_vector_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_search");
    let query: Vec<f32> = (0..128).map(|j| j as f32 / 128.0).collect();

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        for i in 0..500 {
            store.upsert_embedding(&make_embedding(i, 128)).unwrap();
        }
        b.iter(|| store.vector_search(&query, 10).unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        for i in 0..500 {
            store.upsert_embedding(&make_embedding(i, 128)).unwrap();
        }
        b.iter(|| store.vector_search(&query, 10).unwrap());
    });
    group.finish();
}

fn bench_upsert_summary(c: &mut Criterion) {
    let mut group = c.benchmark_group("upsert_summary");
    group.bench_function("sqlite", |b| {
        b.iter_batched(
            || {
                let store = Store::open_in_memory().unwrap();
                for i in 0..100 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
                store
            },
            |store| {
                for i in 0..100 {
                    store.upsert_summary(&make_summary(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("grafeo", |b| {
        b.iter_batched(
            || {
                let store = Store::open_grafeo_in_memory().unwrap();
                for i in 0..100 {
                    store.insert_entity(&make_entity(i)).unwrap();
                }
                store
            },
            |store| {
                for i in 0..100 {
                    store.upsert_summary(&make_summary(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_insert_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_file");
    group.bench_function("sqlite", |b| {
        b.iter_batched(
            || Store::open_in_memory().unwrap(),
            |store| {
                for i in 0..100 {
                    store.insert_file(&make_file(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.bench_function("grafeo", |b| {
        b.iter_batched(
            || Store::open_grafeo_in_memory().unwrap(),
            |store| {
                for i in 0..100 {
                    store.insert_file(&make_file(i)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats");
    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.stats().unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_store(&store, 100);
        b.iter(|| store.stats().unwrap());
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Graph-traversal benchmarks
// ---------------------------------------------------------------------------

/// Parameterized multi-hop traversal with varying depths
fn bench_multi_hop_depths(c: &mut Criterion) {
    for depth in [5, 10, 20] {
        let mut group = c.benchmark_group(format!("multi_hop_depth_{depth}"));

        group.bench_function("sqlite", |b| {
            let store = Store::open_in_memory().unwrap();
            seed_graph(&store, 50, 5); // 50 components, chain of 50
            b.iter(|| {
                store
                    .walk_forward("component::comp-0", EdgeKind::DependsOn, depth)
                    .unwrap()
            });
        });

        group.bench_function("grafeo", |b| {
            let store = Store::open_grafeo_in_memory().unwrap();
            seed_graph(&store, 50, 5);
            b.iter(|| {
                store
                    .walk_forward("component::comp-0", EdgeKind::DependsOn, depth)
                    .unwrap()
            });
        });
        group.finish();
    }
}

/// Walk a DependsOn chain: comp-0 → comp-1 → … → comp-9 (10 hops).
/// Uses native walk_forward for single-query traversal.
fn bench_multi_hop_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_hop_traversal_10");

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_graph(&store, 20, 10);
        b.iter(|| {
            store
                .walk_forward("component::comp-0", EdgeKind::DependsOn, 10)
                .unwrap()
        });
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_graph(&store, 20, 10);
        b.iter(|| {
            store
                .walk_forward("component::comp-0", EdgeKind::DependsOn, 10)
                .unwrap()
        });
    });
    group.finish();
}

/// Fan-out: from one component, collect all symbols it Contains, then
/// fetch each symbol entity. Measures edges_from + N × get_entity.
fn bench_fan_out(c: &mut Criterion) {
    let mut group = c.benchmark_group("fan_out_contains");

    fn fan_out(store: &Store, component: &str) -> Vec<Entity> {
        let edges = store.edges_from(component).unwrap();
        edges
            .iter()
            .filter(|e| e.rel == EdgeKind::Contains)
            .map(|e| store.get_entity(&e.dst_id).unwrap())
            .collect()
    }

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| fan_out(&store, "component::comp-0"));
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| fan_out(&store, "component::comp-0"));
    });
    group.finish();
}

/// Reverse dependency chain: start at comp-19, walk backwards via walk_backward
/// until we reach comp-0. Mirrors how "who depends on me?" is resolved.
fn bench_reverse_dep_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("reverse_dep_chain_10");

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_graph(&store, 20, 10);
        b.iter(|| {
            store
                .walk_backward("component::comp-19", EdgeKind::DependsOn, 10)
                .unwrap()
        });
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_graph(&store, 20, 10);
        b.iter(|| {
            store
                .walk_backward("component::comp-19", EdgeKind::DependsOn, 10)
                .unwrap()
        });
    });
    group.finish();
}

/// Parameterized reverse traversal with varying depths
fn bench_reverse_hop_depths(c: &mut Criterion) {
    for depth in [5, 10, 20] {
        let mut group = c.benchmark_group(format!("reverse_hop_depth_{depth}"));

        group.bench_function("sqlite", |b| {
            let store = Store::open_in_memory().unwrap();
            seed_graph(&store, 50, 5);
            let start = format!("component::comp-{}", depth - 1);
            b.iter(|| {
                store
                    .walk_backward(&start, EdgeKind::DependsOn, depth)
                    .unwrap()
            });
        });

        group.bench_function("grafeo", |b| {
            let store = Store::open_grafeo_in_memory().unwrap();
            seed_graph(&store, 50, 5);
            let start = format!("component::comp-{}", depth - 1);
            b.iter(|| {
                store
                    .walk_backward(&start, EdgeKind::DependsOn, depth)
                    .unwrap()
            });
        });
        group.finish();
    }
}

/// 2-hop neighborhood: from comp-5, find all entities reachable within 2 hops.
/// Uses native reachable_entities for single-query traversal.
fn bench_neighborhood_2hop(c: &mut Criterion) {
    let mut group = c.benchmark_group("neighborhood_2hop");

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| store.reachable_entities("component::comp-5", 2).unwrap());
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| store.reachable_entities("component::comp-5", 2).unwrap());
    });
    group.finish();
}

/// Parameterized reachable entities with varying depths
fn bench_reachable_depths(c: &mut Criterion) {
    for depth in [2, 3, 5] {
        let mut group = c.benchmark_group(format!("reachable_depth_{depth}"));

        group.bench_function("sqlite", |b| {
            let store = Store::open_in_memory().unwrap();
            seed_graph(&store, 30, 10); // More components for deeper reach
            b.iter(|| {
                store
                    .reachable_entities("component::comp-0", depth)
                    .unwrap()
            });
        });

        group.bench_function("grafeo", |b| {
            let store = Store::open_grafeo_in_memory().unwrap();
            seed_graph(&store, 30, 10);
            b.iter(|| {
                store
                    .reachable_entities("component::comp-0", depth)
                    .unwrap()
            });
        });
        group.finish();
    }
}

/// Component subgraph: given a component, load all its symbols via
/// list_entities_by_component, then fetch each symbol's summary and
/// outgoing edges. This is the "load everything about a component" query.
fn bench_component_subgraph(c: &mut Criterion) {
    let mut group = c.benchmark_group("component_subgraph");

    fn load_subgraph(
        store: &Store,
        component_id: &str,
    ) -> (Vec<Entity>, Vec<Summary>, Vec<Vec<Edge>>) {
        let symbols = store.list_entities_by_component(component_id).unwrap();
        let mut summaries = Vec::with_capacity(symbols.len());
        let mut all_edges = Vec::with_capacity(symbols.len());
        for sym in &symbols {
            summaries.push(store.get_summary(&sym.id).unwrap());
            all_edges.push(store.edges_from(&sym.id).unwrap());
        }
        (symbols, summaries, all_edges)
    }

    group.bench_function("sqlite", |b| {
        let store = Store::open_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| load_subgraph(&store, "component::comp-0"));
    });
    group.bench_function("grafeo", |b| {
        let store = Store::open_grafeo_in_memory().unwrap();
        seed_graph(&store, 10, 10);
        b.iter(|| load_subgraph(&store, "component::comp-0"));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_insert_entities,
    bench_get_entity,
    bench_list_entities,
    bench_insert_edges,
    bench_edges_from,
    bench_edges_to,
    bench_upsert_embedding,
    bench_vector_search,
    bench_upsert_summary,
    bench_insert_file,
    bench_stats,
    bench_multi_hop_traversal,
    bench_multi_hop_depths,
    bench_fan_out,
    bench_reverse_dep_chain,
    bench_reverse_hop_depths,
    bench_neighborhood_2hop,
    bench_reachable_depths,
    bench_component_subgraph,
);
criterion_main!(benches);
