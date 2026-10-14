//! Algoritmo de recomendação: **BFS por níveis** (com profundidade limitada) sobre o
//! grafo, propagando um "score de afinidade" pelas arestas.
//!
//! # Recomendação para um cliente (3 saltos, filtragem colaborativa)
//!
//! ```text
//! nível 0: o cliente
//! nível 1: produtos que ele comprou                      (peso = nota/5)
//! nível 2: outros clientes que compraram esses produtos  (clientes "parecidos")
//! nível 3: produtos desses clientes que o cliente ainda não tem  -> candidatos
//! ```
//!
//! Em cada nível, vértices alcançados por vários caminhos são **fundidos** (o score
//! é somado e o vértice é expandido uma única vez). Isso evita candidatos duplicados
//! e reduz o trabalho: o custo vem do número de arestas por nível, não do número de
//! caminhos.
//!
//! Ao sair de um vértice intermediário `v`, o sinal é multiplicado por
//! `1 / ln(2 + grau(v))`. É o mesmo princípio do índice de Adamic–Adar: ter em comum
//! um produto muito popular diz pouco sobre a semelhança entre dois clientes, e um
//! cliente que comprou milhares de itens "endossa" cada um deles pouco.
//!
//! # Interesse por categoria
//!
//! Um segundo ramo do percurso passa pelos vértices de categoria
//! (cliente → produtos → categoria → outros produtos da categoria), com score
//! proporcional à participação da categoria nas compras do cliente e à
//! popularidade do produto. O resultado final é uma combinação convexa dos dois
//! ramos, cada um normalizado pelo seu máximo:
//! `score = (1 - β) · colaborativo + β · categoria`, com β = `category_weight`.
//!
//! # Controle de "hubs"
//!
//! Cada expansão olha, no máximo, `max_neighbors` arestas de cada vértice. Com as
//! listas ordenadas por relevância (`Graph::sort_neighbors_by_relevance`), esses são
//! os vizinhos mais fortes. Assim o custo de uma consulta depende dos limites
//! configurados, e não do tamanho do catálogo.

use std::cmp::{Ordering, Reverse};
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::graph::{Edge, EdgeKind, Graph, NodeIdx, NodeKind};
use crate::models::{CustomerId, ProductId};

/// Parâmetros da recomendação.
#[derive(Debug, Clone, PartialEq)]
pub struct RecommendConfig {
    /// Quantidade máxima de produtos devolvidos.
    pub top_k: usize,
    /// Máximo de arestas examinadas por vértice em cada expansão (controle de hubs).
    pub max_neighbors: usize,
    /// Peso β (0 a 1) do ramo de categorias na combinação final. Com 0, só o comportamento de compra conta.
    pub category_weight: f32,
}

impl Default for RecommendConfig {
    fn default() -> Self {
        RecommendConfig {
            top_k: 10,
            max_neighbors: 200,
            category_weight: 0.25,
        }
    }
}

/// Um produto recomendado e sua pontuação (maior é melhor).
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub product_id: ProductId,
    pub score: f32,
}

/// Acumulador de scores por vértice que preserva a ordem de inserção.
///
/// O `HashMap` localiza a posição em O(1) médio e o `Vec` guarda os pares na ordem em
/// que apareceram. Percorrer o `Vec` torna a soma de floats determinística (mesma
/// entrada, mesmo resultado), o que um `HashMap` com ordem aleatória não garantiria.
#[derive(Default)]
struct Accumulator {
    pos: HashMap<NodeIdx, usize>,
    items: Vec<(NodeIdx, f32)>,
}

impl Accumulator {
    fn add(&mut self, key: NodeIdx, value: f32) {
        match self.pos.entry(key) {
            Entry::Occupied(entry) => {
                self.items[*entry.get()].1 += value;
            }
            Entry::Vacant(entry) => {
                entry.insert(self.items.len());
                self.items.push((key, value));
            }
        }
    }

    fn max(&self) -> f32 {
        self.items.iter().map(|&(_, v)| v).fold(0.0, f32::max)
    }
}

/// Candidato do top-K: ordenado por score e, no empate, pelo menor identificador.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    score: f32,
    id: ProductId,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

/// Atenuação aplicada ao sair de um vértice intermediário de grau `degree`.
fn damping(degree: usize) -> f32 {
    1.0 / (2.0 + degree as f32).ln()
}

/// Popularidade de um produto (cresce devagar com o grau).
fn popularity(graph: &Graph, product: NodeIdx) -> f32 {
    (1.0 + graph.degree(product) as f32).ln()
}

/// Arestas de compra (`Purchase`) de um vértice.
fn purchases(graph: &Graph, node: NodeIdx) -> impl Iterator<Item = &Edge> + '_ {
    graph
        .neighbors(node)
        .iter()
        .filter(|e| e.kind == EdgeKind::Purchase)
}

/// Vértice da categoria de um produto, via catálogo (O(1) médio).
fn category_node(graph: &Graph, catalog: &Catalog, product: NodeIdx) -> Option<NodeIdx> {
    let product_id = graph.id(product);
    let category_id = catalog.product(product_id)?.category_id;
    graph.node(NodeKind::Category, category_id)
}

/// Combina os dois ramos, normaliza e seleciona os `k` melhores com um min-heap de tamanho `k`.
fn rank(
    graph: &Graph,
    collaborative: &Accumulator,
    category: &Accumulator,
    beta: f32,
    k: usize,
) -> Vec<Recommendation> {
    let max_collaborative = collaborative.max();
    let max_category = category.max();

    let mut combined = Accumulator::default();
    if max_collaborative > 0.0 {
        for &(node, value) in &collaborative.items {
            combined.add(node, (1.0 - beta) * value / max_collaborative);
        }
    }
    if max_category > 0.0 {
        for &(node, value) in &category.items {
            combined.add(node, beta * value / max_category);
        }
    }

    // Top-K: O(m log k) para m candidatos, memória O(k).
    let mut heap: BinaryHeap<Reverse<Candidate>> = BinaryHeap::with_capacity(k.min(1_024) + 1);
    for &(node, score) in &combined.items {
        let candidate = Candidate {
            score,
            id: graph.id(node),
        };
        if heap.len() < k {
            heap.push(Reverse(candidate));
        } else {
            let worst = heap.peek().map(|r| r.0);
            if let Some(worst) = worst {
                if candidate > worst {
                    heap.pop();
                    heap.push(Reverse(candidate));
                }
            }
        }
    }

    heap.into_sorted_vec()
        .into_iter()
        .map(|Reverse(c)| Recommendation {
            product_id: c.id,
            score: c.score,
        })
        .collect()
}

/// Recomenda produtos para um cliente. Nunca devolve produtos que o cliente já comprou
/// nem produtos repetidos. Cliente sem compras recebe lista vazia (partida a frio).
///
/// Custo de tempo: O(d · N²) operações de hash no pior caso, com `d` o número de
/// produtos do cliente e `N = max_neighbors`, mais O(m log k) para o top-K.
/// Independe do tamanho total do catálogo. Memória por consulta: O(d · N²).
pub fn recommend_for_customer(
    graph: &Graph,
    catalog: &Catalog,
    customer: CustomerId,
    cfg: &RecommendConfig,
) -> Result<Vec<Recommendation>> {
    let start = graph
        .node(NodeKind::Customer, customer)
        .ok_or(Error::UnknownCustomer(customer))?;
    if cfg.top_k == 0 {
        return Ok(Vec::new());
    }
    let limit = cfg.max_neighbors.max(1);

    // Todos os produtos já comprados (mesmo os que o limite deixa fora da expansão).
    let owned: HashSet<NodeIdx> = purchases(graph, start).map(|e| e.to).collect();
    if owned.is_empty() {
        return Ok(Vec::new());
    }

    // Nível 1: produtos do cliente (os mais bem avaliados primeiro).
    let level1: Vec<(NodeIdx, f32)> = purchases(graph, start)
        .take(limit)
        .map(|e| (e.to, e.weight))
        .collect();

    // Nível 2: clientes parecidos (compraram algum dos mesmos produtos).
    let mut level2 = Accumulator::default();
    for &(product, affinity) in &level1 {
        let damp = damping(graph.degree(product));
        for edge in purchases(graph, product)
            .filter(|e| e.to != start)
            .take(limit)
        {
            level2.add(edge.to, affinity * edge.weight * damp);
        }
    }

    // Nível 3: produtos dos clientes parecidos que o cliente ainda não possui.
    let mut collaborative = Accumulator::default();
    for &(neighbor, similarity) in &level2.items {
        let damp = damping(graph.degree(neighbor));
        for edge in purchases(graph, neighbor)
            .filter(|e| !owned.contains(&e.to))
            .take(limit)
        {
            collaborative.add(edge.to, similarity * edge.weight * damp);
        }
    }

    // Ramo de categorias: interesse do cliente por categoria -> produtos populares da categoria.
    let beta = cfg.category_weight.clamp(0.0, 1.0);
    let mut category_scores = Accumulator::default();
    if beta > 0.0 {
        let mut affinity = Accumulator::default();
        for edge in purchases(graph, start) {
            if let Some(category) = category_node(graph, catalog, edge.to) {
                affinity.add(category, edge.weight);
            }
        }
        let total: f32 = affinity.items.iter().map(|&(_, v)| v).sum();
        for &(category, value) in &affinity.items {
            let share = value / total;
            for edge in graph
                .neighbors(category)
                .iter()
                .filter(|e| e.kind == EdgeKind::BelongsTo && !owned.contains(&e.to))
                .take(limit)
            {
                category_scores.add(edge.to, share * popularity(graph, edge.to));
            }
        }
    }

    Ok(rank(graph, &collaborative, &category_scores, beta, cfg.top_k))
}

/// Recomenda produtos relacionados a um produto ("quem comprou este também comprou").
/// Nunca devolve o próprio produto nem repetidos.
///
/// Percurso: produto → clientes que o compraram → outros produtos desses clientes
/// (co-compra), mais o ramo de categoria (produtos populares da mesma categoria).
/// Custo: O(N²) operações de hash no pior caso, com `N = max_neighbors`.
pub fn recommend_for_product(
    graph: &Graph,
    catalog: &Catalog,
    product: ProductId,
    cfg: &RecommendConfig,
) -> Result<Vec<Recommendation>> {
    let start = graph
        .node(NodeKind::Product, product)
        .ok_or(Error::UnknownProduct(product))?;
    if cfg.top_k == 0 {
        return Ok(Vec::new());
    }
    let limit = cfg.max_neighbors.max(1);

    // Nível 1: clientes que compraram o produto (os de maior afinidade primeiro).
    let buyers: Vec<(NodeIdx, f32)> = purchases(graph, start)
        .take(limit)
        .map(|e| (e.to, e.weight))
        .collect();

    // Nível 2: outros produtos comprados por esses clientes.
    let mut collaborative = Accumulator::default();
    for &(buyer, affinity) in &buyers {
        let damp = damping(graph.degree(buyer));
        for edge in purchases(graph, buyer)
            .filter(|e| e.to != start)
            .take(limit)
        {
            collaborative.add(edge.to, affinity * edge.weight * damp);
        }
    }

    // Ramo de categoria: produtos populares da mesma categoria.
    let beta = cfg.category_weight.clamp(0.0, 1.0);
    let mut category_scores = Accumulator::default();
    if beta > 0.0 {
        if let Some(category) = category_node(graph, catalog, start) {
            for edge in graph
                .neighbors(category)
                .iter()
                .filter(|e| e.kind == EdgeKind::BelongsTo && e.to != start)
                .take(limit)
            {
                category_scores.add(edge.to, popularity(graph, edge.to));
            }
        }
    }

    Ok(rank(graph, &collaborative, &category_scores, beta, cfg.top_k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MegaStore;

    /// Categoria 1: produtos 1, 2, 3. Categoria 2: produtos 4, 5. Clientes 1 a 4.
    fn base_store() -> MegaStore {
        let mut s = MegaStore::new();
        s.add_category(1, "A").unwrap();
        s.add_category(2, "B").unwrap();
        for p in 1..=3 {
            s.add_product(p, format!("Produto {p}"), 1, 1_000).unwrap();
        }
        for p in 4..=5 {
            s.add_product(p, format!("Produto {p}"), 2, 1_000).unwrap();
        }
        for c in 1..=4 {
            s.add_customer(c, format!("Cliente {c}")).unwrap();
        }
        s
    }

    fn ids(recs: &[Recommendation]) -> Vec<ProductId> {
        recs.iter().map(|r| r.product_id).collect()
    }

    fn recommend(store: &MegaStore, customer: CustomerId, cfg: &RecommendConfig) -> Vec<Recommendation> {
        recommend_for_customer(store.graph(), store.catalog(), customer, cfg).unwrap()
    }

    #[test]
    fn recommends_what_similar_customers_bought() {
        let mut s = base_store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        s.record_purchase(2, 1, Some(5)).unwrap();
        s.record_purchase(2, 2, Some(5)).unwrap();
        s.record_purchase(3, 4, Some(5)).unwrap();

        let only_behavior = RecommendConfig {
            category_weight: 0.0,
            ..RecommendConfig::default()
        };
        assert_eq!(ids(&recommend(&s, 1, &only_behavior)), vec![2]);

        let with_categories = ids(&recommend(&s, 1, &RecommendConfig::default()));
        assert_eq!(with_categories[0], 2);
        assert!(!with_categories.contains(&1));
        assert!(!with_categories.contains(&4));
        assert!(!with_categories.contains(&5));
    }

    #[test]
    fn candidates_reached_by_many_paths_appear_once() {
        let mut s = base_store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        s.record_purchase(1, 2, Some(5)).unwrap();
        for c in 2..=4 {
            for p in 1..=3 {
                s.record_purchase(c, p, Some(4)).unwrap();
            }
        }
        let recs = ids(&recommend(&s, 1, &RecommendConfig::default()));
        assert_eq!(recs, vec![3]);
    }

    #[test]
    fn never_recommends_owned_products_even_with_a_tiny_limit() {
        let mut s = base_store();
        for p in 1..=3 {
            s.record_purchase(1, p, Some(5)).unwrap();
        }
        for p in 1..=4 {
            s.record_purchase(2, p, Some(5)).unwrap();
        }
        let cfg = RecommendConfig {
            max_neighbors: 1,
            ..RecommendConfig::default()
        };
        let recs = ids(&recommend(&s, 1, &cfg));
        assert!(recs.contains(&4));
        for owned in 1..=3 {
            assert!(!recs.contains(&owned));
        }
    }

    #[test]
    fn top_k_truncates_and_keeps_the_best_first() {
        let mut s = base_store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        s.record_purchase(2, 1, Some(5)).unwrap();
        s.record_purchase(2, 2, Some(5)).unwrap();
        s.record_purchase(3, 1, Some(5)).unwrap();
        s.record_purchase(3, 2, Some(5)).unwrap();
        s.record_purchase(3, 3, Some(5)).unwrap();

        let full = recommend(&s, 1, &RecommendConfig::default());
        assert!(full.len() >= 2);
        for pair in full.windows(2) {
            assert!(pair[0].score >= pair[1].score);
        }
        let one = recommend(
            &s,
            1,
            &RecommendConfig {
                top_k: 1,
                ..RecommendConfig::default()
            },
        );
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].product_id, full[0].product_id);
        assert_eq!(full[0].product_id, 2);
    }

    #[test]
    fn category_branch_reaches_products_nobody_in_common_bought() {
        let mut s = base_store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        s.record_purchase(2, 2, Some(5)).unwrap();

        let only_behavior = RecommendConfig {
            category_weight: 0.0,
            ..RecommendConfig::default()
        };
        assert!(recommend(&s, 1, &only_behavior).is_empty());

        let recs = ids(&recommend(&s, 1, &RecommendConfig::default()));
        assert_eq!(recs[0], 2);
        assert!(recs.contains(&3));
        assert!(!recs.contains(&1));
    }

    #[test]
    fn unknown_ids_and_cold_start() {
        let s = base_store();
        let cfg = RecommendConfig::default();
        assert_eq!(
            recommend_for_customer(s.graph(), s.catalog(), 99, &cfg),
            Err(Error::UnknownCustomer(99))
        );
        assert_eq!(
            recommend_for_product(s.graph(), s.catalog(), 99, &cfg),
            Err(Error::UnknownProduct(99))
        );
        assert!(recommend(&s, 4, &cfg).is_empty());
    }

    #[test]
    fn product_recommendation_ranks_co_purchases_and_excludes_itself() {
        let mut s = base_store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        s.record_purchase(1, 2, Some(5)).unwrap();
        s.record_purchase(2, 1, Some(5)).unwrap();
        s.record_purchase(2, 2, Some(5)).unwrap();
        s.record_purchase(2, 3, Some(5)).unwrap();
        s.record_purchase(3, 4, Some(5)).unwrap();

        let recs = ids(&recommend_for_product(s.graph(), s.catalog(), 1, &RecommendConfig::default()).unwrap());
        assert_eq!(recs[0], 2);
        assert!(recs.contains(&3));
        assert!(!recs.contains(&1));
        assert!(!recs.contains(&4));
        assert!(!recs.contains(&5));
    }

    #[test]
    fn results_are_deterministic() {
        let build = || {
            let mut s = base_store();
            s.record_purchase(1, 1, Some(5)).unwrap();
            s.record_purchase(2, 1, Some(4)).unwrap();
            s.record_purchase(2, 2, Some(3)).unwrap();
            s.record_purchase(3, 1, Some(5)).unwrap();
            s.record_purchase(3, 3, Some(2)).unwrap();
            s.optimize();
            s
        };
        let first = recommend(&build(), 1, &RecommendConfig::default());
        let second = recommend(&build(), 1, &RecommendConfig::default());
        assert_eq!(first, second);
    }

    #[test]
    fn popular_hub_is_handled_with_a_limit() {
        let mut s = MegaStore::new();
        s.add_category(1, "A").unwrap();
        s.add_product(1, "Hub", 1, 1_000).unwrap();
        s.add_product(2, "Outro", 1, 1_000).unwrap();
        for c in 1..=500u32 {
            s.add_customer(c, format!("Cliente {c}")).unwrap();
            s.record_purchase(c, 1, Some(5)).unwrap();
            if c % 2 == 0 {
                s.record_purchase(c, 2, Some(4)).unwrap();
            }
        }
        s.optimize();
        let cfg = RecommendConfig {
            max_neighbors: 50,
            ..RecommendConfig::default()
        };
        let recs = ids(&recommend(&s, 1, &cfg));
        assert_eq!(recs, vec![2]);
    }
}
