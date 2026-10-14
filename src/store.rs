//! Fachada do sistema: junta o catálogo (`HashMap`) e o grafo (lista de adjacência)
//! e mantém os dois consistentes. É a API que a CLI e os testes usam.

use std::fmt;

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::graph::{EdgeKind, Graph, NodeIdx, NodeKind};
use crate::models::{Category, CategoryId, Customer, CustomerId, Product, ProductId};
use crate::recommender::{self, RecommendConfig, Recommendation};

/// Peso de uma compra sem nota de avaliação (sinal moderado de interesse).
pub const DEFAULT_PURCHASE_WEIGHT: f32 = 0.6;
/// Maior nota de avaliação aceita.
pub const MAX_RATING: u8 = 5;

/// Sistema de recomendação da MegaStore: catálogo + grafo de relações.
#[derive(Debug, Clone, Default)]
pub struct MegaStore {
    catalog: Catalog,
    graph: Graph,
}

/// Passo de um caminho no grafo, já com o rótulo legível do vértice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStep {
    pub kind: NodeKind,
    pub id: u32,
    pub label: String,
}

impl fmt::Display for PathStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} #{} ({})", self.kind, self.id, self.label)
    }
}

/// Resumo do tamanho do sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreStats {
    pub customers: usize,
    pub products: usize,
    pub categories: usize,
    pub vertices: usize,
    pub purchase_edges: usize,
    pub membership_edges: usize,
    pub graph_memory_bytes: usize,
}

fn rating_to_weight(rating: Option<u8>) -> Result<f32> {
    match rating {
        None => Ok(DEFAULT_PURCHASE_WEIGHT),
        Some(r) if (1..=MAX_RATING).contains(&r) => Ok(f32::from(r) / f32::from(MAX_RATING)),
        Some(r) => Err(Error::InvalidRating(r)),
    }
}

impl MegaStore {
    /// Cria um sistema vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Catálogo (somente leitura).
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Grafo (somente leitura).
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Cadastra uma categoria e cria o vértice correspondente.
    pub fn add_category(&mut self, id: CategoryId, name: impl Into<String>) -> Result<()> {
        self.catalog.add_category(Category {
            id,
            name: name.into(),
        })?;
        self.graph.add_node(NodeKind::Category, id);
        Ok(())
    }

    /// Cadastra um cliente e cria o vértice correspondente.
    pub fn add_customer(&mut self, id: CustomerId, name: impl Into<String>) -> Result<()> {
        self.catalog.add_customer(Customer {
            id,
            name: name.into(),
        })?;
        self.graph.add_node(NodeKind::Customer, id);
        Ok(())
    }

    /// Cadastra um produto, cria o vértice e liga-o à sua categoria (aresta `BelongsTo`).
    /// Falha se o identificador já existir ou se a categoria não estiver cadastrada.
    pub fn add_product(
        &mut self,
        id: ProductId,
        name: impl Into<String>,
        category_id: CategoryId,
        price_cents: u64,
    ) -> Result<()> {
        self.catalog.add_product(Product {
            id,
            name: name.into(),
            category_id,
            price_cents,
        })?;
        let product = self.graph.add_node(NodeKind::Product, id);
        let category = self
            .graph
            .node(NodeKind::Category, category_id)
            .ok_or(Error::UnknownCategory(category_id))?;
        self.graph
            .upsert_edge(product, category, 1.0, EdgeKind::BelongsTo);
        Ok(())
    }

    /// Registra uma compra (aresta `Purchase`) com nota opcional de 1 a 5; o peso da
    /// aresta é nota/5. Registrar de novo a mesma compra apenas atualiza o peso, então
    /// não existem conexões duplicadas.
    ///
    /// Devolve `true` se a conexão é nova e `false` se foi atualizada.
    pub fn record_purchase(
        &mut self,
        customer: CustomerId,
        product: ProductId,
        rating: Option<u8>,
    ) -> Result<bool> {
        let weight = rating_to_weight(rating)?;
        let (c, p) = self.purchase_nodes(customer, product)?;
        Ok(self.graph.upsert_edge(c, p, weight, EdgeKind::Purchase))
    }

    /// Remove uma compra. Devolve `true` se a conexão existia.
    pub fn remove_purchase(&mut self, customer: CustomerId, product: ProductId) -> Result<bool> {
        let (c, p) = self.purchase_nodes(customer, product)?;
        Ok(self.graph.remove_edge(c, p, EdgeKind::Purchase))
    }

    fn purchase_nodes(&self, customer: CustomerId, product: ProductId) -> Result<(NodeIdx, NodeIdx)> {
        let c = self
            .graph
            .node(NodeKind::Customer, customer)
            .ok_or(Error::UnknownCustomer(customer))?;
        let p = self
            .graph
            .node(NodeKind::Product, product)
            .ok_or(Error::UnknownProduct(product))?;
        Ok((c, p))
    }

    /// Produtos comprados por um cliente (os de maior afinidade primeiro, se o grafo estiver otimizado).
    pub fn purchased_products(&self, customer: CustomerId) -> Result<Vec<ProductId>> {
        let node = self
            .graph
            .node(NodeKind::Customer, customer)
            .ok_or(Error::UnknownCustomer(customer))?;
        Ok(self
            .graph
            .neighbors(node)
            .iter()
            .filter(|e| e.kind == EdgeKind::Purchase)
            .map(|e| self.graph.id(e.to))
            .collect())
    }

    /// Ordena as listas de adjacência por relevância. Chame depois de cargas em lote
    /// para que o limite de vizinhos (`max_neighbors`) use os vizinhos mais fortes.
    pub fn optimize(&mut self) {
        self.graph.sort_neighbors_by_relevance();
    }

    /// Recomenda produtos para um cliente (BFS de 3 níveis + ramo de categorias).
    pub fn recommend_for_customer(
        &self,
        customer: CustomerId,
        cfg: &RecommendConfig,
    ) -> Result<Vec<Recommendation>> {
        recommender::recommend_for_customer(&self.graph, &self.catalog, customer, cfg)
    }

    /// Recomenda produtos relacionados a um produto.
    pub fn recommend_for_product(
        &self,
        product: ProductId,
        cfg: &RecommendConfig,
    ) -> Result<Vec<Recommendation>> {
        recommender::recommend_for_product(&self.graph, &self.catalog, product, cfg)
    }

    /// Explica a relação entre um cliente e um produto: o menor caminho no grafo (BFS),
    /// limitado a `max_depth` arestas. Prefere caminhos que passam apenas por compras
    /// (cliente, produto, outro cliente, produto); se não houver, aceita qualquer caminho
    /// (por exemplo, via categoria). `Ok(None)` se não houver caminho dentro do limite.
    pub fn explain(
        &self,
        customer: CustomerId,
        product: ProductId,
        max_depth: usize,
    ) -> Result<Option<Vec<PathStep>>> {
        let (c, p) = self.purchase_nodes(customer, product)?;
        let path = self
            .graph
            .shortest_path(c, p, max_depth, Some(EdgeKind::Purchase))
            .or_else(|| self.graph.shortest_path(c, p, max_depth, None));
        Ok(path.map(|nodes| nodes.into_iter().map(|idx| self.describe(idx)).collect()))
    }

    fn describe(&self, idx: NodeIdx) -> PathStep {
        let kind = self.graph.kind(idx);
        let id = self.graph.id(idx);
        let label = match kind {
            NodeKind::Customer => self.catalog.customer(id).map(|c| c.name.clone()),
            NodeKind::Product => self.catalog.product(id).map(|p| p.name.clone()),
            NodeKind::Category => self.catalog.category(id).map(|c| c.name.clone()),
        };
        PathStep {
            kind,
            id,
            label: label.unwrap_or_default(),
        }
    }

    /// Estatísticas de tamanho do catálogo e do grafo.
    pub fn stats(&self) -> StoreStats {
        StoreStats {
            customers: self.catalog.customer_count(),
            products: self.catalog.product_count(),
            categories: self.catalog.category_count(),
            vertices: self.graph.node_count(),
            purchase_edges: self.graph.edge_count_of(EdgeKind::Purchase),
            membership_edges: self.graph.edge_count_of(EdgeKind::BelongsTo),
            graph_memory_bytes: self.graph.approx_memory_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MegaStore {
        let mut s = MegaStore::new();
        s.add_category(1, "Eletrônicos").unwrap();
        s.add_product(1, "Mouse", 1, 8_990).unwrap();
        s.add_product(2, "Teclado", 1, 24_990).unwrap();
        s.add_customer(1, "Ana").unwrap();
        s
    }

    #[test]
    fn registration_creates_vertices_and_category_edges() {
        let s = store();
        let stats = s.stats();
        assert_eq!(stats.categories, 1);
        assert_eq!(stats.products, 2);
        assert_eq!(stats.customers, 1);
        assert_eq!(stats.vertices, 4);
        assert_eq!(stats.membership_edges, 2);
        assert_eq!(stats.purchase_edges, 0);
    }

    #[test]
    fn rejects_duplicates_without_corrupting_the_graph() {
        let mut s = store();
        assert_eq!(s.add_category(1, "Outra"), Err(Error::DuplicateCategory(1)));
        assert_eq!(s.add_product(1, "Clone", 1, 100), Err(Error::DuplicateProduct(1)));
        assert_eq!(s.add_customer(1, "Outra Ana"), Err(Error::DuplicateCustomer(1)));
        assert_eq!(s.add_product(9, "Órfão", 77, 100), Err(Error::UnknownCategory(77)));
        let stats = s.stats();
        assert_eq!(stats.vertices, 4);
        assert_eq!(stats.membership_edges, 2);
    }

    #[test]
    fn recording_the_same_purchase_twice_updates_instead_of_duplicating() {
        let mut s = store();
        assert_eq!(s.record_purchase(1, 1, Some(2)), Ok(true));
        assert_eq!(s.record_purchase(1, 1, Some(5)), Ok(false));
        assert_eq!(s.stats().purchase_edges, 1);
        assert_eq!(s.purchased_products(1), Ok(vec![1]));
        let c = s.graph().node(NodeKind::Customer, 1).unwrap();
        let p = s.graph().node(NodeKind::Product, 1).unwrap();
        let weight = s.graph().edge_weight(c, p, EdgeKind::Purchase).unwrap();
        assert!((weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn purchase_without_rating_uses_the_default_weight() {
        let mut s = store();
        s.record_purchase(1, 2, None).unwrap();
        let c = s.graph().node(NodeKind::Customer, 1).unwrap();
        let p = s.graph().node(NodeKind::Product, 2).unwrap();
        let weight = s.graph().edge_weight(c, p, EdgeKind::Purchase).unwrap();
        assert!((weight - DEFAULT_PURCHASE_WEIGHT).abs() < 1e-6);
    }

    #[test]
    fn validates_ratings_and_identifiers() {
        let mut s = store();
        assert_eq!(s.record_purchase(1, 1, Some(0)), Err(Error::InvalidRating(0)));
        assert_eq!(s.record_purchase(1, 1, Some(6)), Err(Error::InvalidRating(6)));
        assert_eq!(s.record_purchase(9, 1, Some(5)), Err(Error::UnknownCustomer(9)));
        assert_eq!(s.record_purchase(1, 9, Some(5)), Err(Error::UnknownProduct(9)));
        assert_eq!(s.stats().purchase_edges, 0);
    }

    #[test]
    fn removes_purchases() {
        let mut s = store();
        s.record_purchase(1, 1, Some(4)).unwrap();
        assert_eq!(s.remove_purchase(1, 1), Ok(true));
        assert_eq!(s.remove_purchase(1, 1), Ok(false));
        assert_eq!(s.stats().purchase_edges, 0);
        assert_eq!(s.purchased_products(1), Ok(Vec::new()));
    }

    #[test]
    fn explain_describes_the_path_with_labels() {
        let mut s = store();
        s.record_purchase(1, 1, Some(5)).unwrap();
        let direct = s.explain(1, 1, 4).unwrap().unwrap();
        assert_eq!(direct.len(), 2);
        assert_eq!(direct[0].to_string(), "Cliente #1 (Ana)");
        assert_eq!(direct[1].to_string(), "Produto #1 (Mouse)");

        // Teclado (2) não foi comprado: o caminho passa pela categoria (Ana, Mouse, Eletrônicos, Teclado).
        let via_category = s.explain(1, 2, 4).unwrap().unwrap();
        assert_eq!(via_category.len(), 4);
        assert_eq!(via_category[2].kind, NodeKind::Category);
        assert_eq!(s.explain(1, 2, 2), Ok(None));
    }
}
