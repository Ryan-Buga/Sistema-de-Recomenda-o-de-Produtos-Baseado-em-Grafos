# ConectaStore — Recomendação de Produtos Baseada em Grafos

Projeto da disciplina **Data Structure Strategy and Implementation**.
Sistema de recomendação da MegaStore escrito em **Rust**, usando somente a biblioteca padrão (sem dependências externas).


## Sumário

1. [Objetivo e funcionamento](#1-objetivo-e-funcionamento)
2. [Tecnologias e estruturas utilizadas](#2-tecnologias-e-estruturas-utilizadas)
3. [Modelagem como grafo](#3-modelagem-como-grafo)
4. [Algoritmo de recomendação](#4-algoritmo-de-recomendação)
5. [Complexidade de tempo e espaço](#5-complexidade-de-tempo-e-espaço)
6. [Arquitetura da solução](#6-arquitetura-da-solução)
7. [Compilação e execução](#7-compilação-e-execução)
8. [Testes](#8-testes)
9. [Exemplos de uso](#9-exemplos-de-uso)
10. [Resultados dos testes de desempenho](#10-resultados-dos-testes-de-desempenho)
11. [Avaliação de relevância (dados sintéticos)](#11-avaliação-de-relevância-dados-sintéticos)
12. [Decisões, limitações e próximos passos](#12-decisões-limitações-e-próximos-passos)

---

## 1. Objetivo e funcionamento

A MegaStore recomenda hoje apenas os itens mais vendidos ou produtos da mesma categoria, ignorando as relações entre
clientes, compras, categorias e produtos. O ConectaStore representa essas relações como um **grafo** e **percorre**
as conexões para recomendar produtos ligados ao comportamento e aos interesses de cada cliente.

O sistema permite:

- **cadastrar e consultar** produtos, clientes e categorias (por identificador e por busca no nome);
- **construir o grafo e gerenciar as conexões** (registrar, atualizar e cancelar compras, com nota de 1 a 5);
- **recomendar produtos a partir de um cliente** ("clientes parecidos com você compraram…") **ou de um produto** ("quem comprou este também comprou…");
- **explicar** uma recomendação mostrando o caminho no grafo (BFS) entre o cliente e o produto;
- **garantir ausência de duplicatas**: cadastros repetidos são rejeitados, a mesma compra nunca vira duas arestas e nenhuma lista de recomendação repete produto nem devolve algo que o cliente já comprou;
- **medir o tempo de execução** com volumes crescentes de dados sintéticos e **comparar a relevância** com o sistema atual.

> **Resposta à pergunta do desafio.** Um catálogo grande exige que a consulta custe pouco *independentemente do tamanho do catálogo*.
> Para isso o projeto combina: (1) um grafo esparso em **lista de adjacência** (memória O(V+E)); (2) **HashMap** para acesso O(1) por identificador;
> (3) uma **BFS por níveis com profundidade limitada**, que funde caminhos repetidos e atenua "hubs"; (4) listas de vizinhos **ordenadas por relevância**,
> de modo que cada expansão olhe só os `N` vizinhos mais fortes; e (5) um **min-heap de tamanho K** para o ranking. O custo de uma recomendação passa a
> depender de `N` e `K`, e não de quantos milhões de produtos existem.

---

## 2. Tecnologias e estruturas utilizadas

- **Rust** (edição 2021), apenas `std`. Recomendado: Rust estável 1.70 ou superior.
- `#![forbid(unsafe_code)]`: o projeto não usa `unsafe`.

| Estrutura | Onde é usada | Por que foi escolhida |
|---|---|---|
| `Vec<Vec<Edge>>` (lista de adjacência) | `graph.rs` | Grafo esparso: memória O(V+E), iteração dos vizinhos em O(grau) e boa localidade de cache. Uma matriz custaria O(V²). |
| `Edge { to: u32, weight: f32, kind }` | `graph.rs` | 12 bytes por aresta: índices `u32` (até 4 bilhões de vértices) e `f32` bastam para pesos entre 0 e 1. |
| `HashMap<(NodeKind, u32), NodeIdx>` | `graph.rs` | Traduz o identificador externo em índice denso do grafo em O(1) médio. |
| `HashMap<ProductId, Product>` (e clientes, categorias) | `catalog.rs` | Cadastro e consulta por identificador em O(1) médio. |
| `HashMap<CategoryId, Vec<ProductId>>` | `catalog.rs` | Índice secundário: lista os produtos de uma categoria sem varrer o catálogo. |
| `HashSet<NodeIdx>` | `recommender.rs` | Conjunto de produtos já comprados: filtra candidatos em O(1) e impede recomendar o que o cliente já tem. |
| `Accumulator` (`HashMap` + `Vec`) | `recommender.rs` | Soma scores por vértice em O(1) médio e **funde caminhos repetidos** (evita duplicatas). O `Vec` preserva a ordem de inserção, o que torna a soma de floats determinística. |
| `BinaryHeap<Reverse<Candidate>>` (min-heap de tamanho K) | `recommender.rs` | Seleciona os K melhores de m candidatos em O(m log K) e memória O(K), sem ordenar todos os candidatos. |
| `VecDeque` (fila) | `graph.rs` | Fila da BFS de menor caminho (`explain`). |
| Adjacência ordenada por relevância | `Graph::sort_neighbors_by_relevance` | Faz `take(N)` devolver os N vizinhos mais fortes em O(N): controle de "hubs" sem varrer listas enormes. |

---

## 3. Modelagem como grafo

| Elemento | Definição |
|---|---|
| **Vértices** | `Cliente`, `Produto` e `Categoria` (grafo heterogêneo). Cada vértice tem um índice denso `u32`. |
| **Arestas** | `Purchase` (Cliente–Produto): compra com avaliação. `BelongsTo` (Produto–Categoria): pertencimento, peso 1.0. |
| **Pesos** | Compra com nota `n` (1 a 5): `n/5`, ou seja, de 0,2 a 1,0. Compra sem nota: 0,6. Representa a **intensidade do interesse**. |
| **Tipo do grafo** | **Não direcionado**, **ponderado**, **esparso**, sem laços e sem arestas paralelas do mesmo tipo (recompra atualiza o peso). É quase bipartido (clientes × produtos), com a camada de categorias por cima. |

A relação de **co-compra** entre produtos (dois produtos comprados pelo mesmo cliente) **não é materializada** como aresta:
ela é obtida percorrendo `produto → cliente → produto`. Materializá-la custaria O(Σ grau(cliente)²) arestas, o que explode em memória.

```
        (Cliente Ana) ──0.8── (Produto Mouse) ──1.0── (Categoria Eletrônicos)
                                     │                        │
                                   1.0                      1.0
                                     │                        │
                              (Cliente Bruno) ──1.0── (Produto Teclado)
```

### Lista de adjacência × matriz de adjacência

| Critério | Lista de adjacência (**adotada**) | Matriz de adjacência |
|---|---|---|
| Memória | O(V + E) | O(V²) |
| Percorrer vizinhos de um vértice | O(grau) | O(V) |
| Testar se existe a aresta (u, v) | O(min(grau(u), grau(v))) | O(1) |
| Inserir vértice | O(1) amortizado | O(V²) (realocar) |
| Adequada para | grafos **esparsos** (o caso aqui) | grafos densos |

Exemplo: com 100 mil produtos e 100 mil clientes (≈ 200 mil vértices), uma matriz de `f32` ocuparia 200.000² × 4 B ≈ **149 GB**,
enquanto as listas ocupam algumas dezenas de MB. Com 1 milhão de produtos e 1 milhão de clientes, a matriz passaria de **14 TB**.
O comando `bench` imprime essa comparação ("Matriz equivalente") em cada cenário.

---

## 4. Algoritmo de recomendação

### Por que BFS (e não DFS)

A recomendação precisa dos vizinhos **mais próximos primeiro**, porque a relevância decai com a distância no grafo.
A BFS processa o grafo por **níveis** (distância 1, 2, 3…) e permite **fundir** os caminhos que chegam ao mesmo vértice.
A DFS mergulharia por um único caminho até o limite de profundidade e enumeraria caminhos repetidos, e os resultados ficariam dependentes da ordem de visita.
A BFS também é o algoritmo certo para o menor caminho usado em `explain` (explicação da recomendação).

### Recomendação para um cliente (BFS de 3 níveis)

```
nível 0: o cliente
nível 1: produtos que ele comprou                         (peso = nota/5)
nível 2: outros clientes que compraram esses produtos     (clientes "parecidos")
nível 3: produtos desses clientes que ele ainda não tem   → candidatos
```

1. Em cada nível, vértices alcançados por vários caminhos são **fundidos**: o score é somado e o vértice é expandido uma única vez.
   O custo depende do número de **arestas por nível**, não do número de caminhos.
2. Ao sair de um vértice intermediário `v`, o sinal é multiplicado por `1 / ln(2 + grau(v))` (ideia do índice de Adamic–Adar):
   dois clientes que compartilham um produto muito popular são menos "parecidos" do que dois que compartilham um item de nicho,
   e um cliente que comprou milhares de itens "endossa" cada um deles menos.
3. **Ramo de categorias**: `cliente → produtos → categoria → outros produtos da categoria`, com score proporcional à participação da categoria
   nas compras do cliente e à popularidade (`ln(1 + grau)`) do produto. Isso cobre produtos novos, sem compradores em comum.
4. Cada ramo é normalizado pelo seu máximo e combinado: `score = (1 − β) · colaborativo + β · categoria` (β = `category_weight`, padrão 0,25).
5. **Prevenção de duplicatas**: `HashSet` de produtos já comprados (filtrados *antes* de qualquer limite) e `Accumulator` que funde candidatos repetidos.
6. **Top-K** com min-heap. Desempate determinístico: menor identificador primeiro.

### Recomendação para um produto

`produto → clientes que o compraram → outros produtos desses clientes` (co-compra), mais o ramo de categoria (produtos populares da mesma categoria).
Nunca devolve o próprio produto.

### Controle de "hubs"

Produtos muito populares têm milhares de arestas. Cada expansão examina no máximo `max_neighbors` arestas (padrão 200) por vértice.
Depois de `optimize()`, as listas estão ordenadas por peso decrescente (desempate: vizinho de maior grau), então esses são os vizinhos **mais fortes**.

### Parâmetros (`RecommendConfig`)

| Campo | Padrão | Significado |
|---|---:|---|
| `top_k` | 10 | quantidade de produtos devolvidos |
| `max_neighbors` | 200 | máximo de arestas examinadas por vértice em cada expansão |
| `category_weight` | 0,25 | peso β do ramo de categorias (0 = só comportamento de compra) |

---

## 5. Complexidade de tempo e espaço

Notação: V vértices, E arestas, P produtos, `d` produtos comprados pelo cliente, `N = max_neighbors`, `K = top_k`, `m` candidatos, Δ grau máximo.

| Operação | Tempo | Observações |
|---|---|---|
| Cadastrar / consultar por id | O(1) médio | `HashMap` |
| Buscar produto pelo nome | O(P · L) | varredura (L = tamanho do nome); poderia usar índice invertido |
| Inserir vértice | O(1) médio | |
| Inserir / atualizar aresta | O(min(grau(a), grau(b))) | atualização de aresta existente também percorre a lista maior |
| Remover aresta | O(grau(a) + grau(b)) | |
| `optimize()` (ordenar adjacências) | O(E log Δ) | feito uma vez após carga em lote |
| **Recomendar para cliente** | **O(d · N²)** operações de hash no pior caso + O(m log K) | **independe de V, E e P** |
| **Recomendar para produto** | **O(N²)** + O(m log K) | idem |
| `explain` (BFS de menor caminho) | O(V + E) no pior caso, limitado por `max_depth` | |

**Espaço:** o grafo ocupa **O(V + E)** (2E arestas de 12 bytes + 24 bytes de cabeçalho por vértice + índice hash).
O catálogo ocupa O(P + C + K). Uma consulta usa O(d · N²) no pior caso para os acumuladores e O(K) para o heap.
O número de caminhos pode ser exponencial, mas a fusão por nível mantém o trabalho proporcional às arestas por nível.

---

## 6. Arquitetura da solução

```
megastore/
├── Cargo.toml
├── README.md
├── .github/workflows/ci.yml   compila, testa e mede a cada push
├── src/
│   ├── main.rs         CLI: demo, bench, quality, repl
│   ├── lib.rs          reexporta a API pública
│   ├── models.rs       Product, Customer, Category e tipos de id
│   ├── error.rs        Error e Result
│   ├── catalog.rs      cadastro/consulta (HashMap + índice por categoria)
│   ├── graph.rs        grafo em lista de adjacência, BFS de menor caminho
│   ├── recommender.rs  BFS por níveis, deduplicação, controle de hubs, top-K
│   ├── store.rs        MegaStore: fachada que mantém catálogo e grafo consistentes
│   ├── sample.rs       cenário pequeno de demonstração
│   ├── datagen.rs      gerador de dados sintéticos (RNG próprio, reprodutível)
│   ├── bench.rs        medição de tempo com volumes diferentes
│   └── eval.rs         avaliação de relevância (leave-one-out)
└── tests/
    ├── integration_test.rs   fluxo completo, duplicatas, invariantes e tempos
    └── relevance_test.rs     grafo × sistema atual em dados sintéticos
```

```
            ┌────────────┐  cadastro   ┌───────────┐
  main.rs ─▶│ MegaStore  │────────────▶│  Catalog  │  HashMap por id
  (CLI)     │ (store.rs) │             └───────────┘
            │            │  conexões   ┌───────────┐
            │            │────────────▶│   Graph   │  lista de adjacência
            └─────┬──────┘             └─────┬─────┘
                  │ recomendar               │ vizinhos / BFS
                  ▼                          │
            ┌────────────────┐◀──────────────┘
            │  recommender   │  BFS por níveis → Accumulator → min-heap top-K
            └────────────────┘
```

Fluxo de uma recomendação: `MegaStore::recommend_for_customer` → `recommender` percorre o `Graph` por níveis, consulta o `Catalog` só para achar a categoria de cada produto (O(1)) → o `Accumulator` funde os candidatos → o heap devolve os `K` melhores.

---

## 7. Compilação e execução

Pré-requisito: [Rust](https://www.rust-lang.org/tools/install) (estável, 1.70 ou superior) com o `cargo`.

```bash
git clone <URL-DO-REPOSITORIO>
cd megastore

cargo build --release                 # compila
cargo run --release                   # demonstração (comando padrão)
cargo run --release -- repl           # console interativo
cargo run --release -- bench          # desempenho: 1.000, 10.000 e 100.000 produtos
cargo run --release -- bench 1000 10000 100000 1000000   # volumes personalizados
cargo run --release -- quality 10000  # relevância em dados sintéticos
cargo run --release -- ajuda          # lista os comandos
```

> Use sempre `--release` para medir desempenho: o modo debug é de 10 a 50 vezes mais lento.
> O cenário de 1.000.000 de produtos (1 milhão de clientes e 10 milhões de compras) usa alguns GB de RAM.

---

## 8. Testes

```bash
cargo test                                   # unitários + integração (modo debug)
cargo test --release                         # mesmo conjunto, mais rápido
cargo test --test integration_test timing_with_growing_volumes -- --nocapture   # imprime os tempos
cargo test --test relevance_test -- --nocapture                                  # imprime as taxas de acerto
```

O repositório inclui um workflow do GitHub Actions (`.github/workflows/ci.yml`) que compila, executa os testes, roda a demonstração e o benchmark a cada `push`.
A tabela do benchmark aparece no resumo da execução, e pode ser usada como segunda fonte de medições (informando o ambiente: runner `ubuntu-latest`).

- **Unitários** (dentro de cada módulo, `#[cfg(test)]`): grafo (inserção idempotente, arestas sem duplicata, remoção, BFS e profundidade máxima, ordenação), catálogo (duplicatas, categoria inexistente, busca), fachada (validação de notas e ids, recompra, remoção, explicação), recomendador (deduplicação, exclusão do que já foi comprado mesmo com limite pequeno, top-K, ramo de categorias, partida a frio, hubs, determinismo), gerador, avaliação e formatação.
- **Integração** (`tests/`): fluxo completo pela API pública (cadastro → conexões → recomendação → cancelamento), cenário de demonstração, prevenção de duplicatas, invariantes em 3.000 produtos/clientes gerados, reprodutibilidade, comparação de memória lista × matriz, tempos com volumes crescentes e relevância contra o sistema atual.

---

## 9. Exemplos de uso

### Demonstração (`cargo run --release`)

Saída esperada (o cenário de exemplo está em `src/sample.rs`; confira os valores ao executar):

```text
=== ConectaStore: demonstração ===
Catálogo: 5 categorias, 13 produtos, 6 clientes
Grafo: 24 vértices, 38 arestas (25 de compra + 13 de categoria)

[1] Consulta de produtos: nome contém "mouse"
  #2 Mouse Sem Fio R$ 89,90

[2] Recomendações para o cliente #1 (Ana)
  Já comprou: Notebook Gamer, Livro Estruturas de Dados em Rust, Mouse Sem Fio
   1. #3   Teclado Mecânico [Eletrônicos] R$ 249,90  (score 1.000)
   2. #5   Monitor 27 polegadas [Eletrônicos] R$ 1299,00  (score 0.623)
   3. #7   Livro Algoritmos e Grafos [Livros] R$ 99,90  (score 0.429)
   4. #4   Headset USB [Eletrônicos] R$ 159,90  (score 0.425)
   5. #9   Moletom Rust [Vestuário] R$ 159,90  (score 0.310)

[3] Produtos relacionados ao produto #1 (Notebook Gamer)
   1. #2   Mouse Sem Fio [Eletrônicos] R$ 89,90  (score 1.000)
   2. #3   Teclado Mecânico [Eletrônicos] R$ 249,90  (score 0.896)
   3. #5   Monitor 27 polegadas [Eletrônicos] R$ 1299,00  (score 0.852)
   4. #4   Headset USB [Eletrônicos] R$ 159,90  (score 0.476)
   5. #6   Livro Estruturas de Dados em Rust [Livros] R$ 79,90  (score 0.461)

[4] Por que o produto #3 (Teclado Mecânico) aparece para a Ana? Caminho no grafo (BFS):
  Cliente #1 (Ana) -> Produto #1 (Notebook Gamer) -> Cliente #2 (Bruno) -> Produto #3 (Teclado Mecânico)

[5] Prevenção de duplicatas
  cadastrar o produto #3 de novo -> erro: produto 3 já cadastrado
  registrar de novo a compra Ana/Notebook -> conexão existente apenas atualizada
  arestas de compra antes/depois: 25/25
```

O Teclado lidera porque três clientes parecidos com a Ana (Bruno, Carla e Elisa) o compraram. Os produtos que a Ana já tem nunca aparecem.

### Console interativo (`cargo run --release -- repl`)

```text
megastore> categoria 1 Eletrônicos
categoria 1 cadastrada
megastore> produto 1 1 8990 Mouse Sem Fio
produto 1 cadastrado
megastore> produto 2 1 24990 Teclado Mecânico
produto 2 cadastrado
megastore> cliente 1 Ana
cliente 1 cadastrado
megastore> cliente 2 Bruno
cliente 2 cadastrado
megastore> comprar 1 1 5
compra registrada
megastore> comprar 2 1 5
compra registrada
megastore> comprar 2 2 4
compra registrada
megastore> comprar 2 2 4
a compra já existia: conexão apenas atualizada (sem duplicar)
megastore> buscar mouse
  #1 Mouse Sem Fio [categoria 1] R$ 89,90
megastore> recomendar-cliente 1
   1. #2   Teclado Mecânico [Eletrônicos] R$ 249,90  (score 1.000)
megastore> explicar 1 2
  Cliente #1 (Ana) -> Produto #1 (Mouse Sem Fio) -> Cliente #2 (Bruno) -> Produto #2 (Teclado Mecânico)
megastore> produto 2 1 100 Clone
erro: produto 2 já cadastrado
megastore> sair
```

Comandos: `demo`, `categoria`, `produto`, `cliente`, `comprar`, `cancelar`, `buscar`, `ver`, `recomendar-cliente`, `recomendar-produto`, `explicar`, `stats`, `ajuda`, `sair`.
O comando `demo` carrega o cenário de exemplo dentro do console.

### Como biblioteca (API Rust)

```rust
use megastore::{MegaStore, RecommendConfig};

fn main() -> Result<(), megastore::Error> {
    let mut store = MegaStore::new();
    store.add_category(1, "Eletrônicos")?;
    store.add_product(1, "Mouse Sem Fio", 1, 8_990)?;       // id, nome, categoria, preço em centavos
    store.add_product(2, "Teclado Mecânico", 1, 24_990)?;
    store.add_customer(1, "Ana")?;
    store.add_customer(2, "Bruno")?;

    store.record_purchase(1, 1, Some(5))?;                  // cliente, produto, nota (1 a 5)
    store.record_purchase(2, 1, Some(5))?;
    store.record_purchase(2, 2, Some(4))?;
    store.optimize();                                       // ordena as adjacências após a carga

    let cfg = RecommendConfig { top_k: 5, ..RecommendConfig::default() };
    for rec in store.recommend_for_customer(1, &cfg)? {
        println!("produto {} (score {:.3})", rec.product_id, rec.score);
    }
    Ok(())
}
```

---

## 10. Resultados dos testes de desempenho

**Como reproduzir:**

```bash
cargo run --release -- bench 1000 10000 100000
```

**Metodologia.** Para cada volume `N`, o gerador (`datagen.rs`, semente fixa 42) cria `N` produtos, `N` clientes, 20 categorias e 10 compras por cliente
(10·N arestas de compra), com popularidade enviesada (alguns produtos viram "hubs"). O benchmark mede com `std::time::Instant`:
a **carga** (gerar os dados e inseri-los no catálogo e no grafo), a **ordenação das adjacências** (`optimize`) e a **latência de 1.000 recomendações**
de cada tipo (por cliente e por produto), reportando média e percentil 95. Também mostra uma **estimativa** da memória do grafo,
a memória que uma **matriz de adjacência** equivalente exigiria e o RSS do processo (somente Linux).

**Ambiente de medição:** _preencher (CPU, RAM, sistema operacional, versão do Rust)_

| Produtos | Clientes | Compras (arestas) | Vértices | Carga (gerar + inserir) | Ordenar adjacências | Rec. cliente média / p95 | Rec. produto média / p95 | Grafo (estimado) | Matriz equivalente | RSS do processo |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1.000 | _preencher_ | | | | | | | | | |
| 10.000 | _preencher_ | | | | | | | | | |
| 100.000 | _preencher_ | | | | | | | | | |
| 1.000.000 (opcional) | _preencher_ | | | | | | | | | |

_Cole aqui a saída do comando `bench`, que já é uma tabela Markdown com essas colunas._

**O que a análise deve mostrar** (confira nos seus números):

- **Carga e memória do grafo crescem de forma aproximadamente linear** com o número de arestas, como previsto por O(V + E).
- **A latência de recomendação cresce muito pouco** quando o volume é multiplicado por 10 ou 100, porque o custo depende de `d`, `N` e `K` e não do tamanho do catálogo (limite `max_neighbors`).
- **A matriz de adjacência equivalente seria inviável** (centenas de GB já com 100 mil produtos), o que justifica a lista de adjacência.

---

## 11. Avaliação de relevância (dados sintéticos)

O sistema atual da MegaStore recomenda os mais vendidos ou os mais vendidos da mesma categoria. O comando abaixo compara essas duas estratégias com o ConectaStore
usando **leave-one-out**: para cerca de 5% dos clientes, uma compra alinhada ao gosto é retida (não entra no grafo); se o produto retido aparece no top-10, conta como acerto.

```bash
cargo run --release -- quality 10000
```

| Estratégia | Hit rate @10 |
|---|---:|
| Mais vendidos (sistema atual) | _preencher_ |
| Mais vendidos da mesma categoria | _preencher_ |
| ConectaStore (grafo + BFS) | _preencher_ |

**Ressalva importante:** os dados são sintéticos. Cada cliente tem dois "gostos" (categoria + estilo dentro da categoria), e as compras seguem esses gostos com viés de popularidade.
O experimento mostra o **mecanismo** (o grafo capta afinidades finas que "mais vendidos" e "mesma categoria" não captam), mas não prova o desempenho em dados reais da loja.
O teste automatizado `tests/relevance_test.rs` verifica que o grafo supera as duas linhas de base.

---

## 12. Decisões, limitações e próximos passos

- **Partida a frio:** cliente sem compras recebe lista vazia. Uma alternativa é recomendar os mais vendidos da categoria de interesse declarada.
- **Busca por nome** é uma varredura O(P · L). Para milhões de produtos, um índice invertido (termo → produtos) resolveria.
- **Hash:** o `HashMap` padrão usa SipHash (resistente a ataques, porém mais lento). Para ids numéricos internos, um hasher mais simples reduziria o tempo de carga.
- **Memória:** `Vec<Vec<Edge>>` tem 24 bytes de cabeçalho por vértice e folga de capacidade. Um formato **CSR** (arrays contíguos `offsets` + `edges`) economizaria memória, ao custo de tornar as inserções mais caras (indicado para grafo estático).
- **Concorrência:** as consultas só leem o grafo (`&self`), então podem rodar em várias threads depois da carga (por exemplo com `std::thread::scope`).
- **Pesos e hiperparâmetros** (`β`, `max_neighbors`, atenuação) foram escolhidos por simplicidade e podem ser ajustados com avaliação em dados reais.
