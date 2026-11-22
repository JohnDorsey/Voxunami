//! What a [`crate::LodView`] knows of each cube it draws or is to draw, in one place.
//!
//! A cube is *wanted* (the viewer's position says it is to be on the screen) and *shown* (it is on the screen): two sets, which are the inputs and the result of
//! [`coarse_cells::plan_swaps`] and what the view tells its callers, so they stay sets. Everything else that is known of a cube is known of a node (a cube of
//! detail level 1 or more; a brick's picture is the mesher's, see [`crate::brick_states`]), and is one record: its picture is missing, waiting to be shown, or on
//! the screen, and a job may be making a picture of it. These were four collections that had to agree (the sets, a map of records with a picture on the
//! screen and a picture waiting that could not both be, and a third set of the nodes the renderer had a mesh of); now a change is a method here, which changes
//! all of what it touches, and [`CubeStates::check_consistency`] says what the shape of the records cannot.

use std::collections::{BTreeMap, HashMap, HashSet};

use region_octree::BrickCube;
use voxel_meshing::PackedBrickMesh;

/// What a picture that is held costs the budget at least, whatever its mesh takes: a node with nothing to see is held without a mesh (so that it is not made again to
/// find that out), and the records are many.
pub(crate) const MINIMUM_HELD_COST: usize = 64;

/// What the budget is charged for a picture: the bytes of its mesh, and at least [`MINIMUM_HELD_COST`].
fn cost_of(mesh: &PackedBrickMesh) -> usize {
    mesh.byte_size().max(MINIMUM_HELD_COST)
}

/// What is known of the picture of a node.
enum Picture {
    /// There is no up-to-date picture and none is on the screen.
    Missing,
    /// A picture made from data of this version, up to date, not on the screen.
    Waiting { version: u64, mesh: PackedBrickMesh },
    /// A picture made from data of this version is on the screen. `uploaded` is whether the renderer holds a mesh of it (a node with nothing to see has none).
    /// `cost` is what the budget is charged for it if it is taken down and held.
    OnScreen {
        version: u64,
        uploaded: bool,
        cost: usize,
    },
    /// A picture made from data of this version, up to date, **kept** though the node is not shown: the renderer holds a mesh of it, hidden, if `uploaded`, and showing
    /// it again is a move. `order` says how long ago it was taken down (a number that is never used twice and counts up: no clock), which is the order it is let go in.
    Held {
        version: u64,
        uploaded: bool,
        cost: usize,
        order: u64,
    },
}

/// What the renderer is to do about a node that was taken down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// It had no mesh in the renderer.
    Nothing,
    /// The renderer had a mesh of it, which it is to take down for good.
    Removed,
    /// The renderer had a mesh of it, which it is to **hide** and keep (the picture is held).
    Hidden,
}

/// What is held: the pictures of nodes that are not shown and are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldPictures {
    /// How many nodes have a picture held.
    pub nodes: usize,
    /// Of those, how many the renderer holds a mesh of, hidden (the rest have nothing to see).
    pub with_mesh: usize,
    /// What the budget is charged for them: their meshes' bytes, a node with nothing to see being charged a little.
    pub bytes: usize,
}

/// The record of one node.
pub(crate) struct Node {
    picture: Picture,
    /// The version of the data a job is making a picture from.
    requested: Option<u64>,
}

impl Node {
    fn new() -> Self {
        Self {
            picture: Picture::Missing,
            requested: None,
        }
    }

    /// A job is making a picture of it (as far as the view knows: the job may be of data that has changed since).
    pub(crate) fn is_requested(&self) -> bool {
        self.requested.is_some()
    }

    /// A picture is waiting to be shown.
    pub(crate) fn has_waiting_picture(&self) -> bool {
        matches!(self.picture, Picture::Waiting { .. })
    }

    fn is_on_screen(&self) -> bool {
        matches!(self.picture, Picture::OnScreen { .. })
    }
}

/// What the renderer is to do about a node whose picture was made again where it was shown.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Replacement {
    /// Replace its mesh with this one.
    Upload(PackedBrickMesh),
    /// It has nothing to see now, and had a mesh: take it down.
    Remove,
    /// It has nothing to see, and had nothing.
    Unchanged,
}

/// What came of a finished job.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// The node is gone, or the picture is of data that has changed since: it is thrown away.
    Ignored,
    /// The picture is kept, waiting to be shown, which is for the planner to decide.
    Waiting,
    /// The node was shown, and the picture replaces the one on the screen.
    Replaced(Replacement),
}

#[derive(Default)]
pub(crate) struct CubeStates {
    wanted: HashSet<BrickCube>,
    shown: HashSet<BrickCube>,
    /// The cubes that stand between what is shown and what is wanted ([`coarse_cells::intermediate_cubes`]): made and shown on the way, though they are not wanted.
    steps: HashSet<BrickCube>,
    /// The records of the nodes that are wanted, shown or steps on the way, and of those whose pictures are held, and of no others.
    nodes: HashMap<BrickCube, Node>,
    /// How many bytes of pictures that are not shown are kept, at most (not counting the ones that are wanted: those are on their way to the screen). 0, the default, keeps none.
    budget: usize,
    /// What the pictures that are held are charged, in all.
    held_bytes: usize,
    /// The nodes whose pictures are held, by how long ago they were taken down: the first is the oldest.
    held_order: BTreeMap<u64, BrickCube>,
    /// The number the next picture that is held is given.
    next_order: u64,
}

impl CubeStates {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Sets how many bytes of pictures of nodes that are not shown are kept ([`CubeStates::evict_over_budget`] lets go of the ones over it).
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    /// What is held now.
    pub(crate) fn held_pictures(&self) -> HeldPictures {
        let with_mesh = self
            .held_order
            .values()
            .filter(|cube| {
                matches!(
                    self.nodes.get(cube).map(|node| &node.picture),
                    Some(Picture::Held { uploaded: true, .. })
                )
            })
            .count();
        HeldPictures {
            nodes: self.held_order.len(),
            with_mesh,
            bytes: self.held_bytes,
        }
    }

    /// Whether the node's picture is held (it is not shown, and is ready to be).
    pub(crate) fn is_held(&self, cube: BrickCube) -> bool {
        self.nodes
            .get(&cube)
            .is_some_and(|node| matches!(node.picture, Picture::Held { .. }))
    }

    /// What is to be drawn, bricks and nodes.
    pub(crate) fn wanted(&self) -> &HashSet<BrickCube> {
        &self.wanted
    }

    /// What is drawn: bricks that were shown (some with nothing to see) and nodes. They do not overlap.
    pub(crate) fn shown(&self) -> &HashSet<BrickCube> {
        &self.shown
    }

    pub(crate) fn node(&self, cube: BrickCube) -> Option<&Node> {
        self.nodes.get(&cube)
    }

    /// What is to be drawn is now these cubes: the nodes among them have records, and the nodes that are neither wanted nor shown have none.
    #[cfg(test)]
    pub(crate) fn want(&mut self, cubes: &[BrickCube]) {
        self.want_with_steps(cubes, &[]);
    }

    /// The same, and `steps` are the cubes that are to be made and shown on the way to what is wanted (they have records too, until the next time this is said).
    pub(crate) fn want_with_steps(&mut self, cubes: &[BrickCube], steps: &[BrickCube]) {
        self.wanted = cubes.iter().copied().collect();
        self.steps = steps.iter().copied().collect();
        for cube in cubes.iter().chain(steps).filter(|cube| cube.level() > 0) {
            self.nodes.entry(*cube).or_insert_with(Node::new);
        }
        let (wanted, shown, steps) = (&self.wanted, &self.shown, &self.steps);
        self.nodes.retain(|cube, node| {
            wanted.contains(cube)
                || shown.contains(cube)
                || steps.contains(cube)
                || matches!(node.picture, Picture::Held { .. })
        });
    }

    /// Whether the cube is to be made: it is wanted, or a step on the way to what is.
    fn is_to_be_made(&self, cube: &BrickCube) -> bool {
        self.wanted.contains(cube) || self.steps.contains(cube)
    }

    /// Whether the picture of a node that is wanted or shown has to be made (again), from data of this version: it is not being made already, and the one it has
    /// is not of this version.
    pub(crate) fn needs_making(&self, cube: BrickCube, version: u64) -> bool {
        let Some(node) = self.nodes.get(&cube) else {
            return false;
        };
        if node.requested == Some(version) {
            return false;
        }
        match node.picture {
            Picture::OnScreen { version: drawn, .. } => drawn != version,
            Picture::Waiting { version: ready, .. } => {
                self.is_to_be_made(&cube) && ready != version
            }
            // A picture that is held of data that has changed is made again when the node is wanted, and not before.
            Picture::Held { version: held, .. } => self.is_to_be_made(&cube) && held != version,
            Picture::Missing => self.is_to_be_made(&cube),
        }
    }

    /// A job is to make a picture of a node from data of this version. The node has a record: it needed making.
    pub(crate) fn start_making(&mut self, cube: BrickCube, version: u64) {
        self.nodes
            .get_mut(&cube)
            .expect("it needs making, so it has a record")
            .requested = Some(version);
    }

    /// A job has finished a picture of a node from data of version `made`, and `current` is the version of the data now.
    pub(crate) fn finish_making(
        &mut self,
        cube: BrickCube,
        made: u64,
        current: u64,
        mesh: PackedBrickMesh,
    ) -> Arrival {
        let Some(node) = self.nodes.get_mut(&cube) else {
            return Arrival::Ignored;
        };
        if node.requested == Some(made) {
            node.requested = None;
        }
        if made != current {
            // Made from data that has been replaced since: the new data is queued already.
            return Arrival::Ignored;
        }
        if matches!(node.picture, Picture::Held { .. }) {
            // (A picture is held only when no job is making another, and one that is held of old data is dropped before a job is started: this does not happen.)
            return Arrival::Ignored;
        }
        if let Picture::OnScreen { uploaded, .. } = node.picture {
            // On the screen already: the picture is replaced where it is.
            let something_to_see = !mesh.is_empty();
            node.picture = Picture::OnScreen {
                version: made,
                uploaded: something_to_see,
                cost: cost_of(&mesh),
            };
            return Arrival::Replaced(if something_to_see {
                Replacement::Upload(mesh)
            } else if uploaded {
                Replacement::Remove
            } else {
                Replacement::Unchanged
            });
        }
        node.picture = Picture::Waiting {
            version: made,
            mesh,
        };
        Arrival::Waiting
    }

    /// Whether the node has a picture waiting that is of data of this version.
    pub(crate) fn is_ready(&self, cube: BrickCube, version: u64) -> bool {
        self.nodes.get(&cube).is_some_and(|node| {
            matches!(
                node.picture,
                Picture::Waiting { version: ready, .. } | Picture::Held { version: ready, .. }
                    if ready == version
            )
        })
    }

    /// A brick is shown (the mesher has the picture).
    pub(crate) fn show_brick(&mut self, cube: BrickCube) {
        self.shown.insert(cube);
    }

    /// A brick is taken down.
    pub(crate) fn hide_brick(&mut self, cube: BrickCube) {
        self.shown.remove(&cube);
    }

    /// A node is shown with the picture that was waiting, which is returned; the renderer is to be given it unless it is empty.
    pub(crate) fn show_node(&mut self, cube: BrickCube) -> PackedBrickMesh {
        let node = self
            .nodes
            .get_mut(&cube)
            .expect("a node that is ready has a record");
        let Picture::Waiting { version, mesh } =
            std::mem::replace(&mut node.picture, Picture::Missing)
        else {
            panic!("the planner only adds what is ready");
        };
        node.picture = Picture::OnScreen {
            version,
            uploaded: !mesh.is_empty(),
            cost: cost_of(&mesh),
        };
        self.shown.insert(cube);
        mesh
    }

    /// A node whose picture is held is shown with it: returns whether the renderer has a mesh of it, which it is to show (it is hidden).
    pub(crate) fn reveal_node(&mut self, cube: BrickCube) -> bool {
        let node = self
            .nodes
            .get_mut(&cube)
            .expect("a node that is ready has a record");
        let Picture::Held {
            version,
            uploaded,
            cost,
            order,
        } = node.picture
        else {
            panic!("only a picture that is held is revealed");
        };
        node.picture = Picture::OnScreen {
            version,
            uploaded,
            cost,
        };
        self.held_order.remove(&order);
        self.held_bytes -= cost;
        self.shown.insert(cube);
        uploaded
    }

    /// A node is taken down, and its picture is kept if it can be (there is a budget, the picture is of the data of version `current` and no job is making another):
    /// held, hidden in the renderer. If not, as [`CubeStates::hide_node`].
    pub(crate) fn hold_node(&mut self, cube: BrickCube, current: u64) -> Taken {
        let holds = self.budget > 0 && self.nodes.get(&cube).is_some_and(|node| {
            node.requested.is_none()
                && matches!(node.picture, Picture::OnScreen { version, .. } if version == current)
        });
        if !holds {
            return if self.hide_node(cube) {
                Taken::Removed
            } else {
                Taken::Nothing
            };
        }
        self.shown.remove(&cube);
        let node = self.nodes.get_mut(&cube).expect("it was checked");
        let Picture::OnScreen {
            version,
            uploaded,
            cost,
        } = node.picture
        else {
            unreachable!("it was checked");
        };
        let order = self.next_order;
        self.next_order += 1;
        node.picture = Picture::Held {
            version,
            uploaded,
            cost,
            order,
        };
        self.held_order.insert(order, cube);
        self.held_bytes += cost;
        if uploaded {
            Taken::Hidden
        } else {
            Taken::Nothing
        }
    }

    /// The picture of a node that is wanted and is held of data that has changed is let go of, to be made again: returns what the renderer is to do (take down its hidden
    /// mesh). Nothing if the node's picture is not held, or is of the data of version `current`.
    pub(crate) fn drop_stale_held(&mut self, cube: BrickCube, current: u64) -> Taken {
        let Some(node) = self.nodes.get_mut(&cube) else {
            return Taken::Nothing;
        };
        let Picture::Held {
            version,
            uploaded,
            cost,
            order,
        } = node.picture
        else {
            return Taken::Nothing;
        };
        if version == current {
            return Taken::Nothing;
        }
        node.picture = Picture::Missing;
        self.held_order.remove(&order);
        self.held_bytes -= cost;
        if uploaded {
            Taken::Removed
        } else {
            Taken::Nothing
        }
    }

    /// Lets go of held pictures, the oldest first, until what is held is within the budget, and returns the nodes whose hidden meshes the renderer is to take down. A
    /// picture of a node that is wanted or a step on the way is not let go of (it is about to be shown): what is held may pass the budget by those.
    pub(crate) fn evict_over_budget(&mut self) -> Vec<BrickCube> {
        let mut excess = self.held_bytes.saturating_sub(self.budget);
        if excess == 0 {
            return Vec::new();
        }
        let mut victims = Vec::new();
        for (&order, &cube) in &self.held_order {
            if excess == 0 {
                break;
            }
            if self.is_to_be_made(&cube) {
                continue;
            }
            let Some(Picture::Held { cost, uploaded, .. }) =
                self.nodes.get(&cube).map(|node| &node.picture)
            else {
                unreachable!("a node in the order of the held has a held picture");
            };
            excess = excess.saturating_sub(*cost);
            victims.push((order, cube, *cost, *uploaded));
        }
        let mut taken_down = Vec::new();
        for (order, cube, cost, uploaded) in victims {
            self.held_order.remove(&order);
            self.held_bytes -= cost;
            self.nodes.remove(&cube);
            if uploaded {
                taken_down.push(cube);
            }
        }
        taken_down
    }

    /// A node is taken down: returns whether the renderer had a mesh of it, which it is to be told to take down. The record is dropped if the node is not wanted.
    pub(crate) fn hide_node(&mut self, cube: BrickCube) -> bool {
        self.shown.remove(&cube);
        let Some(node) = self.nodes.get_mut(&cube) else {
            return false;
        };
        let uploaded = matches!(node.picture, Picture::OnScreen { uploaded: true, .. });
        node.picture = Picture::Missing;
        if !self.is_to_be_made(&cube) {
            self.nodes.remove(&cube);
        }
        uploaded
    }

    /// Whether the node has a record.
    pub(crate) fn has_node(&self, cube: BrickCube) -> bool {
        self.nodes.contains_key(&cube)
    }

    /// The nodes the renderer holds a mesh of, drawn or hidden, in no order.
    pub(crate) fn uploaded_nodes(&self) -> Vec<BrickCube> {
        self.nodes
            .iter()
            .filter(|(_, node)| {
                matches!(
                    node.picture,
                    Picture::OnScreen { uploaded: true, .. } | Picture::Held { uploaded: true, .. }
                )
            })
            .map(|(cube, _)| *cube)
            .collect()
    }

    /// How many nodes a job is making a picture of, as far as the view knows.
    pub(crate) fn requested_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|node| node.is_requested())
            .count()
    }

    /// Forgets every cube.
    pub(crate) fn clear(&mut self) {
        self.wanted.clear();
        self.shown.clear();
        self.steps.clear();
        self.nodes.clear();
        self.held_order.clear();
        self.held_bytes = 0;
    }

    /// Checks what the shape of the records cannot make impossible, and says what is wrong if it is not so (the first thing found).
    pub(crate) fn check_consistency(&self) -> Result<(), String> {
        for (cube, node) in &self.nodes {
            if cube.level() == 0 {
                return Err(format!("{cube:?} is a brick and has the record of a node"));
            }
            let shown = self.shown.contains(cube);
            let held = matches!(node.picture, Picture::Held { .. });
            if held && shown {
                return Err(format!("{cube:?} is held and shown"));
            }
            if held && node.is_requested() {
                return Err(format!("{cube:?} is held and a job is making it"));
            }
            if !shown && !held && !self.is_to_be_made(cube) {
                return Err(format!(
                    "{cube:?} has a record and is neither wanted nor shown (nor a step on the way, nor held)"
                ));
            }
            if node.is_on_screen() != shown {
                return Err(format!(
                    "{cube:?} has a picture on the screen: {}, and is shown: {shown}",
                    node.is_on_screen()
                ));
            }
        }
        for cube in self.shown.iter().filter(|cube| cube.level() > 0) {
            if !self.nodes.contains_key(cube) {
                return Err(format!("{cube:?} is shown and has no record"));
            }
        }
        let mut held_nodes = 0;
        let mut held_cost = 0;
        for (cube, node) in &self.nodes {
            if let Picture::Held { cost, order, .. } = node.picture {
                held_nodes += 1;
                held_cost += cost;
                if self.held_order.get(&order) != Some(cube) {
                    return Err(format!(
                        "{cube:?} is held and is not in the order of the held"
                    ));
                }
            }
        }
        if held_nodes != self.held_order.len() || held_cost != self.held_bytes {
            return Err(format!(
                "{held_nodes} nodes are held for {held_cost} bytes, and the totals of the held pictures say {} for {}",
                self.held_order.len(),
                self.held_bytes
            ));
        }
        for cube in self.wanted.iter().filter(|cube| cube.level() > 0) {
            if !self.nodes.contains_key(cube) {
                return Err(format!("{cube:?} is wanted and has no record"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
