//! The data side of a family of terrain generators: what a pack author may write for its parameters, and how those become the family's bytes. The family
//! itself (the code that makes a generator from the bytes) is `terrain::GeneratorFamily`; a plugin that has a family says how its parameters are written here.

use std::fmt::Debug;

use serde_json::Value;

/// One thing wrong with the parameters a pack gave a family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyProblem {
    /// Where in the parameters, as a JSON pointer (`/radius`); empty for the parameters as a whole.
    pub path: String,
    pub message: String,
}

/// Parameters read: the family's bytes, and what else a body needs to know about the ground they make.
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyParameters {
    /// The bytes a recipe holds (what `terrain::GeneratorFamily::build` is given).
    pub bytes: Vec<u8>,
    /// How far from the centre of the body its surface is, in voxels: where a body that is told how hard it pulls at its surface pulls so hard.
    pub surface_radius: f64,
}

/// How the parameters of a family are written in a body file.
pub trait FamilyData: Debug + Send + Sync {
    /// The JSON Schema of the parameters object, generated from what the family takes (an editor completes and checks the file with it).
    fn schema(&self) -> Value;

    /// Reads the parameters, reporting every problem rather than the first. `world_seed` is the seed of the world the body is made for, which a parameter
    /// may say it takes (`"seed": "world"`); a body is read once with a seed that means nothing, to find what is wrong with it, and again for the world it
    /// is made in.
    fn read(
        &self,
        parameters: &Value,
        world_seed: u64,
    ) -> Result<FamilyParameters, Vec<FamilyProblem>>;
}
