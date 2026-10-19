//! The client half of the 2048 plugin: the score the server last said, the tool the player chose, what the
//! mouse buttons ask and what the title says.

use std::sync::{Arc, Mutex, MutexGuard};

use game_shell::KeyCode;
use game_shell::client_plugin::{
    ActionSpec, ClientContext, ClientPlugin, ClientRegistrar, MouseButton,
};
use namespaced_id::NamespacedId;

use crate::game::push_request;
use crate::messages::Progress;
use crate::numbered_block::NumberedBlock;
use crate::tool::Tool;

/// What the 2048 client remembers: how the game has gone, as the server last said, and the tool the
/// player has chosen.
#[derive(Debug, Default)]
struct State {
    progress: Progress,
    tool: Tool,
}

/// The client half of `voxunami_2048:rules`.
#[derive(Clone, Debug, Default)]
pub struct Client2048 {
    state: Arc<Mutex<State>>,
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Client2048 {
    /// How the game has gone, as the server last said.
    pub fn progress(&self) -> Progress {
        locked(&self.state).progress
    }

    /// The tool the player has chosen.
    pub fn tool(&self) -> Tool {
        locked(&self.state).tool
    }
}

/// What a block is, in words: "spring ice 8".
pub fn describe(block: NumberedBlock) -> String {
    format!(
        "{} {}",
        block.material.name().replace('_', " "),
        block.number()
    )
}

/// The title: the tool, the score, and the block under the crosshair.
fn hud(tool: Tool, progress: &Progress, looking_at: Option<NumberedBlock>) -> String {
    let largest = match progress.largest_merge {
        0 => "-".to_owned(),
        number => number.to_string(),
    };
    let looking_at = looking_at.map_or_else(|| "nothing".to_owned(), describe);
    format!(
        "tool {} | score {} | pushes {} | largest merge {largest} | looking at {looking_at}",
        tool.name(),
        progress.score,
        progress.pushes
    )
}

impl ClientPlugin for Client2048 {
    fn id(&self) -> NamespacedId {
        "voxunami_2048:rules"
            .parse()
            .expect("constant identifier is valid")
    }

    fn register(&self, registrar: &mut ClientRegistrar<'_>) {
        let state = Arc::clone(&self.state);
        registrar.on_message(move |progress: Progress| locked(&state).progress = progress);

        for (key, tool, id, summary) in [
            (
                KeyCode::Digit1,
                Tool::Double,
                "voxunami_2048:choose_double",
                "chooses the double tool: equal numbers merge into their double",
            ),
            (
                KeyCode::Digit2,
                Tool::Add,
                "voxunami_2048:choose_add",
                "chooses the add tool: any two numbers add up",
            ),
            (
                KeyCode::Digit3,
                Tool::Subtract,
                "voxunami_2048:choose_subtract",
                "chooses the subtract tool: the smaller is taken from the larger",
            ),
        ] {
            let state = Arc::clone(&self.state);
            registrar.key(key, ActionSpec::new(id, summary), move |_| {
                locked(&state).tool = tool;
            });
        }

        for (button, pull, id, summary) in [
            (
                MouseButton::Primary,
                false,
                "voxunami_2048:push",
                "pushes the block you look at away from you",
            ),
            (
                MouseButton::Secondary,
                true,
                "voxunami_2048:pull",
                "pulls the block you look at toward you",
            ),
        ] {
            let state = Arc::clone(&self.state);
            registrar.mouse(button, ActionSpec::new(id, summary), move |context| {
                let tool = locked(&state).tool;
                ask_to_push(context, tool, pull);
            });
        }

        let state = Arc::clone(&self.state);
        registrar.hud_line(move |context| {
            let (tool, progress) = {
                let state = locked(&state);
                (state.tool, state.progress)
            };
            let looking_at = context
                .target_block()
                .and_then(|(block, state)| NumberedBlock::from_voxel(block, state));
            Some(hud(tool, &progress, looking_at))
        });
    }
}

/// Asks the server to push (or pull) the block under the crosshair with the tool.
fn ask_to_push(context: &mut ClientContext<'_>, tool: Tool, pull: bool) {
    if let Some(request) = push_request(context.target(), tool, pull) {
        context.send_message(&request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::Material;

    #[test]
    fn blocks_are_described_by_material_and_number() {
        assert_eq!(describe(NumberedBlock::new(Material::Stone, 2)), "stone 2");
        assert_eq!(
            describe(NumberedBlock::new(Material::SpringIce, 32)),
            "spring ice 32"
        );
        assert_eq!(
            describe(NumberedBlock::new(Material::Ice, 3_000_000_001)),
            "ice 3000000001"
        );
    }

    #[test]
    fn the_title_says_the_tool_the_score_and_what_is_looked_at() {
        let progress = Progress {
            score: 12,
            pushes: 3,
            largest_merge: 0,
        };
        assert_eq!(
            hud(Tool::Add, &progress, None),
            "tool add | score 12 | pushes 3 | largest merge - | looking at nothing"
        );
        let progress = Progress {
            largest_merge: 64,
            ..progress
        };
        assert_eq!(
            hud(
                Tool::Double,
                &progress,
                Some(NumberedBlock::new(Material::Ice, 8))
            ),
            "tool double | score 12 | pushes 3 | largest merge 64 | looking at ice 8"
        );
    }
}
