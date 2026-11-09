//! Ships that are coupled by their joint ends are kept as one: a group is as near as its nearest ship, and awake while any ship of it is. A car left frozen while the car it is
//! coupled to goes on would hold it fixed (a frozen ship is a fixed body), and a car put in a file would lose its joint, so the lifecycle looks at the distance of the group.
//! Which ships are coupled is what the server world holds (`ServerWorld::coupled_ships`).

use std::collections::{BTreeMap, BTreeSet};

use voxel_space::GridId;

/// What the lifecycle has seen of a ship: how far it is from the nearest player (squared), and whether it is in memory and not frozen. A ship in a file is not awake.
pub(crate) struct Member {
    pub id: GridId,
    pub squared_away: f64,
    pub awake: bool,
}

/// The groups that the coupled ships make, each named by its lowest identifier. A ship that is coupled to none is no group: it is asked as it is.
#[derive(Default)]
pub(crate) struct Groups {
    names: BTreeMap<GridId, GridId>,
    /// By the name of the group: the squared distance of its nearest ship, and whether any is awake.
    nearest: BTreeMap<GridId, f64>,
    awake: BTreeSet<GridId>,
}

impl Groups {
    /// The groups made by these couplings, with what is seen of the ships that are in them. `members` is not looked at when there are no couplings, which is nearly always.
    pub fn of(
        couplings: &BTreeSet<(GridId, GridId)>,
        members: impl Iterator<Item = Member>,
    ) -> Self {
        let mut groups = Self::default();
        if couplings.is_empty() {
            return groups;
        }
        let mut neighbours: BTreeMap<GridId, Vec<GridId>> = BTreeMap::new();
        for &(first, second) in couplings {
            neighbours.entry(first).or_default().push(second);
            neighbours.entry(second).or_default().push(first);
        }
        // (Ascending, so the first ship of a group that is reached is its lowest.)
        for &start in neighbours.keys() {
            if groups.names.contains_key(&start) {
                continue;
            }
            groups.names.insert(start, start);
            let mut queue = vec![start];
            while let Some(id) = queue.pop() {
                for &other in &neighbours[&id] {
                    if groups.names.insert(other, start).is_none() {
                        queue.push(other);
                    }
                }
            }
        }
        for member in members {
            let Some(&name) = groups.names.get(&member.id) else {
                continue;
            };
            let nearest = groups.nearest.entry(name).or_insert(f64::INFINITY);
            *nearest = nearest.min(member.squared_away);
            if member.awake {
                groups.awake.insert(name);
            }
        }
        groups
    }

    /// The squared distance that decides what happens to this ship: its group's nearest, or its own, `own`, if it is in no group.
    pub fn squared_away(&self, id: GridId, own: f64) -> f64 {
        self.names
            .get(&id)
            .and_then(|name| self.nearest.get(name))
            .copied()
            .unwrap_or(own)
    }

    /// Whether any ship of the group is awake, or `own`, whether the ship is, if it is in no group.
    pub fn awake(&self, id: GridId, own: bool) -> bool {
        self.names
            .get(&id)
            .map_or(own, |name| self.awake.contains(name))
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::LevelIndex;

    use super::*;

    fn grid(number: u32) -> GridId {
        GridId::root(LevelIndex::new(number))
    }

    fn member(number: u32, squared_away: f64, awake: bool) -> Member {
        Member {
            id: grid(number),
            squared_away,
            awake,
        }
    }

    fn pairs(list: &[(u32, u32)]) -> BTreeSet<(GridId, GridId)> {
        list.iter().map(|&(a, b)| (grid(a), grid(b))).collect()
    }

    #[test]
    fn a_ship_that_is_coupled_to_none_is_asked_as_it_is() {
        let groups = Groups::of(&pairs(&[(1, 2)]), [member(3, 50.0, true)].into_iter());
        assert_eq!(groups.squared_away(grid(3), 50.0), 50.0);
        assert!(groups.awake(grid(3), true) && !groups.awake(grid(3), false));
    }

    #[test]
    fn coupled_ships_through_others_are_one_group_with_the_nearest_distance_and_awake_if_any_is() {
        let groups = Groups::of(
            &pairs(&[(1, 2), (3, 2), (5, 6)]),
            [
                member(1, 900.0, false),
                member(2, 400.0, false),
                member(3, 1600.0, true),
                member(5, 25.0, false),
                member(6, 100.0, false),
            ]
            .into_iter(),
        );
        for ship in [1, 2, 3] {
            assert_eq!(groups.squared_away(grid(ship), f64::NAN), 400.0);
            assert!(groups.awake(grid(ship), false));
        }
        for ship in [5, 6] {
            assert_eq!(groups.squared_away(grid(ship), f64::NAN), 25.0);
            assert!(!groups.awake(grid(ship), true));
        }
    }

    #[test]
    fn with_no_couplings_the_ships_are_not_looked_at() {
        let groups = Groups::of(
            &BTreeSet::new(),
            std::iter::once_with(|| -> Member { panic!("a ship was looked at") }),
        );
        assert_eq!(groups.squared_away(grid(1), 7.0), 7.0);
    }
}
