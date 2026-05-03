use std::thread;

use crate::cli::options::Options;

const ENV_VAR: &str = "MP3RGAIN_THREADS";

/// Resolve the effective worker thread count.
///
/// Priority (highest first):
/// 1. `-j N` / `--threads N` (when N > 0)
/// 2. `MP3RGAIN_THREADS` env var (when N > 0)
/// 3. `std::thread::available_parallelism()`, falling back to 1
///
/// `-j 0` and `MP3RGAIN_THREADS=0` mean "use the default (auto)".
pub fn effective_threads(opts: &Options) -> usize {
    if let Some(n) = opts.threads {
        if n > 0 {
            return n;
        }
    }
    if let Ok(s) = std::env::var(ENV_VAR) {
        if let Ok(n) = s.parse::<usize>() {
            if n > 0 {
                return n;
            }
        }
    }
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Initialize the global rayon thread pool to `n` worker threads.
///
/// Safe to call multiple times — only the first call wins. Subsequent calls
/// (and library code already using the pool) are silently ignored.
pub fn install_global_pool(n: usize) {
    let n = n.max(1);
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build_global();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_with_threads(t: Option<usize>) -> Options {
        Options {
            threads: t,
            ..Default::default()
        }
    }

    #[test]
    fn explicit_count_wins() {
        let opts = opts_with_threads(Some(3));
        assert_eq!(effective_threads(&opts), 3);
    }

    #[test]
    fn zero_falls_back_to_default() {
        let opts = opts_with_threads(Some(0));
        // Without env var, falls back to available_parallelism (>= 1).
        std::env::remove_var(ENV_VAR);
        assert!(effective_threads(&opts) >= 1);
    }

    #[test]
    fn one_means_serial() {
        let opts = opts_with_threads(Some(1));
        assert_eq!(effective_threads(&opts), 1);
    }
}
