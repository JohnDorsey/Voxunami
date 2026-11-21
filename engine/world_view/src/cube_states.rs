//! What a [`crate::LodView`] knows of each cube it draws or is to draw, in one place.
//!
//! A cube is *wanted* (the viewer's position says it is to be on the screen) and *shown* (it is on the screen): two sets, which are the inputs and the result of
//! [`coarse_cells::plan_swaps`] and what the view tells its callers, so they stay sets. Everything else that is known of a cube is known of a node (a cube of
//! detail level 1 or more; a brick's picture is the mesher's, see [`crate::brick_states`]), and is one record: its picture is missing, waiting to be shown, or on
//! the screen, and a job may be making a picture of it. These were four collections that had to agree (the sets, a map of records with a picture on the
//! screen and a picture waiting that could not both be, and a third set of the nodes the renderer had a mesh of); now a change is a method here, which changes
//! all of what it touches, and [`CubeStates::check_consistency`] says what the shape of the records cannot.

use std::collections::{HashMap, HashSet};

use region_octree::BrickCube;
use voxel_meshing::PackedBrickMesh;

/// What is known of the picture of a node.
enum Picture {
    /// There is no up-to-date picture and none is on the screen.
    Missing,
    /// A picture made from data of this version, up to date, not on the screen.
    Waiting { version: u64, mesh: PackedBrickMesh },
    /// A picture made from data of this version is on the screen. `uploaded` is whether the renderer holds a mesh of it (a node with nothing to see has none).
    OnScreen { version: u64, uploaded: bool },
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
    /// The records of the nodes that are wanted, shown or steps on the way, and of no others.
    nodes: HashMap<BrickCube, Node>,
}

impl CubeStates {
    pub(crate) fn new() -> Self {
        Self::default()
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
        self.nodes.retain(|cube, _| {
            wanted.contains(cube) || shown.contains(cube) || steps.contains(cube)
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
        if let Picture::OnScreen { uploaded, .. } = node.picture {
            // On the screen already: the picture is replaced where it is.
            let something_to_see = !mesh.is_empty();
            node.picture = Picture::OnScreen {
                version: made,
                uploaded: something_to_see,
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
            matches!(node.picture, Picture::Waiting { version: ready, .. } if ready == version)
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
        };
        self.shown.insert(cube);
        mesh
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

    /// The nodes the renderer holds a mesh of, in no order.
    pub(crate) fn uploaded_nodes(&self) -> Vec<BrickCube> {
        self.nodes
            .iter()
            .filter(|(_, node)| matches!(node.picture, Picture::OnScreen { uploaded: true, .. }))
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
    }

    /// Checks what the shape of the records cannot make impossible, and says what is wrong if it is not so (the first thing found).
    pub(crate) fn check_consistency(&self) -> Result<(), String> {
        for (cube, node) in &self.nodes {
            if cube.level() == 0 {
                return Err(format!("{cube:?} is a brick and has the record of a node"));
            }
            let shown = self.shown.contains(cube);
            if !shown && !self.is_to_be_made(cube) {
                return Err(format!(
                    "{cube:?} has a record and is neither wanted nor shown (nor a step on the way)"
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
