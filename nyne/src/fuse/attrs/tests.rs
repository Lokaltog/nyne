use std::time::Duration;

use rstest::rstest;

use super::resolve_attr_ttl;
use crate::router::CachePolicy;

const FALLBACK: Duration = Duration::from_secs(7);

#[rstest]
#[case::default_uses_fallback(CachePolicy::Default, FALLBACK)]
#[case::no_cache_zero(CachePolicy::NoCache, Duration::ZERO)]
#[case::ttl_passthrough(CachePolicy::Ttl(Duration::from_millis(250)), Duration::from_millis(250))]
#[case::ttl_zero_passthrough(CachePolicy::Ttl(Duration::ZERO), Duration::ZERO)]
fn resolve_attr_ttl_honors_policy(#[case] policy: CachePolicy, #[case] expected: Duration) {
    assert_eq!(resolve_attr_ttl(policy, FALLBACK), expected);
}
