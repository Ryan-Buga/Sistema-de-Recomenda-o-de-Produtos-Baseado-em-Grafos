//! Avaliação de relevância em dados sintéticos: compara o recomendador baseado em grafo com duas
//! linhas de base que imitam o sistema atual da MegaStore (mais vendidos e mais vendidos da mesma categoria).
//!
//! Método "leave-one-out": para clientes selecionados, uma compra alinhada ao gosto é retida
//! (não entra no grafo). Se o produto retido aparece no top-K recomendado, conta como acerto.

use std::collections::{HashMap, HashSet};

use crate::datagen::Dataset;
use crate::graph::NodeKind;
use crate::models::{CategoryId, CustomerId, ProductId};
use crate::recommender::RecommendConfig;
use crate::store::MegaStore;

/// Taxas de acerto (hit rate @ K) de cada estratégia.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityReport {
    pub k: usize,
    pub evaluated: usize,
    pub best_sellers: f64,
    pub same_category: f64,
    pub graph: f64,
}

fn owned_products(store: &MegaStore, customer: CustomerId) -> HashSet<ProductId> {
    store
        .purchased_products(customer)
        .map(|v| v.into_iter().collect::<HashSet<ProductId>>())
        .unwrap_or_default()
}

/// Linha de base 2: produtos mais vendidos das categorias que o cliente já comprou.
fn same_category_top(
    store: &MegaStore,
    owned: &HashSet<ProductId>,
    degrees: &HashMap<ProductId, usize>,
    k: usize,
) -> Vec<ProductId> {
    let catalog = store.catalog();
    let mut categories: Vec<CategoryId> = owned
        .iter()
        .filter_map(|&p| catalog.product(p).map(|x| x.category_id))
        .collect();
    categories.sort_unstable();
    categories.dedup();

    let mut candidates: Vec<(usize, ProductId)> = Vec::new();
    for category in categories {
        for &p in catalog.products_in_category(category) {
            if !owned.contains(&p) {
                candidates.push((degrees.get(&p).copied().unwrap_or(0), p));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    candidates.into_iter().take(k).map(|(_, p)| p).collect()
}

/// Avalia até `max_samples` compras retidas e devolve o hit rate @ `k` de cada estratégia.
/// O grafo deve estar otimizado (`store.optimize()`) para refletir o uso real.
pub fn evaluate(dataset: &Dataset, k: usize, max_samples: usize) -> QualityReport {
    let store = &dataset.store;
    let graph = store.graph();

    // Popularidade = grau do produto no grafo (quantidade de compradores + a aresta de categoria).
    let mut degrees: HashMap<ProductId, usize> = HashMap::new();
    for product in store.catalog().products() {
        if let Some(node) = graph.node(NodeKind::Product, product.id) {
            degrees.insert(product.id, graph.degree(node));
        }
    }
    let mut ranking: Vec<(usize, ProductId)> = degrees.iter().map(|(&p, &d)| (d, p)).collect();
    ranking.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let best_sellers: Vec<ProductId> = ranking.into_iter().map(|(_, p)| p).collect();

    let cfg = RecommendConfig {
        top_k: k,
        ..RecommendConfig::default()
    };

    let mut evaluated = 0usize;
    let mut hits_best = 0usize;
    let mut hits_category = 0usize;
    let mut hits_graph = 0usize;

    for &(customer, held) in dataset.held_out.iter().take(max_samples) {
        let owned = owned_products(store, customer);
        evaluated += 1;

        if best_sellers
            .iter()
            .filter(|p| !owned.contains(*p))
            .take(k)
            .any(|&p| p == held)
        {
            hits_best += 1;
        }
        if same_category_top(store, &owned, &degrees, k).contains(&held) {
            hits_category += 1;
        }
        let graph_hit = store
            .recommend_for_customer(customer, &cfg)
            .map(|recs| recs.iter().any(|r| r.product_id == held))
            .unwrap_or(false);
        if graph_hit {
            hits_graph += 1;
        }
    }

    let rate = |hits: usize| {
        if evaluated == 0 {
            0.0
        } else {
            hits as f64 / evaluated as f64
        }
    };
    QualityReport {
        k,
        evaluated,
        best_sellers: rate(hits_best),
        same_category: rate(hits_category),
        graph: rate(hits_graph),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datagen::{generate, DatasetSpec};

    #[test]
    fn rates_are_valid_probabilities() {
        let spec = DatasetSpec {
            customers: 400,
            products: 300,
            categories: 5,
            purchases_per_customer: 8,
            seed: 3,
            holdout_every: 5,
        };
        let mut dataset = generate(&spec);
        dataset.store.optimize();
        let report = evaluate(&dataset, 10, 50);
        assert!(report.evaluated > 0 && report.evaluated <= 50);
        for rate in [report.best_sellers, report.same_category, report.graph] {
            assert!((0.0..=1.0).contains(&rate));
        }
    }

    #[test]
    fn empty_holdout_gives_zero_rates() {
        let spec = DatasetSpec::scaled(100, 1);
        let mut dataset = generate(&spec);
        dataset.store.optimize();
        let report = evaluate(&dataset, 10, 50);
        assert_eq!(report.evaluated, 0);
        assert_eq!(report.graph, 0.0);
    }
}
