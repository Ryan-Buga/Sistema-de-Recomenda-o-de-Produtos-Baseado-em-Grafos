//! Testes de integração: usam apenas a API pública da biblioteca `megastore`.

use std::collections::HashSet;

use megastore::bench::{fmt_duration, run_scenario};
use megastore::datagen::{generate, DatasetSpec};
use megastore::graph::{EdgeKind, NodeKind};
use megastore::sample::demo_store;
use megastore::{Error, MegaStore, Recommendation, RecommendConfig};

fn ids(recs: &[Recommendation]) -> Vec<u32> {
    recs.iter().map(|r| r.product_id).collect()
}

fn assert_unique(values: &[u32]) {
    let unique: HashSet<u32> = values.iter().copied().collect();
    assert_eq!(unique.len(), values.len(), "há recomendações repetidas: {values:?}");
}

#[test]
fn register_query_connect_and_recommend_from_scratch() {
    let mut store = MegaStore::new();
    store.add_category(1, "Eletrônicos").unwrap();
    store.add_category(2, "Livros").unwrap();
    store.add_product(1, "Notebook", 1, 450_000).unwrap();
    store.add_product(2, "Mouse Sem Fio", 1, 8_990).unwrap();
    store.add_product(3, "Teclado Mecânico", 1, 24_990).unwrap();
    store.add_product(4, "Livro de Grafos", 2, 9_990).unwrap();
    store.add_customer(1, "Ana").unwrap();
    store.add_customer(2, "Bruno").unwrap();
    store.add_customer(3, "Carla").unwrap();

    // Consulta de produtos.
    let found = store.catalog().search_products("mouse");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, 2);
    assert_eq!(store.catalog().product(3).unwrap().price_display(), "R$ 249,90");
    assert_eq!(store.catalog().products_in_category(1), &[1, 2, 3]);

    // Conexões: Ana e Bruno compartilham Notebook e Mouse; só Bruno comprou o Teclado.
    store.record_purchase(1, 1, Some(5)).unwrap();
    store.record_purchase(1, 2, Some(4)).unwrap();
    store.record_purchase(2, 1, Some(5)).unwrap();
    store.record_purchase(2, 2, Some(5)).unwrap();
    store.record_purchase(2, 3, Some(5)).unwrap();
    store.record_purchase(3, 4, Some(5)).unwrap();
    store.record_purchase(3, 3, Some(4)).unwrap();
    store.optimize();

    let recs = store
        .recommend_for_customer(1, &RecommendConfig::default())
        .unwrap();
    let recommended = ids(&recs);
    assert_eq!(recommended[0], 3, "o teclado deve ser a primeira recomendação da Ana");
    assert!(!recommended.contains(&1) && !recommended.contains(&2));
    assert!(!recommended.contains(&4), "o livro vem de um cliente sem afinidade com a Ana");

    // Gerenciamento de conexões: cancelar a compra do Mouse muda o resultado do grafo.
    assert_eq!(store.remove_purchase(1, 2), Ok(true));
    assert_eq!(store.stats().purchase_edges, 6);
    let after = ids(&store.recommend_for_customer(1, &RecommendConfig::default()).unwrap());
    assert!(after.contains(&2), "o mouse volta a ser candidato depois do cancelamento");
}

#[test]
fn demo_scenario_gives_intuitive_recommendations() {
    let store = demo_store();
    let cfg = RecommendConfig::default();

    // Ana (1) comprou Notebook (1), Mouse (2) e o livro de Rust (6).
    let ana = ids(&store.recommend_for_customer(1, &cfg).unwrap());
    assert_eq!(ana[0], 3, "teclado: comprado por três clientes parecidos com a Ana");
    for owned in [1, 2, 6] {
        assert!(!ana.contains(&owned));
    }
    assert!(ana.contains(&5) && ana.contains(&7));

    // Quem comprou o Notebook também comprou, antes de tudo, o Mouse.
    let related = ids(&store.recommend_for_product(1, &cfg).unwrap());
    assert_eq!(related[0], 2);
    assert!(!related.contains(&1));

    // O livro 13 ainda não tem compradores: aparece pelo ramo de categorias.
    let for_ana_with_categories = ids(&store.recommend_for_customer(1, &cfg).unwrap());
    assert!(for_ana_with_categories.contains(&13));
    let only_behavior = RecommendConfig {
        category_weight: 0.0,
        ..RecommendConfig::default()
    };
    let for_ana_behavior = ids(&store.recommend_for_customer(1, &only_behavior).unwrap());
    assert!(!for_ana_behavior.contains(&13));

    // Explicação por BFS: Ana -> ... -> Teclado em 3 arestas.
    let path = store.explain(1, 3, 6).unwrap().unwrap();
    assert_eq!(path.len(), 4);
    assert_eq!(path[0].kind, NodeKind::Customer);
    assert_eq!(path[0].id, 1);
    assert_eq!(path[2].kind, NodeKind::Customer, "a explicação passa por outro cliente");
    assert_eq!(path[3].kind, NodeKind::Product);
    assert_eq!(path[3].id, 3);
}

#[test]
fn duplicates_are_prevented_everywhere() {
    let mut store = demo_store();
    let before = store.stats();

    assert_eq!(store.add_product(3, "Teclado Clone", 1, 100), Err(Error::DuplicateProduct(3)));
    assert_eq!(store.add_customer(1, "Outra Ana"), Err(Error::DuplicateCustomer(1)));
    assert_eq!(store.add_category(2, "Outra"), Err(Error::DuplicateCategory(2)));
    assert_eq!(store.record_purchase(1, 1, Some(4)), Ok(false));
    assert_eq!(store.stats(), before);

    for customer in 1..=6 {
        let recs = ids(&store.recommend_for_customer(customer, &RecommendConfig::default()).unwrap());
        assert_unique(&recs);
        let owned = store.purchased_products(customer).unwrap();
        for product in recs {
            assert!(!owned.contains(&product), "cliente {customer} recebeu produto que já comprou");
        }
    }
    for product in 1..=13 {
        let recs = ids(&store.recommend_for_product(product, &RecommendConfig::default()).unwrap());
        assert_unique(&recs);
        assert!(!recs.contains(&product));
    }
}

#[test]
fn invariants_hold_on_generated_data() {
    let spec = DatasetSpec {
        customers: 3_000,
        products: 3_000,
        categories: 20,
        purchases_per_customer: 10,
        seed: 2024,
        holdout_every: 0,
    };
    let mut dataset = generate(&spec);
    dataset.store.optimize();
    let store = &dataset.store;
    let cfg = RecommendConfig::default();

    let mut non_empty = 0;
    for customer in (0..3_000u32).step_by(30) {
        let recs = store.recommend_for_customer(customer, &cfg).unwrap();
        assert!(recs.len() <= cfg.top_k);
        for pair in recs.windows(2) {
            assert!(pair[0].score >= pair[1].score, "resultado fora de ordem");
        }
        let recommended = ids(&recs);
        assert_unique(&recommended);
        let owned = store.purchased_products(customer).unwrap();
        for product in &recommended {
            assert!(!owned.contains(product));
        }
        if !recommended.is_empty() {
            non_empty += 1;
        }
    }
    assert!(non_empty > 50, "quase nenhum cliente recebeu recomendações");

    for product in (0..3_000u32).step_by(60) {
        let recs = ids(&store.recommend_for_product(product, &cfg).unwrap());
        assert!(recs.len() <= cfg.top_k);
        assert_unique(&recs);
        assert!(!recs.contains(&product));
    }
}

#[test]
fn same_seed_gives_the_same_recommendations() {
    let spec = DatasetSpec::scaled(1_000, 99);
    let mut a = generate(&spec);
    let mut b = generate(&spec);
    a.store.optimize();
    b.store.optimize();
    let cfg = RecommendConfig::default();
    for customer in (0..1_000u32).step_by(50) {
        assert_eq!(
            a.store.recommend_for_customer(customer, &cfg),
            b.store.recommend_for_customer(customer, &cfg)
        );
    }
}

#[test]
fn adjacency_lists_use_far_less_memory_than_a_matrix() {
    let dataset = generate(&DatasetSpec::scaled(5_000, 5));
    let graph = dataset.store.graph();
    let lists = graph.approx_memory_bytes() as u128;
    let matrix = graph.dense_matrix_bytes();
    assert!(matrix > 50 * lists, "matriz {matrix} bytes vs listas {lists} bytes");
    assert_eq!(graph.edge_count_of(EdgeKind::BelongsTo), 5_000);
}

#[test]
fn timing_with_growing_volumes() {
    println!("produtos | compras | carga | rec. cliente (média) | rec. produto (média)");
    let mut previous_purchases = 0;
    for products in [500usize, 2_000, 8_000] {
        let row = run_scenario(products, 50);
        println!(
            "{:>8} | {:>7} | {:>8} | {:>12} | {:>12}",
            row.products,
            row.purchases,
            fmt_duration(row.load_time),
            fmt_duration(row.customer_queries.mean),
            fmt_duration(row.product_queries.mean)
        );
        assert!(row.purchases > previous_purchases, "o volume deve crescer");
        assert!(row.avg_results > 0.0, "as consultas devem devolver recomendações");
        // Limite muito folgado (build de debug em máquinas lentas): só detecta explosões.
        assert!(row.customer_queries.max.as_secs() < 5);
        assert!(row.product_queries.max.as_secs() < 5);
        previous_purchases = row.purchases;
    }
}
