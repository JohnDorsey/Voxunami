//! How the parameters of the planet family are written in a body file (`docs/rails-and-recipes-spec.md`, section 3.3), and the schema an editor checks them with.

use std::sync::Arc;

use super::planet::PlanetSettings;
use data_packs::{FamilyData, FamilyParameters, FamilyProblem};
use serde_json::{Map, Value, json};

/// The keys, in the order of the settings.
const KEYS: [&str; 7] = [
    "seed",
    "radius",
    "hill_height",
    "largest_hill_wavelength_log2",
    "hill_octave_count",
    "cave_wavelength_log2",
    "cave_half_thickness",
];

/// What a planet has when the file does not say: a ball, with no hills and no caves.
const DEFAULT_LARGEST_HILL_WAVELENGTH_LOG2: u8 = 10;
const DEFAULT_CAVE_WAVELENGTH_LOG2: u8 = 5;
const GREATEST_WAVELENGTH_LOG2: u8 = 56;
/// 2 to the power 64, the first number that is not a `u64`.
const TWO_TO_THE_64: f64 = 18_446_744_073_709_551_616.0;

#[derive(Debug)]
pub(crate) struct PlanetData;

impl PlanetData {
    pub(crate) fn shared() -> Arc<dyn FamilyData> {
        Arc::new(Self)
    }
}

fn problem(path: &str, message: impl Into<String>) -> FamilyProblem {
    FamilyProblem {
        path: path.to_owned(),
        message: message.into(),
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// A whole number from 0 up, written as `7` or as `7.0` (JSON Schema's `integer` takes both).
fn whole_number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0 && *number >= 0.0 && *number < TWO_TO_THE_64)
            .map(|number| number as u64)
    })
}

struct Reader<'a> {
    object: &'a Map<String, Value>,
    problems: Vec<FamilyProblem>,
    /// Whether every field that is there has the kind of value it should; the numbers are only judged then.
    typed: bool,
}

impl Reader<'_> {
    fn number(&mut self, key: &str, default: Option<f64>) -> f64 {
        match self.object.get(key) {
            None => match default {
                Some(default) => default,
                None => {
                    self.missing(key);
                    0.0
                }
            },
            Some(value) => value.as_f64().unwrap_or_else(|| {
                self.typed = false;
                self.problems.push(problem(
                    &format!("/{key}"),
                    format!("expected a number, found {}", kind_of(value)),
                ));
                0.0
            }),
        }
    }

    /// A whole number from 0 to `maximum`.
    fn small_integer(&mut self, key: &str, default: u8, maximum: u8) -> u8 {
        let Some(value) = self.object.get(key) else {
            return default;
        };
        let Some(number) = whole_number(value) else {
            self.typed = false;
            let found = match value {
                Value::Number(number) => number.to_string(),
                other => kind_of(other).to_owned(),
            };
            self.problems.push(problem(
                &format!("/{key}"),
                format!("expected a whole number, found {found}"),
            ));
            return default;
        };
        if number > u64::from(maximum) {
            self.typed = false;
            self.problems.push(problem(
                &format!("/{key}"),
                format!("{number} is outside 0 to {maximum}"),
            ));
            return default;
        }
        number as u8
    }

    fn seed(&mut self, world_seed: u64) -> u64 {
        match self.object.get("seed") {
            None => {
                self.missing("seed");
                0
            }
            Some(Value::String(text)) if text == "world" => world_seed,
            Some(value) => whole_number(value).unwrap_or_else(|| {
                self.typed = false;
                self.problems.push(problem(
                    "/seed",
                    "a seed is a whole number from 0 up, or \"world\"",
                ));
                0
            }),
        }
    }

    fn missing(&mut self, key: &str) {
        self.typed = false;
        self.problems
            .push(problem("", format!("missing \"{key}\"")));
    }
}

impl FamilyData for PlanetData {
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["seed", "radius"],
            "additionalProperties": false,
            "properties": {
                "seed": {
                    "oneOf": [{ "type": "integer", "minimum": 0 }, { "const": "world" }],
                    "description": "what the noise of the hills and caves starts from: a whole number, or \"world\" for the seed of the world the planet is made in"
                },
                "radius": {
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "maximum": 1.0e9,
                    "description": "voxels from the centre to the surface where the hills add nothing"
                },
                "hill_height": {
                    "type": "number",
                    "minimum": 0,
                    "default": 0,
                    "description": "the height the largest hills add (the sum over all the octaves can reach about twice this), in voxels"
                },
                "largest_hill_wavelength_log2": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": GREATEST_WAVELENGTH_LOG2,
                    "default": DEFAULT_LARGEST_HILL_WAVELENGTH_LOG2,
                    "description": "the wavelength of the largest hills is 2 to this power voxels"
                },
                "hill_octave_count": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 255,
                    "default": 0,
                    "description": "how many sizes of hills there are, each half the wavelength of the one before; 0 for none"
                },
                "cave_wavelength_log2": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": GREATEST_WAVELENGTH_LOG2,
                    "default": DEFAULT_CAVE_WAVELENGTH_LOG2,
                    "description": "the wavelength of the sheet caves is 2 to this power voxels"
                },
                "cave_half_thickness": {
                    "type": "number",
                    "minimum": 0,
                    "default": 0,
                    "description": "half the thickness of the cave sheets, in noise units (the noise ranges over -1 to 1); 0 for no caves"
                }
            }
        })
    }

    fn read(
        &self,
        parameters: &Value,
        world_seed: u64,
    ) -> Result<FamilyParameters, Vec<FamilyProblem>> {
        let Some(object) = parameters.as_object() else {
            return Err(vec![problem(
                "",
                format!("expected an object, found {}", kind_of(parameters)),
            )]);
        };
        let mut reader = Reader {
            object,
            problems: Vec::new(),
            typed: true,
        };
        for key in object.keys() {
            if !KEYS.contains(&key.as_str()) {
                let names: Vec<String> = KEYS.iter().map(|name| format!("\"{name}\"")).collect();
                reader.problems.push(problem(
                    &format!("/{key}"),
                    format!(
                        "unknown key \"{key}\"; the keys here are {}",
                        names.join(", ")
                    ),
                ));
            }
        }
        let seed = reader.seed(world_seed);
        let radius = reader.number("radius", None);
        let hill_height = reader.number("hill_height", Some(0.0));
        let largest_hill_wavelength_log2 = reader.small_integer(
            "largest_hill_wavelength_log2",
            DEFAULT_LARGEST_HILL_WAVELENGTH_LOG2,
            GREATEST_WAVELENGTH_LOG2,
        );
        let hill_octave_count = reader.small_integer("hill_octave_count", 0, u8::MAX);
        let cave_wavelength_log2 = reader.small_integer(
            "cave_wavelength_log2",
            DEFAULT_CAVE_WAVELENGTH_LOG2,
            GREATEST_WAVELENGTH_LOG2,
        );
        let cave_half_thickness = reader.number("cave_half_thickness", Some(0.0));
        let settings = PlanetSettings {
            seed,
            radius,
            hill_height,
            largest_hill_wavelength_log2,
            hill_octave_count,
            cave_wavelength_log2,
            cave_half_thickness,
        };
        // The numbers are judged when they all are numbers (what the planet is made of, and how far each may go, is the generator's to say).
        if reader.typed {
            for (field, message) in settings.problems() {
                reader.problems.push(problem(&format!("/{field}"), message));
            }
        }
        if !reader.problems.is_empty() {
            return Err(reader.problems);
        }
        Ok(FamilyParameters {
            bytes: settings.to_bytes(),
            surface_radius: radius,
        })
    }
}
