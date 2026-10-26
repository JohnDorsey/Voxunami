#![allow(dead_code)] // each test file uses a different part of this shared module

//! What the tests of bodies share: a family of generators as a game would give one, and the levels of a game.

use std::sync::Arc;

use data_packs::{FamilyData, FamilyParameters, FamilyProblem};
use namespaced_id::NamespacedId;
use serde_json::{Value, json};

pub fn id(text: &str) -> NamespacedId {
    text.parse().unwrap()
}

/// A family of the tests': a ball with a radius, and a seed that is a number or "world".
#[derive(Debug)]
pub struct Ball;

impl FamilyData for Ball {
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["radius"],
            "additionalProperties": false,
            "properties": {
                "radius": { "type": "number", "exclusiveMinimum": 0 },
                "seed": { "oneOf": [{ "type": "integer", "minimum": 0 }, { "const": "world" }] }
            }
        })
    }

    fn read(
        &self,
        parameters: &Value,
        world_seed: u64,
    ) -> Result<FamilyParameters, Vec<FamilyProblem>> {
        let problem = |path: &str, message: &str| FamilyProblem {
            path: path.to_owned(),
            message: message.to_owned(),
        };
        let Some(object) = parameters.as_object() else {
            return Err(vec![problem("", "expected an object")]);
        };
        let mut problems = Vec::new();
        for key in object.keys() {
            if key != "radius" && key != "seed" {
                problems.push(problem(&format!("/{key}"), "unknown key"));
            }
        }
        let radius = match object.get("radius").and_then(Value::as_f64) {
            Some(radius) if radius > 0.0 => radius,
            Some(_) => {
                problems.push(problem("/radius", "a ball has a radius above 0"));
                1.0
            }
            None => {
                problems.push(problem("", "missing \"radius\""));
                1.0
            }
        };
        let seed = match object.get("seed") {
            None => 0,
            Some(Value::String(text)) if text == "world" => world_seed,
            Some(other) => other.as_u64().unwrap_or_else(|| {
                problems.push(problem("/seed", "a seed is a whole number or \"world\""));
                0
            }),
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        let mut bytes = radius.to_bits().to_le_bytes().to_vec();
        bytes.extend_from_slice(&seed.to_le_bytes());
        Ok(FamilyParameters {
            bytes,
            surface_radius: radius,
        })
    }
}

pub fn families() -> Vec<(NamespacedId, Arc<dyn FamilyData>)> {
    vec![(id("test_game:ball_v1"), Arc::new(Ball))]
}

pub fn levels() -> Vec<NamespacedId> {
    vec![id("test_game:space"), id("test_game:ground")]
}
