//! Grafo ponderado e não direcionado, armazenado como **lista de adjacência**.
//!
//! * Cada vértice recebe um índice denso (`NodeIdx`, um `u32`) que serve de posição
//!   nos vetores internos. O mapa `(tipo, id externo) -> índice` fica em um `HashMap`.
//! * As arestas de cada vértice ficam em um `Vec<Edge>` próprio. Uma aresta não
//!   direcionada ocupa uma posição em cada uma das duas listas.
//! * Memória: O(V + E). Uma matriz de adjacência exigiria O(V²), inviável para
//!   milhões de vértices em um grafo esparso como o de compras de uma loja.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::mem::size_of;

/// Índice denso de um vértice dentro do grafo.
pub type NodeIdx = u32;

/// Tipo do vértice: as três entidades do domínio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Customer,
    Product,
    Category,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            NodeKind::Customer => "Cliente",
            NodeKind::Product => "Produto",
            NodeKind::Category => "Categoria",
        };
        write!(f, "{name}")
    }
}

/// Tipo da aresta.
///
/// * `Purchase`: Cliente–Produto; o peso (0,1] expressa o interesse (nota/5).
/// * `BelongsTo`: Produto–Categoria; peso fixo 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Purchase = 0,
    BelongsTo = 1,
}

const EDGE_KINDS: usize = 2;

/// Aresta de uma lista de adjacência (12 bytes: destino, peso e tipo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub to: NodeIdx,
    pub weight: f32,
    pub kind: EdgeKind,
}

/// Grafo ponderado e não direcionado (heterogêneo: clientes, produtos e categorias).
#[derive(Debug, Clone, Default)]
pub struct Graph {
    kinds: Vec<NodeKind>,
    ids: Vec<u32>,
    adj: Vec<Vec<Edge>>,
    index: HashMap<(NodeKind, u32), NodeIdx>,
    edge_counts: [usize; EDGE_KINDS],
}

impl Graph {
    /// Cria um grafo vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Número de vértices.
    pub fn node_count(&self) -> usize {
        self.kinds.len()
    }

    /// Número de arestas não direcionadas (cada uma é guardada duas vezes).
    pub fn edge_count(&self) -> usize {
        self.edge_counts.iter().sum()
    }

    /// Número de arestas não direcionadas de um tipo.
    pub fn edge_count_of(&self, kind: EdgeKind) -> usize {
        self.edge_counts[kind as usize]
    }

    /// Insere um vértice. É idempotente: se `(kind, id)` já existe, devolve o índice atual.
    /// Custo: O(1) médio.
    pub fn add_node(&mut self, kind: NodeKind, id: u32) -> NodeIdx {
        if let Some(&idx) = self.index.get(&(kind, id)) {
            return idx;
        }
        let idx = self.kinds.len() as NodeIdx;
        self.kinds.push(kind);
        self.ids.push(id);
        self.adj.push(Vec::new());
        self.index.insert((kind, id), idx);
        idx
    }

    /// Procura o índice de um vértice pelo tipo e pelo identificador externo. O(1) médio.
    pub fn node(&self, kind: NodeKind, id: u32) -> Option<NodeIdx> {
        self.index.get(&(kind, id)).copied()
    }

    /// Tipo do vértice. O(1). Entra em pânico se o índice não existir.
    pub fn kind(&self, idx: NodeIdx) -> NodeKind {
        self.kinds[idx as usize]
    }

    /// Identificador externo do vértice. O(1). Entra em pânico se o índice não existir.
    pub fn id(&self, idx: NodeIdx) -> u32 {
        self.ids[idx as usize]
    }

    /// Arestas que saem do vértice. O(1) para obter a lista.
    pub fn neighbors(&self, idx: NodeIdx) -> &[Edge] {
        &self.adj[idx as usize]
    }

    /// Grau do vértice (número de arestas incidentes, de todos os tipos).
    pub fn degree(&self, idx: NodeIdx) -> usize {
        self.adj[idx as usize].len()
    }

    /// Insere a aresta `a — b` (não direcionada) ou, se já existir uma aresta do mesmo
    /// tipo entre os dois vértices, apenas atualiza o peso. Assim o grafo nunca tem
    /// arestas paralelas. Laços (`a == b`) são ignorados.
    ///
    /// Devolve `true` se a aresta é nova e `false` se foi atualizada (ou ignorada).
    ///
    /// Custo: O(min(grau(a), grau(b))) para localizar a aresta, olhando a lista menor.
    /// Uma atualização de aresta existente também percorre a lista maior (O(grau máximo)).
    pub fn upsert_edge(&mut self, a: NodeIdx, b: NodeIdx, weight: f32, kind: EdgeKind) -> bool {
        if a == b {
            return false;
        }
        let (short, long) = if self.adj[a as usize].len() <= self.adj[b as usize].len() {
            (a, b)
        } else {
            (b, a)
        };
        let found = self.adj[short as usize]
            .iter()
            .position(|e| e.to == long && e.kind == kind);
        match found {
            Some(pos) => {
                self.adj[short as usize][pos].weight = weight;
                if let Some(e) = self.adj[long as usize]
                    .iter_mut()
                    .find(|e| e.to == short && e.kind == kind)
                {
                    e.weight = weight;
                }
                false
            }
            None => {
                self.adj[a as usize].push(Edge { to: b, weight, kind });
                self.adj[b as usize].push(Edge { to: a, weight, kind });
                self.edge_counts[kind as usize] += 1;
                true
            }
        }
    }

    /// Remove a aresta `a — b` do tipo informado. Devolve `true` se ela existia.
    /// Custo: O(grau(a) + grau(b)). A ordem relativa das demais arestas é preservada.
    pub fn remove_edge(&mut self, a: NodeIdx, b: NodeIdx, kind: EdgeKind) -> bool {
        let before = self.adj[a as usize].len();
        self.adj[a as usize].retain(|e| !(e.to == b && e.kind == kind));
        if self.adj[a as usize].len() == before {
            return false;
        }
        self.adj[b as usize].retain(|e| !(e.to == a && e.kind == kind));
        self.edge_counts[kind as usize] -= 1;
        true
    }

    /// Peso da aresta `a — b` do tipo informado, se existir.
    pub fn edge_weight(&self, a: NodeIdx, b: NodeIdx, kind: EdgeKind) -> Option<f32> {
        let (short, long) = if self.adj[a as usize].len() <= self.adj[b as usize].len() {
            (a, b)
        } else {
            (b, a)
        };
        self.adj[short as usize]
            .iter()
            .find(|e| e.to == long && e.kind == kind)
            .map(|e| e.weight)
    }

    /// Indica se existe a aresta `a — b` do tipo informado.
    pub fn has_edge(&self, a: NodeIdx, b: NodeIdx, kind: EdgeKind) -> bool {
        self.edge_weight(a, b, kind).is_some()
    }

    /// Ordena cada lista de adjacência por relevância: peso decrescente; em caso de
    /// empate, vizinho de maior grau primeiro (mais popular); depois índice crescente.
    ///
    /// Depois disso, `neighbors(v).iter().take(n)` devolve os `n` vizinhos mais
    /// relevantes em O(n), o que permite limitar a expansão de vértices muito
    /// populares ("hubs") durante o percurso. Custo: O(E log Δ), com Δ o grau máximo.
    /// Deve ser chamado após cargas em lote.
    pub fn sort_neighbors_by_relevance(&mut self) {
        let degrees: Vec<usize> = self.adj.iter().map(|list| list.len()).collect();
        for list in self.adj.iter_mut() {
            list.sort_by(|a, b| {
                b.weight
                    .total_cmp(&a.weight)
                    .then_with(|| degrees[b.to as usize].cmp(&degrees[a.to as usize]))
                    .then_with(|| a.to.cmp(&b.to))
            });
        }
    }

    /// Menor caminho (em número de arestas) entre dois vértices, por **BFS** com fila
    /// (`VecDeque`) e mapa de pais. A busca para na profundidade `max_depth`. Se `only`
    /// for `Some(tipo)`, só atravessa arestas desse tipo.
    /// Custo: O(V + E) no pior caso; na prática visita apenas a região próxima da origem.
    pub fn shortest_path(
        &self,
        from: NodeIdx,
        to: NodeIdx,
        max_depth: usize,
        only: Option<EdgeKind>,
    ) -> Option<Vec<NodeIdx>> {
        if from == to {
            return Some(vec![from]);
        }
        let mut parent: HashMap<NodeIdx, NodeIdx> = HashMap::new();
        let mut queue: VecDeque<(NodeIdx, usize)> = VecDeque::new();
        parent.insert(from, from);
        queue.push_back((from, 0));
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for edge in &self.adj[current as usize] {
                if let Some(kind) = only {
                    if edge.kind != kind {
                        continue;
                    }
                }
                if parent.contains_key(&edge.to) {
                    continue;
                }
                parent.insert(edge.to, current);
                if edge.to == to {
                    let mut path = vec![to];
                    let mut node = to;
                    while node != from {
                        node = parent[&node];
                        path.push(node);
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back((edge.to, depth + 1));
            }
        }
        None
    }

    /// Estimativa (em bytes) da memória usada pelas estruturas do grafo: listas de
    /// adjacência, tabelas de vértices e índice hash. É uma aproximação.
    pub fn approx_memory_bytes(&self) -> usize {
        let edge_slots: usize = self.adj.iter().map(|list| list.capacity()).sum();
        let edges = edge_slots * size_of::<Edge>();
        let headers = self.adj.capacity() * size_of::<Vec<Edge>>();
        let node_tables =
            self.kinds.capacity() * size_of::<NodeKind>() + self.ids.capacity() * size_of::<u32>();
        let index = self.index.capacity() * (size_of::<((NodeKind, u32), NodeIdx)>() + 1);
        edges + headers + node_tables + index
    }

    /// Memória que uma **matriz de adjacência** densa (`f32` por par de vértices)
    /// ocuparia para este mesmo grafo: V² × 4 bytes. Serve apenas para comparação.
    pub fn dense_matrix_bytes(&self) -> u128 {
        let n = self.node_count() as u128;
        n * n * (size_of::<f32>() as u128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// c0 - p0 - c1 - p1 - c2
    fn line_graph() -> (Graph, Vec<NodeIdx>) {
        let mut g = Graph::new();
        let c0 = g.add_node(NodeKind::Customer, 0);
        let p0 = g.add_node(NodeKind::Product, 0);
        let c1 = g.add_node(NodeKind::Customer, 1);
        let p1 = g.add_node(NodeKind::Product, 1);
        let c2 = g.add_node(NodeKind::Customer, 2);
        g.upsert_edge(c0, p0, 1.0, EdgeKind::Purchase);
        g.upsert_edge(c1, p0, 1.0, EdgeKind::Purchase);
        g.upsert_edge(c1, p1, 1.0, EdgeKind::Purchase);
        g.upsert_edge(c2, p1, 1.0, EdgeKind::Purchase);
        (g, vec![c0, p0, c1, p1, c2])
    }

    #[test]
    fn add_node_is_idempotent_and_kind_aware() {
        let mut g = Graph::new();
        let a = g.add_node(NodeKind::Customer, 7);
        let b = g.add_node(NodeKind::Customer, 7);
        let c = g.add_node(NodeKind::Product, 7);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.node(NodeKind::Product, 7), Some(c));
        assert_eq!(g.node(NodeKind::Category, 7), None);
        assert_eq!(g.kind(c), NodeKind::Product);
        assert_eq!(g.id(c), 7);
    }

    #[test]
    fn upsert_edge_keeps_a_single_undirected_edge() {
        let mut g = Graph::new();
        let a = g.add_node(NodeKind::Customer, 1);
        let b = g.add_node(NodeKind::Product, 1);
        assert!(g.upsert_edge(a, b, 0.5, EdgeKind::Purchase));
        assert!(!g.upsert_edge(b, a, 0.9, EdgeKind::Purchase));
        assert_eq!(g.edge_count(), 1);
        assert_eq!(g.neighbors(a).len(), 1);
        assert_eq!(g.neighbors(b).len(), 1);
        assert_eq!(g.edge_weight(a, b, EdgeKind::Purchase), Some(0.9));
        assert_eq!(g.edge_weight(b, a, EdgeKind::Purchase), Some(0.9));
    }

    #[test]
    fn self_loops_are_ignored() {
        let mut g = Graph::new();
        let a = g.add_node(NodeKind::Product, 1);
        assert!(!g.upsert_edge(a, a, 1.0, EdgeKind::BelongsTo));
        assert_eq!(g.edge_count(), 0);
        assert_eq!(g.degree(a), 0);
    }

    #[test]
    fn remove_edge_removes_both_directions() {
        let (mut g, n) = line_graph();
        assert!(g.has_edge(n[0], n[1], EdgeKind::Purchase));
        assert!(g.remove_edge(n[1], n[0], EdgeKind::Purchase));
        assert!(!g.has_edge(n[0], n[1], EdgeKind::Purchase));
        assert_eq!(g.degree(n[0]), 0);
        assert_eq!(g.edge_count(), 3);
        assert!(!g.remove_edge(n[0], n[1], EdgeKind::Purchase));
    }

    #[test]
    fn shortest_path_returns_a_minimal_path() {
        let (g, n) = line_graph();
        assert_eq!(g.shortest_path(n[0], n[2], 10, None), Some(vec![n[0], n[1], n[2]]));
        assert_eq!(g.shortest_path(n[0], n[4], 10, None), Some(n.clone()));
        assert_eq!(g.shortest_path(n[3], n[3], 10, None), Some(vec![n[3]]));
    }

    #[test]
    fn shortest_path_respects_max_depth() {
        let (g, n) = line_graph();
        assert_eq!(g.shortest_path(n[0], n[4], 3, None), None);
        assert!(g.shortest_path(n[0], n[4], 4, None).is_some());
    }

    #[test]
    fn shortest_path_can_be_restricted_to_an_edge_kind() {
        let mut g = Graph::new();
        let c0 = g.add_node(NodeKind::Customer, 0);
        let p0 = g.add_node(NodeKind::Product, 0);
        let cat = g.add_node(NodeKind::Category, 0);
        let p1 = g.add_node(NodeKind::Product, 1);
        g.upsert_edge(c0, p0, 1.0, EdgeKind::Purchase);
        g.upsert_edge(p0, cat, 1.0, EdgeKind::BelongsTo);
        g.upsert_edge(p1, cat, 1.0, EdgeKind::BelongsTo);
        assert_eq!(g.shortest_path(c0, p1, 10, Some(EdgeKind::Purchase)), None);
        assert_eq!(g.shortest_path(c0, p1, 10, None), Some(vec![c0, p0, cat, p1]));
    }

    #[test]
    fn shortest_path_is_none_for_disconnected_vertices() {
        let (mut g, n) = line_graph();
        let lonely = g.add_node(NodeKind::Customer, 99);
        assert_eq!(g.shortest_path(n[0], lonely, 10, None), None);
    }

    #[test]
    fn sort_orders_by_weight_then_neighbor_degree() {
        let mut g = Graph::new();
        let hub = g.add_node(NodeKind::Product, 0);
        let a = g.add_node(NodeKind::Customer, 1);
        let b = g.add_node(NodeKind::Customer, 2);
        let c = g.add_node(NodeKind::Customer, 3);
        let d = g.add_node(NodeKind::Customer, 4);
        let extra = g.add_node(NodeKind::Product, 5);
        g.upsert_edge(hub, a, 0.4, EdgeKind::Purchase);
        g.upsert_edge(hub, b, 1.0, EdgeKind::Purchase);
        g.upsert_edge(hub, c, 0.8, EdgeKind::Purchase);
        g.upsert_edge(hub, d, 0.8, EdgeKind::Purchase);
        // d tem grau 2 e c tem grau 1: no empate de peso, d vem primeiro.
        g.upsert_edge(d, extra, 0.5, EdgeKind::Purchase);
        g.sort_neighbors_by_relevance();
        let order: Vec<NodeIdx> = g.neighbors(hub).iter().map(|e| e.to).collect();
        assert_eq!(order, vec![b, d, c, a]);
    }

    #[test]
    fn dense_matrix_is_far_larger_than_adjacency_lists() {
        let mut g = Graph::new();
        let mut previous = g.add_node(NodeKind::Product, 0);
        for i in 1..1000u32 {
            let current = g.add_node(NodeKind::Product, i);
            g.upsert_edge(previous, current, 1.0, EdgeKind::BelongsTo);
            previous = current;
        }
        assert_eq!(g.dense_matrix_bytes(), 1000u128 * 1000 * 4);
        assert!((g.approx_memory_bytes() as u128) * 10 < g.dense_matrix_bytes());
    }
}
