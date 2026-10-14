//! Catálogo: cadastro e consulta de produtos, clientes e categorias.
//!
//! Usa `HashMap` para acesso por identificador em O(1) médio, e um índice
//! secundário `categoria -> produtos` para listar a categoria sem varrer o catálogo.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::models::{Category, CategoryId, Customer, CustomerId, Product, ProductId};

/// Cadastro de entidades do domínio.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    products: HashMap<ProductId, Product>,
    customers: HashMap<CustomerId, Customer>,
    categories: HashMap<CategoryId, Category>,
    by_category: HashMap<CategoryId, Vec<ProductId>>,
}

impl Catalog {
    /// Cria um catálogo vazio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cadastra uma categoria. Falha se o identificador já existir. O(1) médio.
    pub fn add_category(&mut self, category: Category) -> Result<()> {
        if self.categories.contains_key(&category.id) {
            return Err(Error::DuplicateCategory(category.id));
        }
        self.categories.insert(category.id, category);
        Ok(())
    }

    /// Cadastra um cliente. Falha se o identificador já existir. O(1) médio.
    pub fn add_customer(&mut self, customer: Customer) -> Result<()> {
        if self.customers.contains_key(&customer.id) {
            return Err(Error::DuplicateCustomer(customer.id));
        }
        self.customers.insert(customer.id, customer);
        Ok(())
    }

    /// Cadastra um produto. Falha se o identificador já existir ou se a categoria
    /// não estiver cadastrada. O(1) médio.
    pub fn add_product(&mut self, product: Product) -> Result<()> {
        if self.products.contains_key(&product.id) {
            return Err(Error::DuplicateProduct(product.id));
        }
        if !self.categories.contains_key(&product.category_id) {
            return Err(Error::UnknownCategory(product.category_id));
        }
        self.by_category
            .entry(product.category_id)
            .or_default()
            .push(product.id);
        self.products.insert(product.id, product);
        Ok(())
    }

    /// Consulta um produto pelo identificador. O(1) médio.
    pub fn product(&self, id: ProductId) -> Option<&Product> {
        self.products.get(&id)
    }

    /// Consulta um cliente pelo identificador. O(1) médio.
    pub fn customer(&self, id: CustomerId) -> Option<&Customer> {
        self.customers.get(&id)
    }

    /// Consulta uma categoria pelo identificador. O(1) médio.
    pub fn category(&self, id: CategoryId) -> Option<&Category> {
        self.categories.get(&id)
    }

    /// Quantidade de produtos cadastrados.
    pub fn product_count(&self) -> usize {
        self.products.len()
    }

    /// Quantidade de clientes cadastrados.
    pub fn customer_count(&self) -> usize {
        self.customers.len()
    }

    /// Quantidade de categorias cadastradas.
    pub fn category_count(&self) -> usize {
        self.categories.len()
    }

    /// Percorre todos os produtos (ordem não especificada).
    pub fn products(&self) -> impl Iterator<Item = &Product> + '_ {
        self.products.values()
    }

    /// Identificadores dos produtos de uma categoria, na ordem de cadastro. O(1) + tamanho da lista.
    pub fn products_in_category(&self, id: CategoryId) -> &[ProductId] {
        match self.by_category.get(&id) {
            Some(list) => list.as_slice(),
            None => &[],
        }
    }

    /// Busca produtos cujo nome contém o termo (sem diferenciar maiúsculas de minúsculas).
    /// O resultado vem ordenado por identificador. Custo: O(P · L) para P produtos de nome com L caracteres.
    pub fn search_products(&self, term: &str) -> Vec<&Product> {
        let needle = term.to_lowercase();
        let mut found: Vec<&Product> = self
            .products
            .values()
            .filter(|p| p.name.to_lowercase().contains(&needle))
            .collect();
        found.sort_by_key(|p| p.id);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(id: CategoryId, name: &str) -> Category {
        Category {
            id,
            name: name.to_string(),
        }
    }

    fn product(id: ProductId, name: &str, category_id: CategoryId) -> Product {
        Product {
            id,
            name: name.to_string(),
            category_id,
            price_cents: 1_000,
        }
    }

    fn catalog() -> Catalog {
        let mut c = Catalog::new();
        c.add_category(category(1, "Eletrônicos")).unwrap();
        c.add_category(category(2, "Livros")).unwrap();
        c.add_product(product(1, "Mouse Sem Fio", 1)).unwrap();
        c.add_product(product(2, "Teclado Mecânico", 1)).unwrap();
        c.add_product(product(3, "Livro de Rust", 2)).unwrap();
        c
    }

    #[test]
    fn registers_and_queries_by_id() {
        let c = catalog();
        assert_eq!(c.product_count(), 3);
        assert_eq!(c.category_count(), 2);
        assert_eq!(c.product(2).map(|p| p.name.as_str()), Some("Teclado Mecânico"));
        assert!(c.product(99).is_none());
        assert_eq!(c.category(2).map(|x| x.name.as_str()), Some("Livros"));
    }

    #[test]
    fn rejects_duplicate_identifiers() {
        let mut c = catalog();
        assert_eq!(c.add_category(category(1, "Outra")), Err(Error::DuplicateCategory(1)));
        assert_eq!(c.add_product(product(1, "Clone", 1)), Err(Error::DuplicateProduct(1)));
        let ana = Customer {
            id: 1,
            name: "Ana".to_string(),
        };
        assert_eq!(c.add_customer(ana.clone()), Ok(()));
        assert_eq!(c.add_customer(ana), Err(Error::DuplicateCustomer(1)));
        assert_eq!(c.product_count(), 3);
        assert_eq!(c.customer_count(), 1);
    }

    #[test]
    fn rejects_product_with_unknown_category() {
        let mut c = catalog();
        assert_eq!(c.add_product(product(10, "Órfão", 42)), Err(Error::UnknownCategory(42)));
        assert!(c.product(10).is_none());
    }

    #[test]
    fn lists_products_of_a_category() {
        let c = catalog();
        assert_eq!(c.products_in_category(1), &[1, 2]);
        assert_eq!(c.products_in_category(2), &[3]);
        assert!(c.products_in_category(99).is_empty());
    }

    #[test]
    fn search_ignores_case_and_sorts_by_id() {
        let c = catalog();
        let ids: Vec<ProductId> = c.search_products("MO").iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![1]);
        let ids: Vec<ProductId> = c.search_products("e").iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(c.search_products("geladeira").is_empty());
    }
}
