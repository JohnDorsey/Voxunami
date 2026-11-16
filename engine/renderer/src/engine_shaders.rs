//! The text the engine's own voxel shaders are made of, which a development build can read from the source folder instead of the program (the second step of
//! `docs/hot-reload-plan.md`), so that a change to a shader shows without making the program again.
//!
//! The parts are the files `voxel.wgsl` and its kinds (with or without the pictures, the relief of them, the walk of the sun's ray through the voxels that block it): see
//! `voxel_source`, which puts them together. [`EngineShaderTexts::built_in`] is what the program has; [`EngineShaderTexts::read`] is a folder's.

use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

macro_rules! engine_shader_parts {
    ($($field:ident => $file:literal),+ $(,)?) => {
        /// The text of every file the engine's voxel shaders are made of: what the program has ([`EngineShaderTexts::built_in`]) or what a folder has
        /// ([`EngineShaderTexts::read`]). The files are named in [`EngineShaderTexts::FILES`].
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct EngineShaderTexts {
            $(pub $field: String,)+
        }

        impl EngineShaderTexts {
            /// The names of the files, in the folder of the shaders (`engine/renderer/src`).
            pub const FILES: &'static [&'static str] = &[$($file),+];

            fn from_program() -> Self {
                Self { $($field: include_str!($file).to_owned(),)+ }
            }

            /// Reads every file from this folder. A file that is not there, or cannot be read as text, is an error that names it: the shaders are made from all of them or from
            /// none.
            pub fn read(folder: &Path) -> Result<Self, String> {
                Ok(Self { $($field: read_part(folder, $file)?,)+ })
            }
        }
    };
}

engine_shader_parts! {
    voxel => "voxel.wgsl",
    pictures_on => "pictures_on.wgsl",
    pictures_off => "pictures_off.wgsl",
    relief_flat => "relief_flat.wgsl",
    relief_ray => "relief_ray.wgsl",
    relief_prisms => "relief_prisms.wgsl",
    relief_parallax_occlusion => "relief_parallax_occlusion.wgsl",
    contact_on => "contact_on.wgsl",
    contact_off => "contact_off.wgsl",
    shapes_on => "shapes_on.wgsl",
    shapes_off => "shapes_off.wgsl",
    windows_on => "windows_on.wgsl",
    windows_off => "windows_off.wgsl",
    entities_on => "entities_on.wgsl",
    entities_off => "entities_off.wgsl",
}

impl EngineShaderTexts {
    /// What the program was made with.
    pub fn built_in() -> &'static Self {
        static BUILT_IN: OnceLock<EngineShaderTexts> = OnceLock::new();
        BUILT_IN.get_or_init(Self::from_program)
    }
}

fn read_part(folder: &Path, file: &str) -> Result<String, String> {
    let path = folder.join(file);
    std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The graphics card (or the shader language, which checks first) refused the engine's shaders made from a text that was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineShaderError(pub(crate) String);

impl fmt::Display for EngineShaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the engine's shaders were refused: {}", self.0)
    }
}

impl std::error::Error for EngineShaderError {}

#[cfg(test)]
mod tests;
