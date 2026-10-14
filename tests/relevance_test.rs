//! Teste de relevância em dados sintéticos: o recomendador baseado em grafo deve acertar mais
//! compras retidas (leave-one-out) do que as estratégias do sistema atual da MegaStore.

use megastore::datagen::{generate, DatasetSpec};
use megastore::eval::evaluate;

#[test]
fn graph_recommender_beats_best_sellers_and_same_category() {
    let mut spec = DatasetSpec::scaled(3_000, 7);
    spec.holdout_every = 10;
    let mut dataset = generate(&spec);
    dataset.store.optimize();

    let report = evaluate(&dataset, 10, 300);
    println!(
        "hit rate @{}: mais vendidos {:.1}%, mesma categoria {:.1}%, grafo {:.1}% ({} amostras)",
        report.k,
        report.best_sellers * 100.0,
        report.same_category * 100.0,
        report.graph * 100.0,
        report.evaluated
    );

    assert!(report.evaluated >= 100, "poucas amostras para comparar");
    assert!(report.graph > report.best_sellers);
    assert!(report.graph > report.same_category);
    assert!(report.graph > 0.15, "taxa de acerto do grafo muito baixa: {}", report.graph);
}
