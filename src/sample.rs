//! Cenário de exemplo pequeno e legível, usado na demonstração, no REPL e nos testes.

use crate::store::MegaStore;

/// Monta o cenário de exemplo da MegaStore: 5 categorias, 13 produtos, 6 clientes e suas compras
/// (com notas de 1 a 5). O produto 13 ainda não tem compradores.
pub fn demo_store() -> MegaStore {
    let mut store = MegaStore::new();

    let categories = [
        (1, "Eletrônicos"),
        (2, "Livros"),
        (3, "Vestuário"),
        (4, "Casa e Decoração"),
        (5, "Alimentos"),
    ];
    for (id, name) in categories {
        store.add_category(id, name).expect("categoria de exemplo");
    }

    let products = [
        (1, "Notebook Gamer", 1, 549_900),
        (2, "Mouse Sem Fio", 1, 8_990),
        (3, "Teclado Mecânico", 1, 24_990),
        (4, "Headset USB", 1, 15_990),
        (5, "Monitor 27 polegadas", 1, 129_900),
        (6, "Livro Estruturas de Dados em Rust", 2, 7_990),
        (7, "Livro Algoritmos e Grafos", 2, 9_990),
        (8, "Camiseta Programador", 3, 5_990),
        (9, "Moletom Rust", 3, 15_990),
        (10, "Luminária de Mesa", 4, 12_990),
        (11, "Cadeira Ergonômica", 4, 89_900),
        (12, "Café Especial 500g", 5, 4_990),
        (13, "Livro Clean Code", 2, 8_990),
    ];
    for (id, name, category, price) in products {
        store
            .add_product(id, name, category, price)
            .expect("produto de exemplo");
    }

    let customers = [
        (1, "Ana"),
        (2, "Bruno"),
        (3, "Carla"),
        (4, "Diego"),
        (5, "Elisa"),
        (6, "Fábio"),
    ];
    for (id, name) in customers {
        store.add_customer(id, name).expect("cliente de exemplo");
    }

    // (cliente, produto, nota)
    let purchases = [
        (1, 1, 5),
        (1, 2, 4),
        (1, 6, 5),
        (2, 1, 5),
        (2, 2, 5),
        (2, 3, 5),
        (2, 4, 4),
        (2, 5, 4),
        (3, 2, 4),
        (3, 3, 5),
        (3, 6, 5),
        (3, 7, 5),
        (3, 9, 5),
        (4, 7, 4),
        (4, 8, 4),
        (4, 10, 4),
        (4, 11, 5),
        (4, 12, 5),
        (5, 1, 4),
        (5, 3, 4),
        (5, 5, 5),
        (5, 11, 4),
        (6, 10, 5),
        (6, 11, 4),
        (6, 12, 5),
    ];
    for (customer, product, rating) in purchases {
        store
            .record_purchase(customer, product, Some(rating))
            .expect("compra de exemplo");
    }

    store.optimize();
    store
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_store_has_the_documented_size() {
        let stats = demo_store().stats();
        assert_eq!(stats.categories, 5);
        assert_eq!(stats.products, 13);
        assert_eq!(stats.customers, 6);
        assert_eq!(stats.vertices, 24);
        assert_eq!(stats.membership_edges, 13);
        assert_eq!(stats.purchase_edges, 25);
    }
}
