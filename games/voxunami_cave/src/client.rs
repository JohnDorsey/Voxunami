//! The client halves of Cave's plugins: what the light looks like (the server says how it is cut into
//! cells), what the number keys and mouse buttons do to blocks, and the key that flies.

use std::sync::{Arc, Mutex, MutexGuard};

use base_blocks::{SHAPE_NAMES, shape_name};
use game_shell::KeyCode;
use game_shell::client_plugin::{
    ActionSpec, ClientContext, ClientPlugin, ClientRegistrar, MouseButton,
};
use namespaced_id::NamespacedId;
use voxel_grid::ChannelIndex;
use voxel_space::VoxelCoordinate;
use world_view::BulkLight;

use crate::content::CaveContent;
use crate::game::PLACEABLE_BLOCKS;
use crate::messages::{LightSchemeChanged, SetBlock};
use crate::multiscale_light::{LEVEL_FORMAT, LightScheme, MAXIMUM_SCALES, full_brightness};
use crate::scaled_light::{MultiScaleView, ScaledLight};

fn locked<T>(state: &Mutex<T>) -> MutexGuard<'_, T> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn identifier(text: &str) -> NamespacedId {
    text.parse().expect("constant identifier is valid")
}

/// The request to put `block` (named by identifier), in `state`, at `at`.
pub fn set_block_request(at: VoxelCoordinate, block: &str, state: u32) -> SetBlock {
    SetBlock {
        at,
        block: block.parse().expect("block names are valid identifiers"),
        state,
    }
}

/// The client half of `voxunami_cave:light`: how the world's multiscale light is cut into cells, if it has
/// that light, and so how the view blends it.
#[derive(Clone, Debug)]
pub struct ClientLight {
    scale_channels: Vec<ChannelIndex>,
    scheme: Arc<Mutex<Option<LightScheme>>>,
}

impl ClientLight {
    pub fn new(content: &CaveContent) -> Self {
        Self {
            scale_channels: (0..MAXIMUM_SCALES)
                .map(|scale| content.scale_channel(scale))
                .collect(),
            scheme: Arc::default(),
        }
    }

    /// How the light is cut into cells, if the world has the multiscale light (the server says).
    pub fn scheme(&self) -> Option<LightScheme> {
        *locked(&self.scheme)
    }
}

impl ClientPlugin for ClientLight {
    fn id(&self) -> NamespacedId {
        identifier("voxunami_cave:light")
    }

    fn register(&self, registrar: &mut ClientRegistrar<'_>) {
        let scheme = Arc::clone(&self.scheme);
        registrar.on_message(move |LightSchemeChanged(latest)| *locked(&scheme) = latest);

        // The view compares its light hook by identity, so the hook is made once for a scheme and kept
        // until the scheme changes, which meshes the view again.
        let scheme = Arc::clone(&self.scheme);
        let scale_channels = self.scale_channels.clone();
        let mut kept: Option<(LightScheme, BulkLight)> = None;
        registrar.view(move |mut channels| {
            let Some(scheme) = *locked(&scheme) else {
                return channels;
            };
            let hook = match &kept {
                Some((kept_scheme, hook)) if *kept_scheme == scheme => hook.clone(),
                _ => {
                    let scales = (0..scheme.scales())
                        .map(|scale| {
                            let width = scheme.widths()[scale];
                            ScaledLight {
                                channel: scale_channels[scale],
                                cell_width: width,
                                full_brightness: full_brightness(width),
                                format: LEVEL_FORMAT,
                            }
                        })
                        .collect();
                    let hook = BulkLight(Arc::new(MultiScaleView {
                        light: channels.light,
                        scales,
                    }));
                    kept = Some((scheme, hook.clone()));
                    hook
                }
            };
            channels.bulk_light = Some(hook);
            channels
        });
    }
}

/// What the player has chosen to place.
#[derive(Debug, Default)]
struct Selection {
    block: usize,
    /// The state, which is the shape, that placed blocks get (see [`SHAPE_NAMES`]).
    shape: u32,
}

/// The client half of `voxunami_cave:block_editing`: which block and shape are selected, and what the
/// mouse buttons do with them.
#[derive(Clone, Debug, Default)]
pub struct ClientBlockEditing {
    selection: Arc<Mutex<Selection>>,
}

impl ClientBlockEditing {
    /// The index in [`PLACEABLE_BLOCKS`] of the selected block.
    pub fn selected_block(&self) -> usize {
        locked(&self.selection).block
    }

    /// The state (shape) that placed blocks get.
    pub fn selected_shape(&self) -> u32 {
        locked(&self.selection).shape
    }
}

/// The number key that selects each placeable block, in order.
const SELECT_KEYS: [KeyCode; 6] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
];

/// Asks the server to break the block under the crosshair.
fn break_target(context: &mut ClientContext<'_>) {
    if let Some(hit) = context.target() {
        context.send_message(&set_block_request(hit.voxel, "base_blocks:air", 0));
    }
}

/// Asks the server to place `block` in `state` against the face under the crosshair.
fn place_at_target(context: &mut ClientContext<'_>, block: &str, state: u32) {
    if let Some(hit) = context.target() {
        context.send_message(&set_block_request(hit.previous, block, state));
    }
}

impl ClientPlugin for ClientBlockEditing {
    fn id(&self) -> NamespacedId {
        identifier("voxunami_cave:block_editing")
    }

    fn register(&self, registrar: &mut ClientRegistrar<'_>) {
        for (index, key) in SELECT_KEYS.into_iter().enumerate() {
            let name = PLACEABLE_BLOCKS[index]
                .split(':')
                .next_back()
                .unwrap_or_default();
            let selection = Arc::clone(&self.selection);
            registrar.key(
                key,
                ActionSpec::new(
                    &format!("voxunami_cave:select_{name}"),
                    "selects the block to place",
                ),
                move |_| locked(&selection).block = index,
            );
        }

        let selection = Arc::clone(&self.selection);
        registrar.key(
            KeyCode::KeyR,
            ActionSpec::new(
                "voxunami_cave:cycle_shape",
                "cycles whole block, slabs and stairs",
            ),
            move |_| {
                let mut selection = locked(&selection);
                selection.shape = (selection.shape + 1) % SHAPE_NAMES.len() as u32;
            },
        );

        registrar.mouse(
            MouseButton::Primary,
            ActionSpec::new("voxunami_cave:break_block", "breaks the block you look at"),
            break_target,
        );
        let selection = Arc::clone(&self.selection);
        registrar.mouse(
            MouseButton::Secondary,
            ActionSpec::new(
                "voxunami_cave:place_block",
                "places the selected block against the face you look at",
            ),
            move |context| {
                let (block, shape) = {
                    let selection = locked(&selection);
                    (PLACEABLE_BLOCKS[selection.block], selection.shape)
                };
                place_at_target(context, block, shape);
            },
        );

        let selection = Arc::clone(&self.selection);
        registrar.hud_line(move |_| {
            let selection = locked(&selection);
            let block = PLACEABLE_BLOCKS[selection.block];
            Some(format!(
                "placing {} ({})",
                block.split(':').next_back().unwrap_or_default(),
                shape_name(selection.shape)
            ))
        });
    }
}

/// The client half of `voxunami_cave:movement`: the key that flies.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClientMovement;

impl ClientPlugin for ClientMovement {
    fn id(&self) -> NamespacedId {
        identifier("voxunami_cave:movement")
    }

    fn register(&self, registrar: &mut ClientRegistrar<'_>) {
        registrar.key(
            KeyCode::KeyF,
            ActionSpec::new("voxunami_cave:toggle_flying", "flies or walks"),
            |context| context.send_command("/fly"),
        );
    }
}
