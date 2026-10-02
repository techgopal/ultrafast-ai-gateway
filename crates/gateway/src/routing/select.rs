//! The order in which the targets of a route are tried.

use rand::{Rng, RngExt};

use super::TargetRef;
use crate::snapshot::SnapRoute;

/// The primaries in a weighted random order, each once, then the fallbacks
/// in their order. A primary with a larger weight is likelier to come first.
pub fn plan(route: &SnapRoute, rng: &mut impl Rng) -> Vec<TargetRef> {
    let mut left: Vec<&(TargetRef, u32)> = route.primaries.iter().collect();
    let mut out = Vec::with_capacity(left.len() + route.fallbacks.len());
    while !left.is_empty() {
        let total: u64 = left.iter().map(|(_, w)| u64::from(*w)).sum();
        // Only weights of zero are left: they are tried in their order.
        let pick = if total == 0 {
            0
        } else {
            let mut at = rng.random_range(0..total);
            left.iter()
                .position(|(_, w)| {
                    let w = u64::from(*w);
                    if at < w {
                        true
                    } else {
                        at -= w;
                        false
                    }
                })
                .expect("the draw is below the total")
        };
        out.push(left.remove(pick).0.clone());
    }
    out.extend(route.fallbacks.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::BreakerSettings;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use std::time::Duration;

    fn target(name: &str) -> TargetRef {
        TargetRef {
            provider: "p".into(),
            model: name.into(),
            model_id: 0,
        }
    }

    fn route(primaries: &[(&str, u32)], fallbacks: &[&str]) -> SnapRoute {
        SnapRoute {
            id: 1,
            name: "r".into(),
            everyone: true,
            team_ids: Vec::new(),
            primaries: primaries.iter().map(|(n, w)| (target(n), *w)).collect(),
            fallbacks: fallbacks.iter().map(|n| target(n)).collect(),
            retries: 2,
            first_token_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(300),
            breaker: BreakerSettings::DEFAULT,
            cache: crate::cache::RouteCache::default(),
        }
    }

    fn names(plan: &[TargetRef]) -> Vec<&str> {
        plan.iter().map(|t| t.model.as_str()).collect()
    }

    #[test]
    fn the_first_primary_follows_the_weights() {
        let r = route(&[("a", 1), ("b", 3)], &[]);
        let mut rng = StdRng::seed_from_u64(7);
        let draws = 10_000;
        let first_b = (0..draws)
            .filter(|_| plan(&r, &mut rng)[0].model == "b")
            .count();
        let share = first_b as f64 / draws as f64;
        assert!((share - 0.75).abs() < 0.03, "b came first {share}");
    }

    #[test]
    fn three_primaries_follow_their_weights() {
        let r = route(&[("a", 1), ("b", 2), ("c", 7)], &[]);
        let mut rng = StdRng::seed_from_u64(11);
        let draws = 10_000;
        let mut count = [0usize; 3];
        for _ in 0..draws {
            match plan(&r, &mut rng)[0].model.as_str() {
                "a" => count[0] += 1,
                "b" => count[1] += 1,
                _ => count[2] += 1,
            }
        }
        for (got, want) in count.iter().zip([0.1, 0.2, 0.7]) {
            let share = *got as f64 / draws as f64;
            assert!((share - want).abs() < 0.03, "{share} vs {want}");
        }
    }

    #[test]
    fn every_primary_once_then_fallbacks_in_order() {
        let r = route(&[("a", 1), ("b", 5), ("c", 2)], &["f1", "f2"]);
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..200 {
            let p = plan(&r, &mut rng);
            let n = names(&p);
            let mut head: Vec<_> = n[..3].to_vec();
            head.sort();
            assert_eq!(head, ["a", "b", "c"]);
            assert_eq!(&n[3..], ["f1", "f2"]);
        }
    }

    #[test]
    fn a_weight_of_zero_is_tried_last_among_primaries_in_their_order() {
        let r = route(&[("a", 0), ("b", 4)], &[]);
        let mut rng = StdRng::seed_from_u64(5);
        for _ in 0..100 {
            assert_eq!(names(&plan(&r, &mut rng)), ["b", "a"]);
        }
        let r = route(&[("a", 0), ("b", 0)], &[]);
        assert_eq!(names(&plan(&r, &mut rng)), ["a", "b"]);
    }

    #[test]
    fn a_route_with_no_primaries_goes_straight_to_fallbacks() {
        let r = route(&[], &["f1", "f2"]);
        assert_eq!(
            names(&plan(&r, &mut StdRng::seed_from_u64(1))),
            ["f1", "f2"]
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_order() {
        let r = route(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)], &[]);
        let one = plan(&r, &mut StdRng::seed_from_u64(42));
        let two = plan(&r, &mut StdRng::seed_from_u64(42));
        assert_eq!(one, two);
    }
}
