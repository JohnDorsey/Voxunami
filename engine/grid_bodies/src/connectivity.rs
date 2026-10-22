//! Whether a ship is still in one piece after blocks were taken out of it: [`Connectivity`], a flood that can be given a little work at a time.
//!
//! A ship is one piece if its blocks are joined through faces. After an edit that took blocks away, only the blocks beside the places they were
//! can have been cut off from each other, so a region is grown from each of them (the *seeds*), a voxel at a time, all in turn. When two regions
//! touch they are one. If every seed ends in one region the ship is whole, and that is found as soon as they have met, which is quickly when the
//! cut did not separate anything. A region that runs out of voxels is a piece that is cut off, found in as many steps as it has voxels. When a
//! single region is left that has not run out and there are pieces that have, everything else is joined to it, and need not be flooded.
//!
//! The work is counted, never timed: [`Connectivity::step`] is given a number of voxels it may look at, so what is found and in which tick is the
//! same on every machine. The answer does not depend on how the work is divided between calls.

use std::collections::{BTreeSet, HashMap, VecDeque};

use voxel_space::VoxelCoordinate;

/// What a check has found out so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not yet: more work is needed.
    Working,
    /// The seeds are all joined: nothing was cut off.
    Whole,
    /// These pieces were cut off, each a set of voxels, in order of their first seed. Whatever else the ship has that is in none of them is one
    /// more piece, or none (if it has nothing else).
    Split {
        pieces: Vec<BTreeSet<VoxelCoordinate>>,
    },
}

#[derive(Debug)]
struct Region {
    /// The region this one was joined to, or itself.
    parent: usize,
    frontier: VecDeque<VoxelCoordinate>,
    members: Vec<VoxelCoordinate>,
    /// The region has no voxel left to look at: it is a whole piece.
    exhausted: bool,
}

/// A flood from the voxels beside a cut, done a little at a time.
#[derive(Debug)]
pub struct Connectivity {
    regions: Vec<Region>,
    owner: HashMap<VoxelCoordinate, usize>,
    /// Whose turn it is.
    next: usize,
    outcome: Outcome,
    /// How many voxels have been looked at, for whoever wants to know what a check cost.
    visited: u64,
}

const FACES: [[i64; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

impl Connectivity {
    /// A check that starts from these voxels, which must be voxels of the ship (in any order, once each is enough). Fewer than two seeds
    /// cannot be separated: that check is whole at once.
    pub fn new(seeds: impl IntoIterator<Item = VoxelCoordinate>) -> Self {
        let seeds: BTreeSet<VoxelCoordinate> = seeds.into_iter().collect();
        let mut regions = Vec::with_capacity(seeds.len());
        let mut owner = HashMap::with_capacity(seeds.len());
        for (index, seed) in seeds.into_iter().enumerate() {
            owner.insert(seed, index);
            regions.push(Region {
                parent: index,
                frontier: VecDeque::from([seed]),
                members: vec![seed],
                exhausted: false,
            });
        }
        let outcome = if regions.len() < 2 {
            Outcome::Whole
        } else {
            Outcome::Working
        };
        Self {
            regions,
            owner,
            next: 0,
            outcome,
            visited: 0,
        }
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// How many voxels have been looked at.
    pub fn visited(&self) -> u64 {
        self.visited
    }

    fn root(&self, mut region: usize) -> usize {
        while self.regions[region].parent != region {
            region = self.regions[region].parent;
        }
        region
    }

    /// Joins two regions into the one with the lower number.
    fn join(&mut self, a: usize, b: usize) {
        let (keep, lose) = if a < b { (a, b) } else { (b, a) };
        let frontier = std::mem::take(&mut self.regions[lose].frontier);
        let members = std::mem::take(&mut self.regions[lose].members);
        self.regions[lose].parent = keep;
        self.regions[keep].frontier.extend(frontier);
        self.regions[keep].members.extend(members);
    }

    /// What the regions say now.
    fn settle(&mut self) {
        let roots: Vec<usize> = (0..self.regions.len())
            .filter(|&region| self.regions[region].parent == region)
            .collect();
        let working = roots
            .iter()
            .filter(|&&region| !self.regions[region].exhausted)
            .count();
        let finished: Vec<usize> = roots
            .iter()
            .copied()
            .filter(|&region| self.regions[region].exhausted)
            .collect();
        self.outcome = match (working, finished.len()) {
            (1, 0) | (0, 0) | (0, 1) => Outcome::Whole,
            (0, _) | (1, _) => Outcome::Split {
                pieces: finished
                    .into_iter()
                    .map(|region| self.regions[region].members.iter().copied().collect())
                    .collect(),
            },
            _ => Outcome::Working,
        };
    }

    /// Looks at up to `budget` voxels more, `is_part` saying whether a voxel is part of the ship (a block, not air). Returns what is known.
    pub fn step(
        &mut self,
        mut budget: usize,
        is_part: &impl Fn(VoxelCoordinate) -> bool,
    ) -> &Outcome {
        self.settle();
        while budget > 0 && self.outcome == Outcome::Working {
            // The next region that is still working, in turn.
            let count = self.regions.len();
            let Some(region) =
                (0..count)
                    .map(|offset| (self.next + offset) % count)
                    .find(|&region| {
                        self.regions[region].parent == region && !self.regions[region].exhausted
                    })
            else {
                break;
            };
            self.next = (region + 1) % count;
            let Some(voxel) = self.regions[region].frontier.pop_front() else {
                self.regions[region].exhausted = true;
                self.settle();
                continue;
            };
            budget -= 1;
            self.visited += 1;
            let mut region = region;
            for face in FACES {
                let neighbour = VoxelCoordinate::new(
                    voxel.x() + face[0],
                    voxel.y() + face[1],
                    voxel.z() + face[2],
                );
                if !is_part(neighbour) {
                    continue;
                }
                match self.owner.get(&neighbour).map(|&other| self.root(other)) {
                    None => {
                        self.owner.insert(neighbour, region);
                        self.regions[region].frontier.push_back(neighbour);
                        self.regions[region].members.push(neighbour);
                    }
                    Some(other) if other != region => {
                        self.join(region, other);
                        region = region.min(other);
                    }
                    Some(_) => {}
                }
            }
            if self.regions[region].frontier.is_empty() {
                self.regions[region].exhausted = true;
            }
            self.settle();
        }
        &self.outcome
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn at(x: i64, y: i64, z: i64) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    /// A ship that is these voxels.
    fn ship(voxels: impl IntoIterator<Item = VoxelCoordinate>) -> HashSet<VoxelCoordinate> {
        voxels.into_iter().collect()
    }

    fn row(length: i64) -> HashSet<VoxelCoordinate> {
        ship((0..length).map(|x| at(x, 0, 0)))
    }

    /// The blocks beside the places `removed` were: where a check starts.
    fn seeds_beside(
        ship: &HashSet<VoxelCoordinate>,
        removed: &[VoxelCoordinate],
    ) -> Vec<VoxelCoordinate> {
        let mut seeds = BTreeSet::new();
        for voxel in removed {
            for face in FACES {
                let near = at(
                    voxel.x() + face[0],
                    voxel.y() + face[1],
                    voxel.z() + face[2],
                );
                if ship.contains(&near) {
                    seeds.insert(near);
                }
            }
        }
        seeds.into_iter().collect()
    }

    /// Runs a check to its end, a few voxels at a time, and returns the outcome and what it cost.
    fn run(
        ship: &HashSet<VoxelCoordinate>,
        seeds: Vec<VoxelCoordinate>,
        budget: usize,
    ) -> (Outcome, u64) {
        let mut check = Connectivity::new(seeds);
        let is_part = |voxel: VoxelCoordinate| ship.contains(&voxel);
        for _ in 0..1_000_000 {
            if *check.step(budget, &is_part) != Outcome::Working {
                break;
            }
        }
        (check.outcome().clone(), check.visited())
    }

    #[test]
    fn a_cut_in_a_row_cuts_off_the_smaller_part_and_costs_about_its_size() {
        let mut blocks = row(100);
        let gone = at(10, 0, 0);
        blocks.remove(&gone);
        let (outcome, cost) = run(&blocks, seeds_beside(&blocks, &[gone]), 1000);
        // The ten blocks to the left are a piece; the rest is whatever else the ship has.
        let Outcome::Split { pieces } = outcome else {
            panic!("it was cut");
        };
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0], (0..10).map(|x| at(x, 0, 0)).collect());
        assert!(
            cost <= 30,
            "{cost} voxels looked at to find a piece of 10 in a ship of 99"
        );
    }

    #[test]
    fn a_cut_that_cuts_nothing_off_is_found_as_soon_as_the_sides_meet() {
        // A plate of 40 by 40 with one block taken out of the middle: the four blocks beside the hole are joined round it in a few steps, and
        // the rest of the plate is never looked at.
        let mut plate = ship((0..40).flat_map(|x| (0..40).map(move |z| at(x, 0, z))));
        let gone = at(20, 0, 20);
        plate.remove(&gone);
        let (outcome, cost) = run(&plate, seeds_beside(&plate, &[gone]), 3);
        assert_eq!(outcome, Outcome::Whole);
        assert!(
            cost <= 16,
            "{cost} voxels looked at in a plate of {}",
            plate.len()
        );
    }

    #[test]
    fn a_ring_cut_in_one_place_is_whole_once_the_two_sides_have_gone_round() {
        let mut ring: Vec<VoxelCoordinate> = Vec::new();
        for x in 0..50 {
            ring.push(at(x, 0, 0));
            ring.push(at(x, 0, 50));
        }
        for z in 1..50 {
            ring.push(at(0, 0, z));
            ring.push(at(49, 0, z));
        }
        let mut blocks = ship(ring);
        let gone = at(20, 0, 0);
        blocks.remove(&gone);
        let (outcome, cost) = run(&blocks, seeds_beside(&blocks, &[gone]), 7);
        assert_eq!(outcome, Outcome::Whole);
        assert!(cost <= blocks.len() as u64, "{cost} of {}", blocks.len());
    }

    #[test]
    fn taking_a_block_that_hung_by_one_face_needs_no_work() {
        let mut blocks = row(20);
        let gone = at(19, 0, 0);
        blocks.remove(&gone);
        let (outcome, cost) = run(&blocks, seeds_beside(&blocks, &[gone]), 1);
        assert_eq!((outcome, cost), (Outcome::Whole, 0));
        let (outcome, cost) = run(&blocks, Vec::new(), 1);
        assert_eq!((outcome, cost), (Outcome::Whole, 0));
    }

    #[test]
    fn a_plus_cut_in_the_middle_gives_four_pieces_of_which_three_are_found_and_the_fourth_is_the_rest()
     {
        let mut blocks = ship(
            (0..21)
                .map(|x| at(x, 0, 10))
                .chain((0..21).map(|z| at(10, 0, z))),
        );
        let gone = at(10, 0, 10);
        blocks.remove(&gone);
        let (outcome, _) = run(&blocks, seeds_beside(&blocks, &[gone]), 1000);
        let Outcome::Split { pieces } = outcome else {
            panic!("it was cut");
        };
        // Three arms ran out; the fourth was still going, and is joined to whatever else there is: the rest.
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|piece| piece.len() == 10));
    }

    #[test]
    fn what_is_found_does_not_depend_on_how_the_work_is_divided() {
        // A tree: a trunk with two branches, cut in two places.
        let mut blocks = ship(
            (0..30)
                .map(|y| at(0, y, 0))
                .chain((1..12).map(|x| at(x, 10, 0)))
                .chain((1..9).map(|z| at(0, 20, z))),
        );
        let gone = [at(0, 5, 0), at(6, 10, 0)];
        for voxel in gone {
            blocks.remove(&voxel);
        }
        let seeds = seeds_beside(&blocks, &gone);
        let (wanted, _) = run(&blocks, seeds.clone(), 1_000_000);
        assert!(matches!(wanted, Outcome::Split { .. }));
        for budget in 1..40 {
            assert_eq!(run(&blocks, seeds.clone(), budget).0, wanted, "{budget}");
        }
    }

    #[test]
    fn a_check_given_nothing_to_do_it_with_stays_as_it_was() {
        let blocks = row(10);
        let mut check = Connectivity::new([at(0, 0, 0), at(9, 0, 0)]);
        let is_part = |voxel: VoxelCoordinate| blocks.contains(&voxel);
        assert_eq!(*check.step(0, &is_part), Outcome::Working);
        assert_eq!(check.visited(), 0);
    }
}
