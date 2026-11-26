//! Merge episode and chapter payment routes.

use musicindex_value::percentages;

use crate::PaymentRoute;

/// Combines song and episode routes as ADR 0041 §What The Producer Writes For An Episode File requires.
#[must_use]
pub fn merged_routes(
    episode: &[PaymentRoute],
    song: &[PaymentRoute],
    remote_percentage: f64,
) -> Vec<PaymentRoute> {
    let episode_shares = route_percentages(episode);
    let fee_total: f64 = episode
        .iter()
        .zip(&episode_shares)
        .filter_map(|(route, share)| (route.fee == Some(true)).then_some(*share).flatten())
        .sum();
    let remainder = 100.0 - fee_total;
    let episode_routes: Vec<_> = episode
        .iter()
        .zip(&episode_shares)
        .filter(|(route, _)| route.fee != Some(true))
        .map(|(route, share)| (route, *share))
        .collect();
    let song_shares = route_percentages(song);
    let song_usable = song_shares.iter().any(Option::is_some);
    let song_part = if !song_usable {
        0.0
    } else if episode_routes.iter().all(|(_, share)| share.is_none()) {
        remainder
    } else {
        remote_percentage * remainder / 100.0
    };
    let episode_part = remainder - song_part;
    let non_fee_shares = percentages(
        &episode_routes
            .iter()
            .map(|(_, share)| *share)
            .collect::<Vec<_>>(),
    );

    let mut output = scaled_routes(song, &song_shares, song_part);
    output.extend(scaled_selected(
        &episode_routes,
        &non_fee_shares,
        episode_part,
    ));
    output.extend(
        episode
            .iter()
            .zip(episode_shares)
            .filter(|(route, _)| route.fee == Some(true))
            .filter_map(|(route, share)| share.map(|share| scaled_route(route, share)))
            .flatten(),
    );
    complete_or_empty(output)
}

/// Replaces episode recipients with local chapter routes as ADR 0041 §What The Producer Writes For An Episode File requires.
#[must_use]
pub fn local_split_routes(episode: &[PaymentRoute], local: &[PaymentRoute]) -> Vec<PaymentRoute> {
    let episode_shares = route_percentages(episode);
    let fee_total: f64 = episode
        .iter()
        .zip(&episode_shares)
        .filter_map(|(route, share)| (route.fee == Some(true)).then_some(*share).flatten())
        .sum();
    let remainder = 100.0 - fee_total;
    let mut output = episode
        .iter()
        .zip(&episode_shares)
        .filter(|(route, _)| route.fee == Some(true))
        .filter_map(|(route, share)| share.map(|share| scaled_route(route, share)))
        .flatten()
        .collect::<Vec<_>>();
    output.extend(scaled_routes(local, &route_percentages(local), remainder));
    complete_or_empty(output)
}

/// Reports if a route group contains a percentage.
#[must_use]
pub fn has_percentage(routes: &[PaymentRoute]) -> bool {
    route_percentages(routes).iter().any(Option::is_some)
}

fn complete_or_empty(routes: Vec<PaymentRoute>) -> Vec<PaymentRoute> {
    let total: f64 = routes.iter().filter_map(|route| route.split).sum();
    if (total - 100.0).abs() <= 0.01 {
        routes
    } else {
        Vec::new()
    }
}

fn route_percentages(routes: &[PaymentRoute]) -> Vec<Option<f64>> {
    percentages(&routes.iter().map(|route| route.split).collect::<Vec<_>>())
}

fn scaled_routes(routes: &[PaymentRoute], shares: &[Option<f64>], part: f64) -> Vec<PaymentRoute> {
    routes
        .iter()
        .zip(shares)
        .filter_map(|(route, share)| share.map(|share| (route, share)))
        .filter_map(|(route, share)| scaled_route(route, share * part / 100.0))
        .collect()
}

fn scaled_selected(
    routes: &[(&PaymentRoute, Option<f64>)],
    shares: &[Option<f64>],
    part: f64,
) -> Vec<PaymentRoute> {
    routes
        .iter()
        .zip(shares)
        .filter_map(|((route, _), share)| share.map(|share| (*route, share)))
        .filter_map(|(route, share)| scaled_route(route, share * part / 100.0))
        .collect()
}

fn scaled_route(route: &PaymentRoute, split: f64) -> Option<PaymentRoute> {
    let rounded = (split * 1000.0).round() / 1000.0;
    (rounded != 0.0).then(|| {
        let mut route = route.clone();
        route.split = Some(rounded);
        route
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(split: Option<f64>, fee: Option<bool>, address: &str) -> PaymentRoute {
        PaymentRoute {
            split,
            fee,
            address: Some(address.to_owned()),
            ..PaymentRoute::default()
        }
    }

    fn splits(routes: &[PaymentRoute]) -> Vec<f64> {
        routes.iter().filter_map(|route| route.split).collect()
    }

    #[test]
    fn merged_routes_match_the_adr_example() {
        let result = merged_routes(
            &[
                route(Some(95.0), None, "alice"),
                route(Some(5.0), Some(true), "fee"),
            ],
            &[
                route(Some(50.0), None, "song-a"),
                route(Some(50.0), None, "song-b"),
            ],
            90.0,
        );
        assert_eq!(splits(&result), vec![42.75, 42.75, 9.5, 5.0]);
    }

    #[test]
    fn local_split_keeps_episode_fee_and_divides_the_remainder() {
        let result = local_split_routes(
            &[
                route(Some(5.0), Some(true), "fee"),
                route(Some(95.0), None, "alice"),
            ],
            &[
                route(Some(1.0), None, "local-a"),
                route(Some(3.0), None, "local-b"),
            ],
        );
        assert_eq!(splits(&result), vec![5.0, 23.75, 71.25]);
    }

    #[test]
    fn remote_percentage_endpoints_and_empty_groups_keep_valid_routes() {
        let episode = [route(Some(100.0), None, "episode")];
        let song = [route(Some(100.0), None, "song")];
        for percentage in [0.0, 50.0, 100.0] {
            let routes = merged_routes(&episode, &song, percentage);
            assert!((splits(&routes).iter().sum::<f64>() - 100.0).abs() < 0.01);
            let song_only = merged_routes(&[], &song, percentage);
            assert_eq!(splits(&song_only), vec![100.0]);
            let episode_only = merged_routes(&episode, &[], percentage);
            assert!(
                episode_only.is_empty()
                    || (splits(&episode_only).iter().sum::<f64>() - 100.0).abs() < 0.01
            );
        }
        assert!(merged_routes(&[], &[], 50.0).is_empty());
    }

    #[test]
    fn invalid_shares_and_rounded_zero_routes_are_removed() {
        let episode = [route(Some(100.0), None, "episode")];
        let song = [
            route(Some(0.0), None, "zero"),
            route(None, None, "missing"),
            route(Some(-1.0), None, "negative"),
            route(Some(1.0), None, "small"),
        ];
        let routes = merged_routes(&episode, &song, 0.0001);
        assert!(
            routes
                .iter()
                .all(|route| route.address.as_deref() != Some("small"))
        );
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].address.as_deref(), Some("episode"));
    }

    #[test]
    fn song_shares_of_zero_and_one_song_route_alone() {
        let episode = [
            route(Some(95.0), None, "alice"),
            route(Some(5.0), Some(true), "fee"),
        ];
        let zero = [route(Some(0.0), None, "a"), route(Some(0.0), None, "b")];
        let routes = merged_routes(&episode, &zero, 90.0);
        assert!(
            routes.is_empty() || (splits(&routes).iter().sum::<f64>() - 100.0).abs() < 0.01,
            "{routes:?}"
        );
        assert!(
            routes
                .iter()
                .all(|route| route.split.is_some_and(|split| split > 0.0))
        );
        let one = [route(Some(7.0), None, "song")];
        let routes = merged_routes(&episode, &one, 90.0);
        assert_eq!(splits(&routes), vec![85.5, 9.5, 5.0]);
    }
}
