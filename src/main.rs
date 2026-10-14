//! Interface de linha de comando do ConectaStore.
//!
//! Comandos: `demo` (padrão), `bench`, `quality`, `repl` e `ajuda`.

use std::io::{self, BufRead, Write};

use megastore::bench::{fmt_bytes, fmt_duration, run_scenario, BenchRow};
use megastore::datagen::{generate, DatasetSpec};
use megastore::eval::evaluate;
use megastore::{sample, MegaStore, RecommendConfig, Recommendation};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("demo") => demo(),
        Some("bench") => bench_command(&args[1..]),
        Some("quality") => quality_command(&args[1..]),
        Some("repl") => repl(),
        Some("ajuda") | Some("help") | Some("--help") | Some("-h") => print_help(),
        Some(other) => {
            eprintln!("comando desconhecido: {other}\n");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("ConectaStore: sistema de recomendação de produtos baseado em grafos");
    println!();
    println!("Uso: megastore [comando]");
    println!();
    println!("Comandos:");
    println!("  demo                     demonstração com um cenário pequeno (padrão)");
    println!("  bench [produtos...]      mede tempos com volumes crescentes (padrão: 1000 10000 100000)");
    println!("  quality [produtos]       compara a relevância com os mais vendidos (padrão: 10000)");
    println!("  repl                     console interativo para cadastrar, comprar e recomendar");
    println!("  ajuda                    mostra esta mensagem");
}

// ---------------------------------------------------------------------------------------------
// Demonstração
// ---------------------------------------------------------------------------------------------

fn print_recommendations(store: &MegaStore, recs: &[Recommendation]) {
    if recs.is_empty() {
        println!("  (nenhuma recomendação encontrada)");
        return;
    }
    for (i, rec) in recs.iter().enumerate() {
        match store.catalog().product(rec.product_id) {
            Some(p) => {
                let category = store
                    .catalog()
                    .category(p.category_id)
                    .map(|c| c.name.as_str())
                    .unwrap_or("?");
                println!(
                    "  {:>2}. #{:<3} {} [{}] {}  (score {:.3})",
                    i + 1,
                    p.id,
                    p.name,
                    category,
                    p.price_display(),
                    rec.score
                );
            }
            None => println!("  {:>2}. #{} (produto fora do catálogo)", i + 1, rec.product_id),
        }
    }
}

fn product_names(store: &MegaStore, ids: &[u32]) -> String {
    let names: Vec<String> = ids
        .iter()
        .map(|&id| {
            store
                .catalog()
                .product(id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| format!("#{id}"))
        })
        .collect();
    names.join(", ")
}

fn print_stats(store: &MegaStore) {
    let s = store.stats();
    println!(
        "Catálogo: {} categorias, {} produtos, {} clientes",
        s.categories, s.products, s.customers
    );
    println!(
        "Grafo: {} vértices, {} arestas ({} de compra + {} de categoria)",
        s.vertices,
        s.purchase_edges + s.membership_edges,
        s.purchase_edges,
        s.membership_edges
    );
}

fn demo() {
    let mut store = sample::demo_store();
    let cfg = RecommendConfig {
        top_k: 5,
        ..RecommendConfig::default()
    };

    println!("=== ConectaStore: demonstração ===");
    print_stats(&store);

    println!("\n[1] Consulta de produtos: nome contém \"mouse\"");
    for p in store.catalog().search_products("mouse") {
        println!("  #{} {} {}", p.id, p.name, p.price_display());
    }

    println!("\n[2] Recomendações para o cliente #1 (Ana)");
    let owned = store.purchased_products(1).unwrap_or_default();
    println!("  Já comprou: {}", product_names(&store, &owned));
    match store.recommend_for_customer(1, &cfg) {
        Ok(recs) => print_recommendations(&store, &recs),
        Err(e) => println!("  erro: {e}"),
    }

    println!("\n[3] Produtos relacionados ao produto #1 (Notebook Gamer)");
    match store.recommend_for_product(1, &cfg) {
        Ok(recs) => print_recommendations(&store, &recs),
        Err(e) => println!("  erro: {e}"),
    }

    println!("\n[4] Por que o produto #3 (Teclado Mecânico) aparece para a Ana? Caminho no grafo (BFS):");
    match store.explain(1, 3, 6) {
        Ok(Some(path)) => {
            let steps: Vec<String> = path.iter().map(|s| s.to_string()).collect();
            println!("  {}", steps.join(" -> "));
        }
        Ok(None) => println!("  nenhum caminho dentro do limite"),
        Err(e) => println!("  erro: {e}"),
    }

    println!("\n[5] Prevenção de duplicatas");
    match store.add_product(3, "Teclado Clone", 1, 1_000) {
        Err(e) => println!("  cadastrar o produto #3 de novo -> erro: {e}"),
        Ok(()) => println!("  (inesperado) produto duplicado foi aceito"),
    }
    let before = store.stats().purchase_edges;
    match store.record_purchase(1, 1, Some(5)) {
        Ok(false) => println!("  registrar de novo a compra Ana/Notebook -> conexão existente apenas atualizada"),
        Ok(true) => println!("  (inesperado) criou uma conexão nova"),
        Err(e) => println!("  erro: {e}"),
    }
    println!(
        "  arestas de compra antes/depois: {}/{}",
        before,
        store.stats().purchase_edges
    );
    println!("\nPara explorar: `cargo run --release -- repl`");
}

// ---------------------------------------------------------------------------------------------
// Benchmark e avaliação de relevância
// ---------------------------------------------------------------------------------------------

fn print_bench_header() {
    println!("| Produtos | Clientes | Compras (arestas) | Vértices | Carga (gerar + inserir) | Ordenar adjacências | Rec. cliente média / p95 | Rec. produto média / p95 | Grafo (estimado) | Matriz equivalente | RSS do processo |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
}

fn print_bench_row(row: &BenchRow) {
    let rss = match row.rss_kb {
        Some(kb) => fmt_bytes(kb as u128 * 1024),
        None => "n/d".to_string(),
    };
    println!(
        "| {} | {} | {} | {} | {} | {} | {} / {} | {} / {} | {} | {} | {} |",
        row.products,
        row.customers,
        row.purchases,
        row.vertices,
        fmt_duration(row.load_time),
        fmt_duration(row.optimize_time),
        fmt_duration(row.customer_queries.mean),
        fmt_duration(row.customer_queries.p95),
        fmt_duration(row.product_queries.mean),
        fmt_duration(row.product_queries.p95),
        fmt_bytes(row.graph_bytes as u128),
        fmt_bytes(row.matrix_bytes),
        rss
    );
}

fn bench_command(args: &[String]) {
    let mut sizes: Vec<usize> = args.iter().filter_map(|a| a.parse().ok()).collect();
    if sizes.is_empty() {
        sizes = vec![1_000, 10_000, 100_000];
    }
    sizes.sort_unstable();

    println!("Benchmark do ConectaStore (use `cargo run --release` para medidas realistas)");
    println!("Cada cenário: N produtos, N clientes, 20 categorias, 10 compras por cliente; 1000 consultas de cada tipo.\n");
    print_bench_header();
    for products in sizes {
        let row = run_scenario(products, 1_000);
        print_bench_row(&row);
        if row.avg_results < 1.0 {
            eprintln!("aviso: média de {:.1} resultados por consulta no cenário de {} produtos", row.avg_results, products);
        }
    }
}

fn quality_command(args: &[String]) {
    let products: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let mut spec = DatasetSpec::scaled(products, 7);
    spec.holdout_every = 20;

    println!("Avaliação de relevância em dados sintéticos ({products} produtos, {products} clientes)");
    let mut dataset = generate(&spec);
    dataset.store.optimize();
    let report = evaluate(&dataset, 10, 500);

    println!(
        "Compras retidas avaliadas: {}. Taxa de acerto no top-{} (hit rate):\n",
        report.evaluated, report.k
    );
    println!("| Estratégia | Hit rate @{} |", report.k);
    println!("|---|---:|");
    println!("| Mais vendidos (sistema atual) | {:.1}% |", report.best_sellers * 100.0);
    println!("| Mais vendidos da mesma categoria | {:.1}% |", report.same_category * 100.0);
    println!("| ConectaStore (grafo + BFS) | {:.1}% |", report.graph * 100.0);
}

// ---------------------------------------------------------------------------------------------
// Console interativo
// ---------------------------------------------------------------------------------------------

fn print_repl_help() {
    println!("Comandos:");
    println!("  demo                                             carrega o cenário de exemplo");
    println!("  categoria <id> <nome>                            cadastra uma categoria");
    println!("  produto <id> <categoria> <preço em centavos> <nome>   cadastra um produto");
    println!("  cliente <id> <nome>                              cadastra um cliente");
    println!("  comprar <cliente> <produto> [nota 1-5]           registra (ou atualiza) uma compra");
    println!("  cancelar <cliente> <produto>                     remove uma compra");
    println!("  buscar <termo>                                   busca produtos pelo nome");
    println!("  ver <produto>                                    mostra os dados de um produto");
    println!("  recomendar-cliente <id> [k]                      recomenda produtos para um cliente");
    println!("  recomendar-produto <id> [k]                      recomenda produtos relacionados");
    println!("  explicar <cliente> <produto>                     caminho no grafo (BFS) entre os dois");
    println!("  stats                                            tamanho do catálogo e do grafo");
    println!("  ajuda | sair");
}

fn arg_u32(parts: &[&str], index: usize, usage: &str) -> Result<u32, String> {
    parts
        .get(index)
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(|| format!("uso: {usage}"))
}

fn arg_text(parts: &[&str], from: usize, usage: &str) -> Result<String, String> {
    if parts.len() > from {
        Ok(parts[from..].join(" "))
    } else {
        Err(format!("uso: {usage}"))
    }
}

fn arg_k(parts: &[&str], index: usize) -> usize {
    parts
        .get(index)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5)
}

fn execute(store: &mut MegaStore, parts: &[&str]) -> Result<(), String> {
    match parts[0] {
        "ajuda" | "help" => {
            print_repl_help();
            Ok(())
        }
        "demo" => {
            *store = sample::demo_store();
            println!("cenário de exemplo carregado");
            print_stats(store);
            Ok(())
        }
        "categoria" => {
            let usage = "categoria <id> <nome>";
            let id = arg_u32(parts, 1, usage)?;
            let name = arg_text(parts, 2, usage)?;
            store.add_category(id, name).map_err(|e| e.to_string())?;
            println!("categoria {id} cadastrada");
            Ok(())
        }
        "produto" => {
            let usage = "produto <id> <categoria> <preço em centavos> <nome>";
            let id = arg_u32(parts, 1, usage)?;
            let category = arg_u32(parts, 2, usage)?;
            let price = parts
                .get(3)
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| format!("uso: {usage}"))?;
            let name = arg_text(parts, 4, usage)?;
            store
                .add_product(id, name, category, price)
                .map_err(|e| e.to_string())?;
            println!("produto {id} cadastrado");
            Ok(())
        }
        "cliente" => {
            let usage = "cliente <id> <nome>";
            let id = arg_u32(parts, 1, usage)?;
            let name = arg_text(parts, 2, usage)?;
            store.add_customer(id, name).map_err(|e| e.to_string())?;
            println!("cliente {id} cadastrado");
            Ok(())
        }
        "comprar" => {
            let usage = "comprar <cliente> <produto> [nota 1-5]";
            let customer = arg_u32(parts, 1, usage)?;
            let product = arg_u32(parts, 2, usage)?;
            let rating = match parts.get(3) {
                None => None,
                Some(text) => Some(
                    text.parse::<u8>()
                        .map_err(|_| "a nota deve ser um número de 1 a 5".to_string())?,
                ),
            };
            let created = store
                .record_purchase(customer, product, rating)
                .map_err(|e| e.to_string())?;
            if created {
                println!("compra registrada");
            } else {
                println!("a compra já existia: conexão apenas atualizada (sem duplicar)");
            }
            Ok(())
        }
        "cancelar" => {
            let usage = "cancelar <cliente> <produto>";
            let customer = arg_u32(parts, 1, usage)?;
            let product = arg_u32(parts, 2, usage)?;
            let removed = store
                .remove_purchase(customer, product)
                .map_err(|e| e.to_string())?;
            if removed {
                println!("compra removida");
            } else {
                println!("não havia compra entre esse cliente e esse produto");
            }
            Ok(())
        }
        "buscar" => {
            let term = arg_text(parts, 1, "buscar <termo>")?;
            let found = store.catalog().search_products(&term);
            if found.is_empty() {
                println!("nenhum produto encontrado");
            }
            for p in found {
                println!("  #{} {} [categoria {}] {}", p.id, p.name, p.category_id, p.price_display());
            }
            Ok(())
        }
        "ver" => {
            let id = arg_u32(parts, 1, "ver <produto>")?;
            match store.catalog().product(id) {
                Some(p) => {
                    let category = store
                        .catalog()
                        .category(p.category_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    println!("  #{} {}", p.id, p.name);
                    println!("  categoria: {} ({})", category, p.category_id);
                    println!("  preço: {}", p.price_display());
                    Ok(())
                }
                None => Err(format!("produto {id} não encontrado")),
            }
        }
        "recomendar-cliente" => {
            let id = arg_u32(parts, 1, "recomendar-cliente <id> [k]")?;
            let cfg = RecommendConfig {
                top_k: arg_k(parts, 2),
                ..RecommendConfig::default()
            };
            store.optimize();
            let recs = store
                .recommend_for_customer(id, &cfg)
                .map_err(|e| e.to_string())?;
            print_recommendations(store, &recs);
            Ok(())
        }
        "recomendar-produto" => {
            let id = arg_u32(parts, 1, "recomendar-produto <id> [k]")?;
            let cfg = RecommendConfig {
                top_k: arg_k(parts, 2),
                ..RecommendConfig::default()
            };
            store.optimize();
            let recs = store
                .recommend_for_product(id, &cfg)
                .map_err(|e| e.to_string())?;
            print_recommendations(store, &recs);
            Ok(())
        }
        "explicar" => {
            let usage = "explicar <cliente> <produto>";
            let customer = arg_u32(parts, 1, usage)?;
            let product = arg_u32(parts, 2, usage)?;
            match store
                .explain(customer, product, 6)
                .map_err(|e| e.to_string())?
            {
                Some(path) => {
                    let steps: Vec<String> = path.iter().map(|s| s.to_string()).collect();
                    println!("  {}", steps.join(" -> "));
                }
                None => println!("  nenhum caminho de até 6 arestas"),
            }
            Ok(())
        }
        "stats" => {
            print_stats(store);
            Ok(())
        }
        other => Err(format!("comando desconhecido: {other} (digite 'ajuda')")),
    }
}

fn repl() {
    println!("ConectaStore REPL. Digite 'ajuda' para ver os comandos e 'sair' para encerrar.");
    let mut store = MegaStore::new();
    let stdin = io::stdin();
    loop {
        print!("megastore> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        if matches!(parts[0], "sair" | "exit" | "quit") {
            break;
        }
        if let Err(message) = execute(&mut store, &parts) {
            println!("erro: {message}");
        }
    }
}
