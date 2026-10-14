//! Gerador de dados sintéticos (determinístico, sem dependências externas).
//!
//! O modelo imita um comportamento realista de compra:
//! * produtos populares ("hubs"): dentro de cada nicho, a escolha é enviesada para os primeiros itens;
//! * cada cliente tem dois "gostos" (categoria + estilo) e 80% das compras vêm deles;
//! * compras alinhadas ao gosto recebem notas altas (4 a 5); compras aleatórias, notas baixas (1 a 3).

use crate::models::{CategoryId, CustomerId, ProductId};
use crate::store::MegaStore;

/// Quantidade máxima de "estilos" dentro de cada categoria.
const STYLES: usize = 4;
/// Fração das compras que segue os gostos do cliente.
const TASTE_SHARE: f64 = 0.8;
/// Expoente do viés de popularidade (quanto maior, mais concentradas as compras).
const SKEW_POWER: i32 = 3;

/// Gerador pseudoaleatório SplitMix64 (pequeno, rápido e reprodutível).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// Cria o gerador a partir de uma semente.
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    /// Próximo número de 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Inteiro em `0..n` (`n` deve ser maior que zero).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Número em `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Parâmetros do conjunto de dados sintético.
#[derive(Debug, Clone)]
pub struct DatasetSpec {
    pub customers: usize,
    pub products: usize,
    pub categories: usize,
    pub purchases_per_customer: usize,
    pub seed: u64,
    /// Se maior que zero, retém uma compra de cada `holdout_every`-ésimo cliente para avaliar a relevância.
    pub holdout_every: usize,
}

impl DatasetSpec {
    /// Cenário usado nos benchmarks: tantos clientes quantos produtos, 20 categorias e 10 compras por cliente.
    pub fn scaled(products: usize, seed: u64) -> Self {
        DatasetSpec {
            customers: products,
            products,
            categories: 20,
            purchases_per_customer: 10,
            seed,
            holdout_every: 0,
        }
    }
}

/// Resultado da geração: o sistema carregado e as compras retidas para avaliação.
#[derive(Debug, Clone)]
pub struct Dataset {
    pub store: MegaStore,
    /// Pares (cliente, produto) que **não** foram registrados no grafo.
    pub held_out: Vec<(CustomerId, ProductId)>,
    pub spec: DatasetSpec,
}

/// Sorteia um produto do "nicho" (categoria, estilo), com viés para os primeiros itens.
///
/// Os produtos da categoria `c` são `c, c+K, c+2K, ...` (índice `j`); o estilo do item é `j % styles`.
fn pick_in_cell(
    rng: &mut Rng,
    products: usize,
    categories: usize,
    styles: usize,
    category: usize,
    style: usize,
) -> ProductId {
    let in_category = (products - category + categories - 1) / categories;
    let in_cell = (in_category - style + styles - 1) / styles;
    let u = rng.unit();
    let skewed = u.powi(SKEW_POWER);
    let m = ((in_cell as f64) * skewed) as usize;
    let m = m.min(in_cell - 1);
    let j = style + styles * m;
    (category + categories * j) as ProductId
}

/// Gera o conjunto de dados. **Não** chama `optimize()`: quem usa decide (e mede) esse passo.
///
/// Se `products < categories`, o número de categorias é reduzido para `products`.
pub fn generate(spec: &DatasetSpec) -> Dataset {
    let products = spec.products.max(1);
    let categories = spec.categories.clamp(1, products);
    let styles = (products / categories).clamp(1, STYLES);
    let mut rng = Rng::new(spec.seed);
    let mut store = MegaStore::new();

    for c in 0..categories {
        store
            .add_category(c as CategoryId, format!("Categoria {c}"))
            .expect("identificadores gerados são únicos");
    }
    for p in 0..products {
        let category = (p % categories) as CategoryId;
        let price = 1_000 + (p as u64 * 37) % 90_000;
        store
            .add_product(p as ProductId, format!("Produto {p}"), category, price)
            .expect("identificadores gerados são únicos");
    }
    for c in 0..spec.customers {
        store
            .add_customer(c as CustomerId, format!("Cliente {c}"))
            .expect("identificadores gerados são únicos");
    }

    let mut held_out: Vec<(CustomerId, ProductId)> = Vec::new();
    for c in 0..spec.customers {
        let tastes = [
            (
                rng.below(categories as u64) as usize,
                rng.below(styles as u64) as usize,
            ),
            (
                rng.below(categories as u64) as usize,
                rng.below(styles as u64) as usize,
            ),
        ];

        // (produto, nota, veio_do_gosto)
        let mut bought: Vec<(ProductId, u8, bool)> = Vec::with_capacity(spec.purchases_per_customer);
        for t in 0..spec.purchases_per_customer {
            let from_taste = rng.unit() < TASTE_SHARE;
            let (category, style) = if from_taste {
                tastes[t % 2]
            } else {
                (
                    rng.below(categories as u64) as usize,
                    rng.below(styles as u64) as usize,
                )
            };
            let mut product = pick_in_cell(&mut rng, products, categories, styles, category, style);
            let mut tries = 0;
            while bought.iter().any(|b| b.0 == product) && tries < 5 {
                product = pick_in_cell(&mut rng, products, categories, styles, category, style);
                tries += 1;
            }
            if bought.iter().any(|b| b.0 == product) {
                continue;
            }
            let rating = if from_taste {
                4 + rng.below(2) as u8
            } else {
                1 + rng.below(3) as u8
            };
            bought.push((product, rating, from_taste));
        }

        let hold = spec.holdout_every > 0 && c % spec.holdout_every == 0 && bought.len() >= 3;
        let held_index = if hold {
            bought.iter().rposition(|b| b.2)
        } else {
            None
        };
        for (i, &(product, rating, _)) in bought.iter().enumerate() {
            if Some(i) == held_index {
                held_out.push((c as CustomerId, product));
                continue;
            }
            store
                .record_purchase(c as CustomerId, product, Some(rating))
                .expect("ids e notas gerados são válidos");
        }
    }

    Dataset {
        store,
        held_out,
        spec: spec.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DatasetSpec {
        DatasetSpec {
            customers: 300,
            products: 200,
            categories: 5,
            purchases_per_customer: 8,
            seed: 11,
            holdout_every: 10,
        }
    }

    #[test]
    fn same_seed_produces_the_same_dataset() {
        let a = generate(&spec());
        let b = generate(&spec());
        assert_eq!(a.store.stats().purchase_edges, b.store.stats().purchase_edges);
        assert_eq!(a.held_out, b.held_out);
    }

    #[test]
    fn respects_requested_sizes() {
        let d = generate(&spec());
        let stats = d.store.stats();
        assert_eq!(stats.customers, 300);
        assert_eq!(stats.products, 200);
        assert_eq!(stats.categories, 5);
        assert!(stats.purchase_edges > 0);
        assert!(stats.purchase_edges <= 300 * 8);
    }

    #[test]
    fn held_out_purchases_are_not_in_the_graph() {
        let d = generate(&spec());
        assert!(!d.held_out.is_empty());
        for &(customer, product) in &d.held_out {
            let owned = d.store.purchased_products(customer).unwrap();
            assert!(!owned.contains(&product));
        }
    }

    #[test]
    fn tiny_specs_do_not_panic() {
        let tiny = DatasetSpec {
            customers: 2,
            products: 3,
            categories: 10,
            purchases_per_customer: 5,
            seed: 1,
            holdout_every: 0,
        };
        let d = generate(&tiny);
        assert_eq!(d.store.stats().products, 3);
        assert_eq!(d.store.stats().categories, 3);
    }

    #[test]
    fn rng_is_reproducible_and_in_range() {
        let mut a = Rng::new(5);
        let mut b = Rng::new(5);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
            let x = a.unit();
            assert!((0.0..1.0).contains(&x));
            b.unit();
            assert!(a.below(7) < 7);
            b.below(7);
        }
    }
}
