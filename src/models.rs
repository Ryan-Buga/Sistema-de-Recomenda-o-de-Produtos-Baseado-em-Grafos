//! Entidades do domínio: categorias, produtos e clientes.

/// Identificador externo de uma categoria.
pub type CategoryId = u32;
/// Identificador externo de um produto.
pub type ProductId = u32;
/// Identificador externo de um cliente.
pub type CustomerId = u32;

/// Categoria do catálogo (por exemplo, "Eletrônicos").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    pub name: String,
}

/// Produto à venda. O preço é guardado em centavos para evitar erros de ponto flutuante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: ProductId,
    pub name: String,
    pub category_id: CategoryId,
    pub price_cents: u64,
}

/// Cliente cadastrado na loja.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    pub id: CustomerId,
    pub name: String,
}

impl Product {
    /// Preço formatado em reais, por exemplo `R$ 89,90`.
    pub fn price_display(&self) -> String {
        format!("R$ {},{:02}", self.price_cents / 100, self.price_cents % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_is_formatted_in_reais() {
        let product = Product {
            id: 1,
            name: "Mouse".to_string(),
            category_id: 1,
            price_cents: 8_905,
        };
        assert_eq!(product.price_display(), "R$ 89,05");
    }
}
