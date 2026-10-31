//! A body file: one body of a pack, named by the file. A body is a thing in a level of the world with ground of its own that a family of terrain generators
//! makes: a planet. (`docs/specs/rails-and-recipes-spec.md`, section 3.3.)

use std::sync::Arc;

use namespaced_id::NamespacedId;
use serde_json::{Map, Value};

use crate::error::PackErrors;
use crate::family::{FamilyData, FamilyParameters};
use crate::walk::{Path, Walk, quoted};

const KEYS: [&str; 7] = [
    "level",
    "position",
    "terrain",
    "extent_bricks",
    "mass",
    "surface_gravity",
    "rail",
];
const TERRAIN_KEYS: [&str; 2] = ["family", "parameters"];
const RAIL_KEYS: [&str; 2] = ["spin", "orbit"];
const SPIN_KEYS: [&str; 3] = ["axis", "period", "angle"];
const ORBIT_KEYS: [&str; 5] = ["around", "normal", "radius", "period", "phase"];

/// The farthest a body may be from the origin of its level along an axis, in voxels.
pub const POSITION_LIMIT: f64 = 1.0e15;
/// The most bricks either way of its centre that the ground of a body may reach.
pub const GREATEST_EXTENT_BRICKS: i64 = 1_000_000;
/// The most a body may weigh, in kilograms (a ship's mass is an `f32`).
const GREATEST_MASS: f64 = f32::MAX as f64;

/// How heavy a body is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Weight {
    /// In kilograms.
    Mass(f64),
    /// As heavy as it must be to pull this hard at its surface, in metres a second a second (the game's pull and the body's radius say how heavy that is).
    SurfaceGravity(f64),
}

/// A steady turn about an axis, as a file says it: seconds and degrees, the axis as written (any direction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinData {
    pub axis: [f64; 3],
    pub period: f64,
    pub angle_degrees: f64,
}

/// What an orbit goes round: a point of the level, or another body of the world (by name; that it is there and was made before is for the world to say).
#[derive(Clone, Debug, PartialEq)]
pub enum Around {
    Point([f64; 3]),
    Body(NamespacedId),
}

/// A circle gone round at a steady rate, as a file says it.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitData {
    pub around: Around,
    pub normal: [f64; 3],
    pub radius: f64,
    pub period: f64,
    pub phase_degrees: f64,
}

/// How a body moves (`docs/specs/rails-and-recipes-spec.md`, section 3.4).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyRail {
    pub spin: Option<SpinData>,
    pub orbit: Option<OrbitData>,
}

/// A body as a file describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    /// The level it is in.
    pub level: NamespacedId,
    /// Where its centre is in the level, in voxels.
    pub position: [f64; 3],
    /// The family that makes its ground, and the parameters as the file gave them (read again, for the world it is made in, by [`read_family_parameters`]).
    pub family: NamespacedId,
    pub parameters: Value,
    /// How many bricks either way of its centre its ground reaches.
    pub extent_bricks: u32,
    pub weight: Weight,
    /// How it moves, if it does (a body with an orbit has its place from it, and the position above is where it stands only if it does not).
    pub rail: Option<BodyRail>,
}

/// What a body needs to know about the game it is for.
#[derive(Clone, Copy, Debug)]
pub struct BodyContext<'a> {
    pub families: &'a [(NamespacedId, Arc<dyn FamilyData>)],
    /// The levels a new world of the game has.
    pub levels: &'a [NamespacedId],
}

impl BodyContext<'_> {
    /// For a game that has no bodies.
    pub const NONE: BodyContext<'static> = BodyContext {
        families: &[],
        levels: &[],
    };
}

/// What a game says about its worlds so that packs can put bodies in them, in a form a game can keep: the families of generators whose parameters a body file
/// may write, and the levels a new world has.
#[derive(Clone, Debug, Default)]
pub struct BodySupport {
    pub families: Vec<(NamespacedId, Arc<dyn FamilyData>)>,
    pub levels: Vec<NamespacedId>,
}

impl BodySupport {
    pub fn context(&self) -> BodyContext<'_> {
        BodyContext {
            families: &self.families,
            levels: &self.levels,
        }
    }
}

/// The body a file describes: its identifier is the pack's name and the file's, such as `my_pack:moon` for `my_pack/bodies/moon.json`.
pub fn read_body(
    file: &str,
    pack: &str,
    name: &str,
    text: &str,
    context: &BodyContext<'_>,
) -> Result<(NamespacedId, Body), PackErrors> {
    let (mut walk, value) = Walk::parse(file, text)?;
    let root = Path::root();
    let identifier = body_identifier(&mut walk, pack, name, &root);
    let body = read_definition(&mut walk, &value, &root, context);
    match (identifier, body) {
        (Some(identifier), Some(body)) => walk.finish((identifier, body)),
        _ => Err(walk
            .finish(())
            .expect_err("a problem was found, or both parts would be there")),
    }
}

/// The parameters of a body, read by its family for a world with this seed. A message for whoever runs the server if the family is not loaded or finds a
/// problem (a body that was read has none, but the family may be another this time).
pub fn read_family_parameters(
    families: &[(NamespacedId, Arc<dyn FamilyData>)],
    body: &Body,
    world_seed: u64,
) -> Result<FamilyParameters, String> {
    let (_, family) = families
        .iter()
        .find(|(name, _)| *name == body.family)
        .ok_or_else(|| format!("no family {} is loaded", body.family))?;
    family
        .read(&body.parameters, world_seed)
        .map_err(|problems| {
            problems
                .iter()
                .map(|problem| format!("{}: {}", problem.path, problem.message))
                .collect::<Vec<_>>()
                .join("; ")
        })
}

fn body_identifier(walk: &mut Walk, pack: &str, name: &str, path: &Path) -> Option<NamespacedId> {
    match NamespacedId::new(pack, name) {
        Ok(identifier) => Some(identifier),
        Err(error) => {
            walk.fail(
                path,
                format!("the file name \"{name}\" is not a body name: {error}"),
            );
            None
        }
    }
}

fn read_definition(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    context: &BodyContext<'_>,
) -> Option<Body> {
    let object = walk.object(value, path, &KEYS)?;
    let level = walk
        .require(object, "level", path)
        .and_then(|level| read_level(walk, level, &path.key("level"), context.levels));
    let position = match object.get("position") {
        Some(position) => read_position(walk, position, &path.key("position")),
        None => Some([0.0; 3]),
    };
    let terrain = walk
        .require(object, "terrain", path)
        .and_then(|terrain| read_terrain(walk, terrain, &path.key("terrain"), context.families));
    let extent_bricks = walk
        .require(object, "extent_bricks", path)
        .and_then(|extent| {
            walk.integer(
                extent,
                &path.key("extent_bricks"),
                1,
                GREATEST_EXTENT_BRICKS,
            )
        })
        .map(|extent| extent as u32);
    let weight = read_weight(walk, object, path);
    let rail = match object.get("rail") {
        Some(rail) => read_rail(walk, rail, &path.key("rail")).map(Some),
        None => Some(None),
    };
    if object.contains_key("position")
        && rail
            .as_ref()
            .is_some_and(|rail| rail.as_ref().is_some_and(|rail| rail.orbit.is_some()))
    {
        walk.fail(
            &path.key("position"),
            "a body that goes round has its place from its orbit",
        );
    }
    let (family, parameters) = terrain?;
    Some(Body {
        level: level?,
        position: position?,
        family,
        parameters,
        extent_bricks: extent_bricks?,
        weight: weight?,
        rail: rail?,
    })
}

fn read_level(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    levels: &[NamespacedId],
) -> Option<NamespacedId> {
    let text = walk.string(value, path)?;
    let level: NamespacedId = match text.parse() {
        Ok(level) => level,
        Err(error) => {
            walk.fail(path, format!("\"{text}\" is not a level name: {error}"));
            return None;
        }
    };
    if !levels.contains(&level) {
        let names: Vec<&str> = levels.iter().map(NamespacedId::as_str).collect();
        walk.fail(
            path,
            format!(
                "the game has no level \"{text}\"; its levels are {}",
                quoted(&names)
            ),
        );
        return None;
    }
    Some(level)
}

fn read_position(walk: &mut Walk, value: &Value, path: &Path) -> Option<[f64; 3]> {
    let list = walk.array(value, path)?;
    if list.len() != 3 {
        walk.fail(
            path,
            format!("a position is 3 numbers (x, y, z), found {}", list.len()),
        );
        return None;
    }
    let mut position = [0.0; 3];
    let mut good = true;
    for (index, (slot, component)) in position.iter_mut().zip(list).enumerate() {
        let at = path.index(index);
        match walk.number(component, &at) {
            Some(number) if number.abs() <= POSITION_LIMIT => *slot = number,
            Some(_) => {
                walk.fail(
                    &at,
                    format!(
                        "{} is outside -{POSITION_LIMIT:.0} to {POSITION_LIMIT:.0}",
                        component
                    ),
                );
                good = false;
            }
            None => good = false,
        }
    }
    good.then_some(position)
}

/// `{ "family": ..., "parameters": ... }`: the family, which must be loaded, and its parameters, which it reads.
fn read_terrain(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    families: &[(NamespacedId, Arc<dyn FamilyData>)],
) -> Option<(NamespacedId, Value)> {
    let object = walk.object(value, path, &TERRAIN_KEYS)?;
    let family_path = path.key("family");
    let family = walk
        .require(object, "family", path)
        .and_then(|family| read_family(walk, family, &family_path, families));
    let parameters_path = path.key("parameters");
    let parameters = walk.require(object, "parameters", path);
    if let (Some((_, data)), Some(parameters)) = (&family, parameters)
        && let Err(problems) = data.read(parameters, 0)
    {
        for problem in problems {
            walk.fail(&parameters_path.appended(&problem.path), problem.message);
        }
        return None;
    }
    let (name, _) = family?;
    Some((name, parameters?.clone()))
}

fn read_family(
    walk: &mut Walk,
    value: &Value,
    path: &Path,
    families: &[(NamespacedId, Arc<dyn FamilyData>)],
) -> Option<(NamespacedId, Arc<dyn FamilyData>)> {
    let text = walk.string(value, path)?;
    let name: NamespacedId = match text.parse() {
        Ok(name) => name,
        Err(error) => {
            walk.fail(path, format!("\"{text}\" is not a family name: {error}"));
            return None;
        }
    };
    match families.iter().find(|(known, _)| *known == name) {
        Some((_, data)) => Some((name, Arc::clone(data))),
        None => {
            let names: Vec<&str> = families.iter().map(|(known, _)| known.as_str()).collect();
            walk.fail(
                path,
                format!(
                    "no family \"{text}\" is loaded; the families are {}",
                    quoted(&names)
                ),
            );
            None
        }
    }
}

fn read_rail(walk: &mut Walk, value: &Value, path: &Path) -> Option<BodyRail> {
    let object = walk.object(value, path, &RAIL_KEYS)?;
    if object.is_empty() {
        walk.fail(path, "a rail has a spin, an orbit or both");
        return None;
    }
    let spin = match object.get("spin") {
        Some(spin) => read_spin(walk, spin, &path.key("spin")).map(Some),
        None => Some(None),
    };
    let orbit = match object.get("orbit") {
        Some(orbit) => read_orbit(walk, orbit, &path.key("orbit")).map(Some),
        None => Some(None),
    };
    Some(BodyRail {
        spin: spin?,
        orbit: orbit?,
    })
}

fn read_spin(walk: &mut Walk, value: &Value, path: &Path) -> Option<SpinData> {
    let object = walk.object(value, path, &SPIN_KEYS)?;
    let axis = walk
        .require(object, "axis", path)
        .and_then(|axis| walk.direction(axis, &path.key("axis")));
    let period = walk
        .require(object, "period", path)
        .and_then(|period| walk.period(period, &path.key("period")));
    let angle = match object.get("angle") {
        Some(angle) => walk.number(angle, &path.key("angle")),
        None => Some(0.0),
    };
    Some(SpinData {
        axis: axis?,
        period: period?,
        angle_degrees: angle?,
    })
}

fn read_orbit(walk: &mut Walk, value: &Value, path: &Path) -> Option<OrbitData> {
    let object = walk.object(value, path, &ORBIT_KEYS)?;
    let around = walk
        .require(object, "around", path)
        .and_then(|around| read_around(walk, around, &path.key("around")));
    let normal = match object.get("normal") {
        Some(normal) => walk.direction(normal, &path.key("normal")),
        None => Some([0.0, 1.0, 0.0]),
    };
    let radius_path = path.key("radius");
    let radius = walk.require(object, "radius", path).and_then(|radius| {
        let number = walk.positive_number(radius, &radius_path)?;
        if number > POSITION_LIMIT {
            walk.fail(
                &radius_path,
                format!("{radius} is more than {POSITION_LIMIT:.0} voxels"),
            );
            return None;
        }
        Some(number)
    });
    let period = walk
        .require(object, "period", path)
        .and_then(|period| walk.period(period, &path.key("period")));
    let phase = match object.get("phase") {
        Some(phase) => walk.number(phase, &path.key("phase")),
        None => Some(0.0),
    };
    Some(OrbitData {
        around: around?,
        normal: normal?,
        radius: radius?,
        period: period?,
        phase_degrees: phase?,
    })
}

fn read_around(walk: &mut Walk, value: &Value, path: &Path) -> Option<Around> {
    match value {
        Value::String(text) => match text.parse::<NamespacedId>() {
            Ok(name) => Some(Around::Body(name)),
            Err(error) => {
                walk.fail(path, format!("\"{text}\" is not a body name: {error}"));
                None
            }
        },
        Value::Array(list) if list.len() == 3 => {
            let mut point = [0.0; 3];
            let mut good = true;
            for (index, (slot, component)) in point.iter_mut().zip(list).enumerate() {
                match walk.number(component, &path.index(index)) {
                    Some(number) => *slot = number,
                    None => good = false,
                }
            }
            good.then_some(Around::Point(point))
        }
        other => {
            let found = match other {
                Value::Array(list) => format!("a list of {}", list.len()),
                other => crate::walk::kind_of(other).to_owned(),
            };
            walk.fail(
                path,
                format!(
                    "a centre is a body (\"my_pack:star\") or a point of the level (3 numbers), found {found}"
                ),
            );
            None
        }
    }
}

const WEIGHT_MESSAGE: &str = "give how heavy it is as \"mass\" (kilograms) or as \"surface_gravity\" (metres a second a second), one of them";

fn read_weight(walk: &mut Walk, object: &Map<String, Value>, path: &Path) -> Option<Weight> {
    match (object.get("mass"), object.get("surface_gravity")) {
        (Some(mass), None) => {
            let at = path.key("mass");
            let number = walk.positive_number(mass, &at)?;
            if number > GREATEST_MASS {
                walk.fail(
                    &at,
                    format!("{mass} is more than a body can weigh (3.4e38 kilograms)"),
                );
                return None;
            }
            Some(Weight::Mass(number))
        }
        (None, Some(gravity)) => walk
            .positive_number(gravity, &path.key("surface_gravity"))
            .map(Weight::SurfaceGravity),
        _ => {
            walk.fail(path, WEIGHT_MESSAGE);
            None
        }
    }
}
