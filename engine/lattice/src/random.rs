use terrain::mix;
use voxel_space::{GridId, VoxelCoordinate};

/// A 64-bit random number for a voxel of a grid at a tick.
///
/// It is a function of its arguments and nothing else, bit for bit the same on every machine: a
/// rule that decides by chance whether something happens at a voxel asks for the number of that
/// voxel at that tick, so the world's history does not depend on the order voxels are looked at.
/// `purpose` tells apart the decisions a rule makes about one voxel (or the rules that make them),
/// so that they are independent of each other. Every bit is as good as every other.
pub fn voxel_random(
    seed: u64,
    tick: u64,
    grid: GridId,
    voxel: VoxelCoordinate,
    purpose: u64,
) -> u64 {
    let mut hash = mix(seed ^ purpose.rotate_left(32));
    for part in [
        tick,
        grid.get(),
        voxel.x() as u64,
        voxel.y() as u64,
        voxel.z() as u64,
    ] {
        hash = mix(hash ^ part);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voxel(x: i128, y: i128, z: i128) -> VoxelCoordinate {
        VoxelCoordinate::new(x, y, z)
    }

    #[test]
    fn the_numbers_are_pinned_because_worlds_depend_on_them() {
        // What fire and any later chance does in a saved world depends on these exact numbers, so
        // they must never change: a different mix is a different function with a new name.
        let main = GridId::MAIN;
        assert_eq!(voxel_random(0, 0, main, voxel(0, 0, 0), 0), PINNED[0]);
        assert_eq!(
            voxel_random(2024, 77, main, voxel(-3, 100, 5), 1),
            PINNED[1]
        );
        assert_eq!(
            voxel_random(
                u64::MAX,
                u64::MAX,
                GridId::new(3),
                voxel(1 << 40, -7, 9),
                99
            ),
            PINNED[2]
        );
    }

    /// Computed independently, with Python's arbitrary-precision integers.
    const PINNED: [u64; 3] = [
        0xCBD3_7AD2_9B93_B094,
        0x3A11_198A_9F70_B2AD,
        0xD5C1_52C1_3A89_00A5,
    ];

    #[test]
    fn every_argument_changes_the_number() {
        let main = GridId::MAIN;
        let base = voxel_random(5, 10, main, voxel(1, 2, 3), 0);
        let others = [
            voxel_random(6, 10, main, voxel(1, 2, 3), 0),
            voxel_random(5, 11, main, voxel(1, 2, 3), 0),
            voxel_random(5, 10, GridId::new(1), voxel(1, 2, 3), 0),
            voxel_random(5, 10, main, voxel(2, 2, 3), 0),
            voxel_random(5, 10, main, voxel(1, 3, 3), 0),
            voxel_random(5, 10, main, voxel(1, 2, 4), 0),
            voxel_random(5, 10, main, voxel(1, 2, 3), 1),
            // The same coordinates in another order are not the same voxel.
            voxel_random(5, 10, main, voxel(3, 2, 1), 0),
        ];
        for (index, other) in others.iter().enumerate() {
            assert_ne!(*other, base, "argument {index}");
        }
        assert_eq!(voxel_random(5, 10, main, voxel(1, 2, 3), 0), base);
    }

    #[test]
    fn the_bits_are_balanced_and_independent_enough_to_decide_things_by() {
        // Over a block of voxels and ticks, each of the 64 bits is set about half the time, and
        // two neighbouring voxels' bits agree about half the time.
        let mut ones = [0u32; 64];
        let mut agreements = 0u32;
        let samples = 20_000u32;
        for index in 0..samples {
            let x = i128::from(index % 40) - 20;
            let tick = u64::from(index / 40);
            let here = voxel_random(1, tick, GridId::MAIN, voxel(x, 3, -9), 0);
            let next = voxel_random(1, tick, GridId::MAIN, voxel(x + 1, 3, -9), 0);
            for (bit, count) in ones.iter_mut().enumerate() {
                *count += ((here >> bit) & 1) as u32;
            }
            agreements += (!(here ^ next)).count_ones();
        }
        for (bit, &count) in ones.iter().enumerate() {
            let share = f64::from(count) / f64::from(samples);
            assert!((0.47..0.53).contains(&share), "bit {bit}: {share}");
        }
        let share = f64::from(agreements) / (f64::from(samples) * 64.0);
        assert!((0.49..0.51).contains(&share), "{share}");
    }
}
