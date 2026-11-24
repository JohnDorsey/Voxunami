//! Which mechanisms are one: groups and ships that are alone, each with a frame of its own, that are near each other are the sides of one contact ghost, and what is near them
//! chains (a mechanism near a second that is near a third has all three).

use std::collections::{BTreeMap, BTreeSet};

use voxel_space::GridId;

/// The clusters of `mechanisms`, each by the lowest anchor in it, with its anchors in order. A mechanism is joined to another (a higher one) when a ship of the other's group is in
/// what is `near` it; the clusters are the pieces that are joined, so the order the pairs are looked at in makes no difference. `members_of` gives the ships of the group that an
/// anchor speaks for (the anchor itself too).
///
/// The cost is the ships near the mechanisms, not the mechanisms squared: a world in which every one of 400 ships on a rail has a ship near it asked each mechanism about each other
/// one, 20 ms a tick, measured 2026-11-18.
pub(super) fn clusters_of(
    anchors: &[GridId],
    near: &BTreeMap<GridId, BTreeSet<GridId>>,
    members_of: impl Fn(GridId) -> Vec<GridId>,
) -> BTreeMap<GridId, Vec<GridId>> {
    let anchor_of: BTreeMap<GridId, GridId> = anchors
        .iter()
        .flat_map(|anchor| {
            members_of(*anchor)
                .into_iter()
                .map(move |ship| (ship, *anchor))
        })
        .collect();
    let mut cluster_of: BTreeMap<GridId, GridId> = anchors.iter().map(|a| (*a, *a)).collect();
    for first in anchors {
        for ship in &near[first] {
            let Some(second) = anchor_of.get(ship) else {
                continue;
            };
            if second > first {
                let (kept, absorbed) = (cluster_of[first], cluster_of[second]);
                if kept != absorbed {
                    let (low, high) = (kept.min(absorbed), kept.max(absorbed));
                    for cluster in cluster_of.values_mut() {
                        if *cluster == high {
                            *cluster = low;
                        }
                    }
                }
            }
        }
    }
    let mut clusters: BTreeMap<GridId, Vec<GridId>> = BTreeMap::new();
    for (anchor, cluster) in &cluster_of {
        clusters.entry(*cluster).or_default().push(*anchor);
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program as it was (2026-11-17): every anchor against every anchor.
    fn by_looking_at_every_pair(
        anchors: &[GridId],
        near: &BTreeMap<GridId, BTreeSet<GridId>>,
        members_of: impl Fn(GridId) -> Vec<GridId>,
    ) -> BTreeMap<GridId, Vec<GridId>> {
        let mut cluster_of: BTreeMap<GridId, GridId> = anchors.iter().map(|a| (*a, *a)).collect();
        for first in anchors {
            for second in anchors {
                let is_near = members_of(*second)
                    .iter()
                    .any(|ship| near[first].contains(ship));
                if second > first && is_near {
                    let (kept, absorbed) = (cluster_of[first], cluster_of[second]);
                    if kept != absorbed {
                        let (low, high) = (kept.min(absorbed), kept.max(absorbed));
                        for cluster in cluster_of.values_mut() {
                            if *cluster == high {
                                *cluster = low;
                            }
                        }
                    }
                }
            }
        }
        let mut clusters: BTreeMap<GridId, Vec<GridId>> = BTreeMap::new();
        for (anchor, cluster) in &cluster_of {
            clusters.entry(*cluster).or_default().push(*anchor);
        }
        clusters
    }

    /// SplitMix64.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
    }

    #[test]
    fn the_clusters_are_those_of_looking_at_every_pair_of_mechanisms() {
        let mut most_in_one = 0;
        let mut more_than_one_cluster = 0;
        for seed in 0..300 {
            let mut random = Random(seed);
            // Ships 1 to 60 in runs of one to three; the first of a run is the anchor of a mechanism, and the run is its group.
            let mut groups: BTreeMap<GridId, Vec<GridId>> = BTreeMap::new();
            let mut next = 1;
            while next <= 60 {
                let size = 1 + random.next() % 3;
                let ships: Vec<GridId> = (next..next + size).map(GridId::new).collect();
                groups.insert(ships[0], ships);
                next += size;
            }
            let anchors: Vec<GridId> = groups.keys().copied().collect();
            // What is near each: a few ships of other groups and some that are no mechanism's, none of its own.
            let near: BTreeMap<GridId, BTreeSet<GridId>> = anchors
                .iter()
                .map(|anchor| {
                    let count = random.next() % 3;
                    let ships = (0..count)
                        .map(|_| GridId::new(1 + random.next() % 70))
                        .filter(|ship| !groups[anchor].contains(ship))
                        .collect();
                    (*anchor, ships)
                })
                .collect();
            let members_of = |anchor: GridId| groups[&anchor].clone();
            let clusters = clusters_of(&anchors, &near, members_of);
            assert_eq!(
                clusters,
                by_looking_at_every_pair(&anchors, &near, members_of),
                "seed {seed}"
            );
            most_in_one = most_in_one.max(clusters.values().map(Vec::len).max().unwrap());
            more_than_one_cluster += usize::from(clusters.len() > 1);
        }
        // (The inputs make clusters of several mechanisms and are not all one: the test is of something.)
        assert!(most_in_one >= 4, "{most_in_one}");
        assert!(more_than_one_cluster > 100, "{more_than_one_cluster}");
    }

    #[test]
    fn a_mechanism_near_a_second_that_is_near_a_third_is_one_cluster_by_the_lowest_anchor_and_a_pair_apart_is_another()
     {
        let [one, two, three, four, five] = [1, 2, 3, 4, 5].map(GridId::new);
        let anchors = [one, two, three, four, five];
        let near = BTreeMap::from([
            (one, BTreeSet::from([two])),
            (two, BTreeSet::from([one, three])),
            (three, BTreeSet::from([two])),
            (four, BTreeSet::from([five])),
            (five, BTreeSet::from([four])),
        ]);
        assert_eq!(
            clusters_of(&anchors, &near, |anchor| vec![anchor]),
            BTreeMap::from([(one, vec![one, two, three]), (four, vec![four, five])])
        );
    }
}
