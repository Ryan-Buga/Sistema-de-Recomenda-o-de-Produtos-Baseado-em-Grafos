//! Medição básica de desempenho com diferentes volumes de dados.
//!
//! Para cada volume gera um conjunto sintético, mede o tempo de carga (gerar + inserir no grafo),
//! o tempo de ordenação das adjacências e a latência das recomendações (média, p50, p95 e máximo).

use std::time::{Duration, Instant};

use crate::datagen::{generate, DatasetSpec};
use crate::models::{CustomerId, ProductId};
use crate::recommender::RecommendConfig;

/// Estatísticas de latência de um conjunto de consultas.
#[derive(Debug, Clone, Copy, Default)]
pub struct LatencyStats {
    pub samples: usize,
    pub mean: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
}

/// Resultado de um cenário de benchmark.
#[derive(Debug, Clone)]
pub struct BenchRow {
    pub products: usize,
    pub customers: usize,
    pub purchases: usize,
    pub vertices: usize,
    pub load_time: Duration,
    pub optimize_time: Duration,
    pub customer_queries: LatencyStats,
    pub product_queries: LatencyStats,
    /// Média de itens devolvidos por consulta (verificação de sanidade).
    pub avg_results: f64,
    pub graph_bytes: usize,
    pub matrix_bytes: u128,
    pub rss_kb: Option<u64>,
}

/// Posição do percentil `q` (0 a 1) em uma lista ordenada de `n` elementos (método do posto mais próximo).
fn percentile_index(n: usize, q: f64) -> usize {
    let rank = (q * n as f64).ceil() as usize;
    rank.saturating_sub(1).min(n - 1)
}

/// Resume uma lista de durações.
pub fn summarize(mut durations: Vec<Duration>) -> LatencyStats {
    if durations.is_empty() {
        return LatencyStats::default();
    }
    durations.sort();
    let n = durations.len();
    let total: Duration = durations.iter().sum();
    LatencyStats {
        samples: n,
        mean: total / (n as u32),
        p50: durations[percentile_index(n, 0.50)],
        p95: durations[percentile_index(n, 0.95)],
        max: durations[n - 1],
    }
}

/// Memória residente do processo em KB (somente Linux; `None` nas demais plataformas).
pub fn resident_memory_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Executa um cenário com `products` produtos (e o mesmo número de clientes, 10 compras por cliente)
/// e mede `samples` consultas de cada tipo.
pub fn run_scenario(products: usize, samples: usize) -> BenchRow {
    let spec = DatasetSpec::scaled(products, 42);
    let samples = samples.max(1);

    let started = Instant::now();
    let mut dataset = generate(&spec);
    let load_time = started.elapsed();

    let started = Instant::now();
    dataset.store.optimize();
    let optimize_time = started.elapsed();

    let rss_kb = resident_memory_kb();
    let stats = dataset.store.stats();
    let cfg = RecommendConfig::default();
    let mut returned = 0usize;
    let mut queries = 0usize;

    let step = (spec.customers / samples).max(1);
    let mut customer_times = Vec::with_capacity(samples);
    for customer in (0..spec.customers).step_by(step).take(samples) {
        let started = Instant::now();
        let recs = dataset
            .store
            .recommend_for_customer(customer as CustomerId, &cfg)
            .unwrap_or_default();
        customer_times.push(started.elapsed());
        returned += recs.len();
        queries += 1;
    }

    let step = (spec.products / samples).max(1);
    let mut product_times = Vec::with_capacity(samples);
    for product in (0..spec.products).step_by(step).take(samples) {
        let started = Instant::now();
        let recs = dataset
            .store
            .recommend_for_product(product as ProductId, &cfg)
            .unwrap_or_default();
        product_times.push(started.elapsed());
        returned += recs.len();
        queries += 1;
    }

    BenchRow {
        products: stats.products,
        customers: stats.customers,
        purchases: stats.purchase_edges,
        vertices: stats.vertices,
        load_time,
        optimize_time,
        customer_queries: summarize(customer_times),
        product_queries: summarize(product_times),
        avg_results: if queries == 0 {
            0.0
        } else {
            returned as f64 / queries as f64
        },
        graph_bytes: stats.graph_memory_bytes,
        matrix_bytes: dataset.store.graph().dense_matrix_bytes(),
        rss_kb,
    }
}

/// Formata uma duração de forma legível (ns, µs, ms ou s).
pub fn fmt_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.1} µs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2} ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

/// Formata uma quantidade de bytes (B, KB, MB, GB, TB).
pub fn fmt_bytes(bytes: u128) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_computes_order_statistics() {
        let durations: Vec<Duration> = (1..=100u64).rev().map(Duration::from_millis).collect();
        let stats = summarize(durations);
        assert_eq!(stats.samples, 100);
        assert_eq!(stats.p50, Duration::from_millis(50));
        assert_eq!(stats.max, Duration::from_millis(100));
        assert!(stats.p95 >= stats.p50 && stats.p95 <= stats.max);
        assert!(stats.mean >= Duration::from_millis(50) && stats.mean <= Duration::from_millis(51));
    }

    #[test]
    fn summarize_handles_empty_input() {
        let stats = summarize(Vec::new());
        assert_eq!(stats.samples, 0);
        assert_eq!(stats.max, Duration::ZERO);
    }

    #[test]
    fn formats_durations_and_sizes() {
        assert_eq!(fmt_duration(Duration::from_nanos(900)), "900 ns");
        assert_eq!(fmt_duration(Duration::from_micros(1_500)), "1.50 ms");
        assert_eq!(fmt_duration(Duration::from_secs(2)), "2.00 s");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn small_scenario_runs_end_to_end() {
        let row = run_scenario(300, 10);
        assert_eq!(row.products, 300);
        assert_eq!(row.customers, 300);
        assert!(row.purchases > 0);
        assert_eq!(row.customer_queries.samples, 10);
        assert_eq!(row.product_queries.samples, 10);
        assert!(row.matrix_bytes > row.graph_bytes as u128);
    }
}
