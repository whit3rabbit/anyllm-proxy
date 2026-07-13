//! The frontier: deterministic, monotone, batched. `F(n)` is a pure function of
//! message count. Messages with index `< F(n)` and not Immutable are ELIGIBLE.

#[derive(Clone, Debug)]
pub struct FrontierPolicy {
    /// Messages in the last `keep_recent` turns are always verbatim. Default 4 (two
    /// user+assistant exchanges); the latest message is separately Immutable.
    pub keep_recent: usize,
    /// Hysteresis: the frontier advances only in steps of `batch_k` messages. Bigger K
    /// = fewer invalidation events on implicit-cache providers, slower savings ramp. On
    /// Anthropic (breakpoint-managed) K can be small (2).
    pub batch_k: usize,
}

impl Default for FrontierPolicy {
    fn default() -> Self {
        Self {
            keep_recent: 4,
            batch_k: 4,
        }
    }
}

/// `F(n)`: pure function of message count. Monotone in `n`; moves in steps of
/// `batch_k`. Invariant: `frontier(n+1, p) >= frontier(n, p)`.
pub fn frontier(n_messages: usize, p: &FrontierPolicy) -> usize {
    let eligible_end = n_messages.saturating_sub(p.keep_recent);
    let k = p.batch_k.max(1);
    eligible_end - (eligible_end % k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batched_and_bounded() {
        let p = FrontierPolicy {
            keep_recent: 4,
            batch_k: 4,
        };
        assert_eq!(frontier(0, &p), 0);
        assert_eq!(frontier(4, &p), 0);
        assert_eq!(frontier(7, &p), 0); // eligible_end=3, floor to 0
        assert_eq!(frontier(8, &p), 4); // eligible_end=4
        assert_eq!(frontier(11, &p), 4); // eligible_end=7 -> floor 4
        assert_eq!(frontier(12, &p), 8);
    }

    #[test]
    fn monotone_in_n() {
        let p = FrontierPolicy::default();
        let mut prev = 0;
        for n in 0..500 {
            let f = frontier(n, &p);
            assert!(f >= prev, "frontier decreased at n={n}: {f} < {prev}");
            prev = f;
        }
    }

    #[test]
    fn batch_k_zero_is_safe() {
        let p = FrontierPolicy {
            keep_recent: 0,
            batch_k: 0,
        };
        assert_eq!(frontier(10, &p), 10); // k.max(1) == 1
    }
}
