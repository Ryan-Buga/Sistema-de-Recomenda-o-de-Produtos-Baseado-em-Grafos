//! # ConectaStore: recomendação de produtos baseada em grafos
//!
//! Biblioteca do sistema de recomendação da MegaStore. Clientes, produtos e categorias são
//! vértices de um grafo ponderado e não direcionado; compras (com nota) e pertencimento a
//! categorias são arestas. A recomendação percorre o grafo com BFS por níveis.
//!
//! Módulos:
//! * [`models`]: entidades (`Product`, `Customer`, `Category`);
//! * [`catalog`]: cadastro e consulta por identificador (`HashMap`);
//! * [`graph`]: grafo em lista de adjacência e BFS de menor caminho;
//! * [`recommender`]: algoritmo de recomendação (BFS por níveis, deduplicação, top-K);
//! * [`store`]: fachada que mantém catálogo e grafo consistentes;
//! * [`datagen`], [`bench`] e [`eval`]: dados sintéticos, medição de tempo e avaliação de relevância;
//! * [`sample`]: cenário pequeno de demonstração.

#![forbid(unsafe_code)]

pub mod bench;
pub mod catalog;
pub mod datagen;
pub mod error;
pub mod eval;
pub mod graph;
pub mod models;
pub mod recommender;
pub mod sample;
pub mod store;

pub use error::{Error, Result};
pub use models::{Category, CategoryId, Customer, CustomerId, Product, ProductId};
pub use recommender::{RecommendConfig, Recommendation};
pub use store::{MegaStore, PathStep, StoreStats};
