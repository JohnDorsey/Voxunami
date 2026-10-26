//! The family of planets: the parameters of a planet as bytes (the form a world keeps them in), and the family that makes the generator of a planet from them.

use std::sync::Arc;

use voxel_grid::ChannelIndex;

use crate::generator::{FamilyError, GeneratorFamily, TerrainGenerator, TerrainMaterials};
use crate::noise::MAXIMUM_WAVELENGTH_LOG2;
use crate::planet::{PlanetSettings, planet};

/// The length of the bytes of the settings of a planet: the seed (8), the radius (8), the height of the hills (8), three single bytes, the half thickness of the caves (8).
const SETTINGS_LENGTH: usize = 35;

/// The greatest radius of a planet, in voxels: a billion, which is a thousand times the Earth's.
const GREATEST_RADIUS: f64 = 1.0e9;

impl PlanetSettings {
    /// The settings as bytes, which are a file format like the output of the generator they make: the seed, the radius, the height of the hills, the wavelength of
    /// the largest of them, their number of octaves, the wavelength of the caves and their half thickness; the numbers little-endian and the floats as their bits.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(SETTINGS_LENGTH);
        bytes.extend_from_slice(&self.seed.to_le_bytes());
        bytes.extend_from_slice(&self.radius.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.hill_height.to_bits().to_le_bytes());
        bytes.push(self.largest_hill_wavelength_log2);
        bytes.push(self.hill_octave_count);
        bytes.push(self.cave_wavelength_log2);
        bytes.extend_from_slice(&self.cave_half_thickness.to_bits().to_le_bytes());
        bytes
    }

    /// Everything that makes these settings no planet, each as the name of the field and why: a radius that is not above nothing and at most
    /// [`GREATEST_RADIUS`], a negative or not finite height of the hills or thickness of the caves, a wavelength past the greatest the noise takes (where the
    /// generator would panic or make nonsense). In the order of the fields.
    pub fn problems(&self) -> Vec<(&'static str, String)> {
        let mut problems = Vec::new();
        if !(self.radius > 0.0 && self.radius <= GREATEST_RADIUS) {
            problems.push((
                "radius",
                format!(
                    "the radius of a planet is above nothing and at most {GREATEST_RADIUS} voxels, not {}",
                    self.radius
                ),
            ));
        }
        if !(self.hill_height.is_finite() && self.hill_height >= 0.0) {
            problems.push((
                "hill_height",
                format!(
                    "the height of the hills of a planet is not negative and finite, not {}",
                    self.hill_height
                ),
            ));
        }
        for (field, wavelength_log2) in [
            (
                "largest_hill_wavelength_log2",
                self.largest_hill_wavelength_log2,
            ),
            ("cave_wavelength_log2", self.cave_wavelength_log2),
        ] {
            if wavelength_log2 > MAXIMUM_WAVELENGTH_LOG2 {
                problems.push((
                    field,
                    format!(
                        "a wavelength of 2^{wavelength_log2} voxels is more than the noise takes (2^{MAXIMUM_WAVELENGTH_LOG2})"
                    ),
                ));
            }
        }
        if !(self.cave_half_thickness.is_finite() && self.cave_half_thickness >= 0.0) {
            problems.push((
                "cave_half_thickness",
                format!(
                    "the half thickness of the caves of a planet is not negative and finite, not {}",
                    self.cave_half_thickness
                ),
            ));
        }
        problems
    }

    /// The settings that [`PlanetSettings::to_bytes`] wrote. Refuses the wrong length and settings that are no planet ([`PlanetSettings::problems`]).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FamilyError> {
        let bytes: &[u8; SETTINGS_LENGTH] = bytes.try_into().map_err(|_| {
            FamilyError::new(format!(
                "the settings of a planet are {SETTINGS_LENGTH} bytes, not {}",
                bytes.len()
            ))
        })?;
        let float = |start: usize| {
            f64::from_bits(u64::from_le_bytes(
                bytes[start..start + 8].try_into().expect("eight bytes"),
            ))
        };
        let settings = Self {
            seed: u64::from_le_bytes(bytes[0..8].try_into().expect("eight bytes")),
            radius: float(8),
            hill_height: float(16),
            largest_hill_wavelength_log2: bytes[24],
            hill_octave_count: bytes[25],
            cave_wavelength_log2: bytes[26],
            cave_half_thickness: float(27),
        };
        if let Some((_, message)) = settings.problems().into_iter().next() {
            return Err(FamilyError::new(message));
        }
        Ok(settings)
    }
}

/// The planets: a family whose parameters are the bytes of [`PlanetSettings`], writing the material of a voxel into one channel.
#[derive(Clone, Debug)]
pub struct PlanetFamily {
    material_channel: ChannelIndex,
    materials: TerrainMaterials,
}

impl PlanetFamily {
    pub fn new(material_channel: ChannelIndex, materials: TerrainMaterials) -> Self {
        Self {
            material_channel,
            materials,
        }
    }
}

impl GeneratorFamily for PlanetFamily {
    fn build(&self, parameters: &[u8]) -> Result<Arc<dyn TerrainGenerator>, FamilyError> {
        let settings = PlanetSettings::from_bytes(parameters)?;
        Ok(Arc::new(planet(
            settings,
            self.material_channel,
            self.materials,
        )))
    }
}

#[cfg(test)]
mod tests {
    use voxel_grid::test_support::StandardTestLayout;
    use voxel_space::BrickCoordinate;

    use super::*;
    use crate::planet::test_fixtures::{MATERIALS, small_planet};
    use crate::planet::{PlanetSettings, planet};

    fn family(fixture: &StandardTestLayout) -> PlanetFamily {
        PlanetFamily::new(fixture.material, MATERIALS)
    }

    #[test]
    fn the_bytes_of_the_settings_of_a_planet_are_these_in_this_order() {
        // The bytes are a file format (a world keeps them in its recipes): the seed, the radius, the height of the hills, the wavelength of the largest, their
        // number of octaves, the wavelength of the caves and their half thickness; the numbers little-endian, the floats as their bits.
        let settings = PlanetSettings {
            seed: 0x0102_0304_0506_0708,
            radius: 63_710.0,
            hill_height: 160.0,
            largest_hill_wavelength_log2: 10,
            hill_octave_count: 9,
            cave_wavelength_log2: 5,
            cave_half_thickness: 0.08,
        };
        let mut expected = Vec::new();
        expected.extend_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        expected.extend_from_slice(&63_710.0f64.to_bits().to_le_bytes());
        expected.extend_from_slice(&160.0f64.to_bits().to_le_bytes());
        expected.extend_from_slice(&[10, 9, 5]);
        expected.extend_from_slice(&0.08f64.to_bits().to_le_bytes());
        assert_eq!(settings.to_bytes(), expected);
        assert_eq!(expected.len(), 35);
        // (And one number written out, so that the test is not only the code over again: the seed's first byte is the lowest.)
        assert_eq!(&settings.to_bytes()[..3], &[0x08, 0x07, 0x06]);
    }

    #[test]
    fn settings_come_back_from_their_bytes_exactly() {
        for settings in [
            small_planet(5),
            PlanetSettings::one_percent_of_earth(2024),
            PlanetSettings {
                seed: u64::MAX,
                radius: 0.5,
                hill_height: 0.0,
                largest_hill_wavelength_log2: 0,
                hill_octave_count: 255,
                cave_wavelength_log2: 56,
                cave_half_thickness: 0.0,
            },
        ] {
            assert_eq!(
                PlanetSettings::from_bytes(&settings.to_bytes()),
                Ok(settings)
            );
        }
    }

    #[test]
    fn bytes_that_are_not_the_settings_of_a_planet_are_refused_and_do_not_panic() {
        let good = small_planet(5).to_bytes();
        // The wrong length, either way.
        assert!(PlanetSettings::from_bytes(&[]).is_err());
        assert!(PlanetSettings::from_bytes(&good[..34]).is_err());
        let mut longer = good.clone();
        longer.push(0);
        assert!(PlanetSettings::from_bytes(&longer).is_err());
        // Numbers that are no planet: a radius that is nothing, negative, not a number, infinite or more than a billion voxels; hills with a negative or a
        // non-finite height; a wavelength past the greatest the noise takes; caves with a negative or a non-finite thickness.
        let with = |change: &dyn Fn(&mut PlanetSettings)| {
            let mut settings = small_planet(5);
            change(&mut settings);
            settings.to_bytes()
        };
        for bytes in [
            with(&|s| s.radius = 0.0),
            with(&|s| s.radius = -3.0),
            with(&|s| s.radius = f64::NAN),
            with(&|s| s.radius = f64::INFINITY),
            with(&|s| s.radius = 1.0e9 + 1.0),
            with(&|s| s.hill_height = -1.0),
            with(&|s| s.hill_height = f64::NAN),
            with(&|s| s.hill_height = f64::INFINITY),
            with(&|s| s.largest_hill_wavelength_log2 = 57),
            with(&|s| s.cave_wavelength_log2 = 57),
            with(&|s| s.cave_half_thickness = -0.1),
            with(&|s| s.cave_half_thickness = f64::NAN),
            with(&|s| s.cave_half_thickness = f64::INFINITY),
        ] {
            assert!(PlanetSettings::from_bytes(&bytes).is_err());
        }
        // The edges that are allowed.
        assert!(PlanetSettings::from_bytes(&with(&|s| s.radius = 1.0e9)).is_ok());
        assert!(
            PlanetSettings::from_bytes(&with(&|s| s.largest_hill_wavelength_log2 = 56)).is_ok()
        );
    }

    #[test]
    fn settings_that_are_no_planet_say_every_field_that_is_wrong_in_the_order_of_the_fields() {
        assert!(small_planet(5).problems().is_empty());
        let nonsense = PlanetSettings {
            seed: 1,
            radius: -1.0,
            hill_height: f64::NAN,
            largest_hill_wavelength_log2: 60,
            hill_octave_count: 3,
            cave_wavelength_log2: 57,
            cave_half_thickness: f64::INFINITY,
        };
        let fields: Vec<&str> = nonsense
            .problems()
            .iter()
            .map(|(field, _)| *field)
            .collect();
        assert_eq!(
            fields,
            [
                "radius",
                "hill_height",
                "largest_hill_wavelength_log2",
                "cave_wavelength_log2",
                "cave_half_thickness"
            ]
        );
        // The bytes say the first.
        let error = PlanetSettings::from_bytes(&nonsense.to_bytes()).unwrap_err();
        assert_eq!(error.to_string(), nonsense.problems()[0].1);
    }

    #[test]
    fn the_family_makes_the_generator_of_the_planet_its_bytes_say() {
        let fixture = StandardTestLayout::default();
        let settings = small_planet(77);
        let built = family(&fixture).build(&settings.to_bytes()).unwrap();
        let direct = planet(settings, fixture.material, MATERIALS);
        // Bricks of the middle, the surface, the sky, and a seam where three faces meet.
        for coordinate in [
            BrickCoordinate::new(0, 0, 0),
            BrickCoordinate::new(0, 15, 0),
            BrickCoordinate::new(0, 16, 0),
            BrickCoordinate::new(9, 9, 9),
            BrickCoordinate::new(-9, 9, 9),
            BrickCoordinate::new(0, 40, 0),
        ] {
            assert_eq!(
                built.generate_brick(&fixture.layout, coordinate),
                direct.generate_brick(&fixture.layout, coordinate),
                "at {coordinate:?}"
            );
        }
        // What it says about the planet is the same too.
        assert_eq!(built.solid_core(), direct.solid_core());
    }

    #[test]
    fn two_planets_of_different_seeds_are_different_planets() {
        let fixture = StandardTestLayout::default();
        let family = family(&fixture);
        let first = family.build(&small_planet(1).to_bytes()).unwrap();
        let second = family.build(&small_planet(2).to_bytes()).unwrap();
        let surface = BrickCoordinate::new(3, 15, 2);
        assert_ne!(
            first.generate_brick(&fixture.layout, surface),
            second.generate_brick(&fixture.layout, surface)
        );
    }

    #[test]
    fn the_family_refuses_bytes_that_are_no_planet_and_says_why() {
        let fixture = StandardTestLayout::default();
        let family = family(&fixture);
        let error = family.build(&[1, 2, 3]).unwrap_err();
        assert!(
            error.to_string().contains("35"),
            "the message says how long the bytes should be: {error}"
        );
        let mut settings = small_planet(1);
        settings.radius = -1.0;
        let error = family.build(&settings.to_bytes()).unwrap_err();
        assert!(error.to_string().contains("radius"), "{error}");
        // (A family is an object the world keeps and the loader calls from any thread.)
        fn requires_thread_safety<T: Send + Sync>(_: &T) {}
        requires_thread_safety(&Arc::new(family));
    }
}
