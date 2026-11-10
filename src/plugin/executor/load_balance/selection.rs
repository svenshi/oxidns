// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicUsize, Ordering};

use rand::{Rng, RngExt};
use serde::Deserialize;

use crate::infra::error::{DnsError, Result};

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Strategy {
    #[default]
    RoundRobin,
    Random,
    WeightedRandom,
}

#[derive(Debug)]
pub(super) struct Selector {
    strategy: Strategy,
    count: usize,
    cursor: AtomicUsize,
    cumulative_weights: Vec<u64>,
    total_weight: u64,
}

impl Selector {
    pub fn new(strategy: Strategy, weights: impl ExactSizeIterator<Item = u32>) -> Result<Self> {
        let count = weights.len();
        let mut total_weight = 0u64;
        let mut cumulative_weights = if strategy == Strategy::WeightedRandom {
            Vec::with_capacity(count)
        } else {
            Vec::new()
        };
        for weight in weights {
            total_weight = total_weight
                .checked_add(u64::from(weight))
                .ok_or_else(|| DnsError::plugin("load_balance cumulative weight exceeds u64"))?;
            if strategy == Strategy::WeightedRandom {
                cumulative_weights.push(total_weight);
            }
        }
        Ok(Self {
            strategy,
            count,
            cursor: AtomicUsize::new(0),
            cumulative_weights,
            total_weight,
        })
    }

    pub fn select(&self) -> usize {
        match self.strategy {
            Strategy::RoundRobin => self.round_robin(),
            _ => self.select_with_rng(&mut rand::rng()),
        }
    }

    fn select_with_rng(&self, rng: &mut impl Rng) -> usize {
        match self.strategy {
            Strategy::RoundRobin => self.round_robin(),
            Strategy::Random => rng.random_range(0..self.count),
            Strategy::WeightedRandom => self.weighted_index(rng.random_range(0..self.total_weight)),
        }
    }

    fn round_robin(&self) -> usize {
        let mut current = self.cursor.load(Ordering::Relaxed);
        loop {
            let next = if current == self.count - 1 {
                0
            } else {
                current + 1
            };
            match self.cursor.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return current,
                Err(actual) => current = actual,
            }
        }
    }

    fn weighted_index(&self, ticket: u64) -> usize {
        self.cumulative_weights
            .partition_point(|&limit| limit <= ticket)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn round_robin_wraps_and_ignores_weights() {
        let selector = Selector::new(Strategy::RoundRobin, [9, 1, 2].into_iter()).unwrap();
        assert_eq!(
            (0..8).map(|_| selector.select()).collect::<Vec<_>>(),
            [0, 1, 2, 0, 1, 2, 0, 1]
        );
        assert!(selector.cumulative_weights.is_empty());
    }

    #[test]
    fn concurrent_round_robin_is_balanced() {
        let selector =
            Arc::new(Selector::new(Strategy::RoundRobin, [1, 1, 1].into_iter()).unwrap());
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let selector = selector.clone();
                std::thread::spawn(move || {
                    let mut counts = [0; 3];
                    for _ in 0..300 {
                        counts[selector.select()] += 1;
                    }
                    counts
                })
            })
            .collect();
        let mut counts = [0; 3];
        for thread in threads {
            for (total, count) in counts.iter_mut().zip(thread.join().unwrap()) {
                *total += count;
            }
        }
        assert_eq!(counts, [800; 3]);
    }

    #[test]
    fn random_selection_is_bounded_and_single_target_always_wins() {
        let mut rng = StdRng::seed_from_u64(370);
        for strategy in [
            Strategy::RoundRobin,
            Strategy::Random,
            Strategy::WeightedRandom,
        ] {
            let one = Selector::new(strategy, [u32::MAX].into_iter()).unwrap();
            let multiple = Selector::new(strategy, [u32::MAX, 1, u32::MAX].into_iter()).unwrap();
            for _ in 0..100 {
                assert_eq!(one.select_with_rng(&mut rng), 0);
                assert!(multiple.select_with_rng(&mut rng) < 3);
            }
        }
    }

    #[test]
    fn weighted_intervals_follow_exact_weights() {
        let selector = Selector::new(Strategy::WeightedRandom, [2, 1, 1].into_iter()).unwrap();
        assert_eq!(
            (0..4)
                .map(|ticket| selector.weighted_index(ticket))
                .collect::<Vec<_>>(),
            [0, 0, 1, 2]
        );
        let large =
            Selector::new(Strategy::WeightedRandom, [u32::MAX, u32::MAX].into_iter()).unwrap();
        assert_eq!(large.total_weight, u64::from(u32::MAX) * 2);
        assert_eq!(large.weighted_index(u64::from(u32::MAX) - 1), 0);
        assert_eq!(large.weighted_index(u64::from(u32::MAX)), 1);
        assert_eq!(large.weighted_index(large.total_weight - 1), 1);
    }
}
