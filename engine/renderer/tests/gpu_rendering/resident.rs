//! Meshes the renderer holds apart from those it draws (`docs/specs/viewpoints-spec.md`, phase 1).
//!
//! A mesh is **drawn** (in the draw list, which the frame walks, culls and casts shadows from) or **hidden** (held on the graphics card, and nothing else). A picture costs what is
//! drawn and not what is held, and a mesh that goes from one to the other is moved, not made again. The tests count what a frame visits (`bricks_drawn + bricks_culled`: a counter,
//! not a clock) and compare pictures to the pixel.

use renderer::HeldMesh;

use super::*;

/// One of each of the four things the renderer holds meshes of.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Brick,
    Node,
    GridBrick,
    GridNode,
}

const KINDS: [Kind; 4] = [Kind::Brick, Kind::Node, Kind::GridBrick, Kind::GridNode];

impl Kind {
    /// The mesh of this kind that is drawn as the red cube of [`CUBE`]: a node's cells are two voxels wide, so its cube is two voxels wide at twice the place.
    fn held(self) -> HeldMesh {
        let node = cube_of_bricks([0, 0, 0], 1);
        match self {
            Kind::Brick => HeldMesh::Brick(ORIGIN_BRICK),
            Kind::Node => HeldMesh::Node(node),
            Kind::GridBrick => HeldMesh::GridBrick(a_ship(), ORIGIN_BRICK),
            Kind::GridNode => HeldMesh::GridNode(a_ship(), node),
        }
    }

    /// A camera that sees the cube of this kind in the middle of the picture.
    fn camera(self) -> Camera {
        match self {
            Kind::Brick | Kind::GridBrick => camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]),
            Kind::Node | Kind::GridNode => camera_at(ORIGIN_BRICK, [33.0, 33.0, 40.0]),
        }
    }

    /// Puts the mesh in the renderer with the call a game uses (so that it is drawn), and the pose a ship needs to be drawn at all.
    fn upload(self, renderer: &mut VoxelRenderer, voxels: &[([u8; 3], u32)]) {
        let mesh = mesh_of(voxels);
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        match self.held() {
            HeldMesh::Brick(coordinate) => renderer.upload_brick(coordinate, &mesh),
            HeldMesh::Node(cube) => renderer.upload_coarse_node(cube, &mesh),
            HeldMesh::GridBrick(grid, coordinate) => {
                renderer.upload_grid_brick(grid, coordinate, &mesh);
            }
            HeldMesh::GridNode(grid, cube) => renderer.upload_grid_coarse_node(grid, cube, &mesh),
        }
    }
}

/// How many bricks and nodes the frame visited (drawn, or culled by the camera).
fn visited(statistics: RenderStatistics) -> usize {
    statistics.bricks_drawn + statistics.bricks_culled
}

fn all_sky(image: &[u8]) -> bool {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .all(|texel| is_sky([texel[0], texel[1], texel[2], texel[3]]))
}

#[test]
fn a_hidden_mesh_is_not_drawn_nor_visited_and_when_shown_is_drawn_exactly_as_before() {
    for context in contexts() {
        let name = adapter_name(&context);
        for kind in KINDS {
            let mut renderer = new_renderer(&context);
            kind.upload(&mut renderer, &CUBE);
            let camera = kind.camera();
            let before = settled(&mut renderer, &camera);
            assert!(
                is_red(pixel(&before, SIZE / 2, SIZE / 2)),
                "{name} {kind:?}"
            );
            let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert_eq!(visited(statistics), 1, "{name} {kind:?}");
            assert_eq!(renderer.hidden_mesh_count(), 0, "{name} {kind:?}");

            assert!(renderer.hide_mesh(kind.held()), "{name} {kind:?}");
            assert!(renderer.is_mesh_hidden(kind.held()), "{name} {kind:?}");
            assert_eq!(renderer.hidden_mesh_count(), 1, "{name} {kind:?}");
            let hidden = settled(&mut renderer, &camera);
            assert!(all_sky(&hidden), "{name} {kind:?}: a hidden mesh is drawn");
            let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert_eq!(
                visited(statistics),
                0,
                "{name} {kind:?}: a hidden mesh is visited"
            );
            assert!(
                !renderer.hide_mesh(kind.held()),
                "{name} {kind:?}: hidden already"
            );

            assert!(renderer.show_mesh(kind.held()), "{name} {kind:?}");
            assert!(!renderer.is_mesh_hidden(kind.held()), "{name} {kind:?}");
            assert_eq!(renderer.hidden_mesh_count(), 0, "{name} {kind:?}");
            assert!(
                !renderer.show_mesh(kind.held()),
                "{name} {kind:?}: drawn already"
            );
            let after = settled(&mut renderer, &camera);
            assert!(after == before, "{name} {kind:?}: not the picture it was");
            let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert_eq!(visited(statistics), 1, "{name} {kind:?}");
        }
    }
}

#[test]
fn a_frame_visits_what_is_drawn_and_not_what_is_held() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        renderer.set_grid_pose(a_ship(), pose([0, 0, 0], [0.0; 3], Orientation::IDENTITY));
        place_voxels(&mut renderer, ORIGIN_BRICK, &CUBE);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let alone = settled(&mut renderer, &camera);
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(visited(statistics), 1, "{name}");

        // Two thousand meshes of every kind, hidden: 8,000 buffers that nothing draws.
        let far = mesh_of(&CUBE);
        for index in 0..2000i128 {
            let along = BrickCoordinate::new(100 + index, 7, 0);
            let node = cube_of_bricks([200 + 2 * index, 8, 0], 1);
            renderer.upload_hidden_mesh(HeldMesh::Brick(along), &far);
            renderer.upload_hidden_mesh(HeldMesh::Node(node), &far);
            renderer.upload_hidden_mesh(HeldMesh::GridBrick(a_ship(), along), &far);
            renderer.upload_hidden_mesh(HeldMesh::GridNode(a_ship(), node), &far);
        }
        assert_eq!(renderer.hidden_mesh_count(), 8000, "{name}");
        let with_hidden = settled(&mut renderer, &camera);
        assert!(
            with_hidden == alone,
            "{name}: what is only held changed the picture"
        );
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(
            visited(statistics),
            1,
            "{name}: the frame walked meshes that are not drawn"
        );

        // Shown, they are all walked (and culled: they are far away).
        for index in 0..2000i128 {
            let along = BrickCoordinate::new(100 + index, 7, 0);
            let node = cube_of_bricks([200 + 2 * index, 8, 0], 1);
            for held in [
                HeldMesh::Brick(along),
                HeldMesh::Node(node),
                HeldMesh::GridBrick(a_ship(), along),
                HeldMesh::GridNode(a_ship(), node),
            ] {
                assert!(renderer.show_mesh(held), "{name}");
            }
        }
        assert_eq!(renderer.hidden_mesh_count(), 0, "{name}");
        let (_, statistics) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(visited(statistics), 8001, "{name}");
        assert_eq!(
            statistics.bricks_drawn, 1,
            "{name}: only the cube is in view"
        );
    }
}

#[test]
fn a_mesh_uploaded_hidden_is_held_and_not_drawn_and_is_replaced_where_it_is() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        let camera = camera_at(ORIGIN_BRICK, [16.5, 16.5, 22.0]);
        let held = HeldMesh::Brick(ORIGIN_BRICK);

        renderer.upload_hidden_mesh(held, &mesh_of(&CUBE));
        assert!(renderer.is_mesh_hidden(held), "{name}");
        assert_eq!(
            (renderer.brick_count(), renderer.hidden_mesh_count()),
            (0, 1),
            "{name}"
        );
        assert!(all_sky(&settled(&mut renderer, &camera)), "{name}");

        // Another mesh for the same place replaces it, and it is still hidden: what the next picture draws is the new one.
        renderer.upload_hidden_mesh(held, &mesh_of(&[([16, 16, 16], GREEN)]));
        assert_eq!(
            (renderer.brick_count(), renderer.hidden_mesh_count()),
            (0, 1),
            "{name}"
        );
        assert!(all_sky(&settled(&mut renderer, &camera)), "{name}");
        assert!(renderer.show_mesh(held), "{name}");
        let green = settled(&mut renderer, &camera);
        assert!(is_green(pixel(&green, SIZE / 2, SIZE / 2)), "{name}");

        // A mesh that is drawn is replaced as it always was, and is still drawn (a call that says "hidden" does not hide it).
        renderer.upload_hidden_mesh(held, &mesh_of(&CUBE));
        assert!(!renderer.is_mesh_hidden(held), "{name}");
        assert!(
            is_red(pixel(&settled(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}"
        );
        renderer.upload_brick(ORIGIN_BRICK, &mesh_of(&[([16, 16, 16], GREEN)]));
        assert!(
            is_green(pixel(&settled(&mut renderer, &camera), SIZE / 2, SIZE / 2)),
            "{name}"
        );

        // A mesh with nothing in it is no mesh, drawn or hidden.
        renderer.hide_mesh(held);
        renderer.upload_hidden_mesh(held, &mesh_of(&[]));
        assert_eq!(renderer.hidden_mesh_count(), 0, "{name}");
        assert!(!renderer.show_mesh(held), "{name}");
    }
}

#[test]
fn removing_a_mesh_forgets_it_hidden_or_drawn() {
    for context in contexts() {
        let name = adapter_name(&context);
        for kind in KINDS {
            let mut renderer = new_renderer(&context);
            kind.upload(&mut renderer, &CUBE);
            let held = kind.held();
            assert!(renderer.hide_mesh(held), "{name} {kind:?}");
            match held {
                HeldMesh::Brick(coordinate) => renderer.remove_brick(coordinate),
                HeldMesh::Node(cube) => renderer.remove_coarse_node(cube),
                HeldMesh::GridBrick(grid, coordinate) => {
                    renderer.remove_grid_brick(grid, coordinate)
                }
                HeldMesh::GridNode(grid, cube) => renderer.remove_grid_coarse_node(grid, cube),
            }
            assert_eq!(renderer.hidden_mesh_count(), 0, "{name} {kind:?}");
            assert!(!renderer.show_mesh(held), "{name} {kind:?}");
            assert!(
                all_sky(&settled(&mut renderer, &kind.camera())),
                "{name} {kind:?}"
            );
        }

        // A ship that is forgotten takes its hidden meshes with it, and leaves those of another.
        let mut renderer = new_renderer(&context);
        let other = GridId::new(GridId::FIRST_ALLOCATED + 1);
        let mesh = mesh_of(&CUBE);
        renderer.upload_hidden_mesh(HeldMesh::GridBrick(a_ship(), ORIGIN_BRICK), &mesh);
        renderer.upload_hidden_mesh(
            HeldMesh::GridNode(a_ship(), cube_of_bricks([0, 0, 0], 1)),
            &mesh,
        );
        renderer.upload_hidden_mesh(HeldMesh::GridBrick(other, ORIGIN_BRICK), &mesh);
        renderer.remove_grid(a_ship());
        assert_eq!(renderer.hidden_mesh_count(), 1, "{name}");
        assert!(
            renderer.is_mesh_hidden(HeldMesh::GridBrick(other, ORIGIN_BRICK)),
            "{name}"
        );
    }
}

#[test]
fn what_is_only_held_does_not_make_the_shadow_maps_drawn_again_but_what_is_drawn_does() {
    for context in contexts() {
        let name = adapter_name(&context);
        let mut renderer = new_renderer(&context);
        upload_shadow_scene(&mut renderer, ShadowScene::Block, 0);
        let (camera, _, _) = shadow_scene_setup(ShadowScene::Block, 0);
        renderer.set_atmosphere(shadow_atmosphere(0, SUN_OVER_THE_EAST, true));
        let (_, first) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert!(first.shadow_cascades_drawn >= 1, "{name}");
        let (_, kept) = renderer.render_to_rgba(&camera, SIZE, SIZE);
        assert_eq!(kept.shadow_cascades_drawn, 0, "{name}: the maps are kept");

        // A mesh that comes to be held beside the scene, and one that is replaced there, are no change to what the maps hold.
        let beside = HeldMesh::Brick(BrickCoordinate::new(1, 0, 0));
        renderer.upload_hidden_mesh(beside, &mesh_of(&CUBE));
        renderer.upload_hidden_mesh(beside, &mesh_of(&[([8, 8, 8], GREEN)]));
        for _ in 0..4 {
            let (_, still) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert_eq!(
                still.shadow_cascades_drawn, 0,
                "{name}: held meshes redrew a map"
            );
        }
        // One that is hidden is not a change either, but the one that was drawn and is not now is: the block's shadow is gone.
        assert!(renderer.show_mesh(beside), "{name}");
        let redrawn: usize = (0..4)
            .map(|_| {
                renderer
                    .render_to_rgba(&camera, SIZE, SIZE)
                    .1
                    .shadow_cascades_drawn
            })
            .sum();
        assert!(redrawn >= 1, "{name}: showing a mesh did not redraw a map");
        assert!(renderer.hide_mesh(beside), "{name}");
        let redrawn: usize = (0..4)
            .map(|_| {
                renderer
                    .render_to_rgba(&camera, SIZE, SIZE)
                    .1
                    .shadow_cascades_drawn
            })
            .sum();
        assert!(redrawn >= 1, "{name}: hiding a mesh did not redraw a map");
        for _ in 0..4 {
            let (_, still) = renderer.render_to_rgba(&camera, SIZE, SIZE);
            assert_eq!(
                still.shadow_cascades_drawn, 0,
                "{name}: the maps were not kept again"
            );
        }
    }
}
