//! Erros possíveis nas operações de cadastro, conexão e recomendação.

use std::fmt;

use crate::models::{CategoryId, CustomerId, ProductId};

/// Erro das operações do sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    DuplicateCategory(CategoryId),
    DuplicateProduct(ProductId),
    DuplicateCustomer(CustomerId),
    UnknownCategory(CategoryId),
    UnknownProduct(ProductId),
    UnknownCustomer(CustomerId),
    /// A nota de avaliação deve estar entre 1 e 5.
    InvalidRating(u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DuplicateCategory(id) => write!(f, "categoria {id} já cadastrada"),
            Error::DuplicateProduct(id) => write!(f, "produto {id} já cadastrado"),
            Error::DuplicateCustomer(id) => write!(f, "cliente {id} já cadastrado"),
            Error::UnknownCategory(id) => write!(f, "categoria {id} não encontrada"),
            Error::UnknownProduct(id) => write!(f, "produto {id} não encontrado"),
            Error::UnknownCustomer(id) => write!(f, "cliente {id} não encontrado"),
            Error::InvalidRating(r) => write!(f, "nota inválida: {r} (use um valor de 1 a 5)"),
        }
    }
}

impl std::error::Error for Error {}

/// Resultado padrão das operações do sistema.
pub type Result<T> = std::result::Result<T, Error>;
