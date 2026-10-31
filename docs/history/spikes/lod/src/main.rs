//! Experiments on levels of detail for far terrain. See ../../../lod-spike.md for what they found.
//!
//! `cargo run --release -- NAME` runs one; `help` lists them. They print what they measured. Times are for one machine (see the report); the counts repeat.

mod columns;
mod gpu;
mod horizon;
mod occlusion;
mod picture;
mod pruning;
mod sweep;

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match name.as_str() {
        "pruning" => {
            pruning::how_many_cubes_and_how_long_with_and_without_pruning();
            pruning::no_cube_that_pruning_drops_has_anything_in_it();
        }
        "detail" => pruning::how_the_cell_size_changes_the_cube_count(),
        "columns" => {
            columns::batch_noise_has_the_same_bits_as_sampling_each_column();
            columns::the_mesh_made_from_the_columns_covers_the_faces_the_cells_and_the_greedy_mesher_give();
            columns::top_cell_fast_is_top_cell();
            columns::the_faster_way_makes_the_same_faces_and_how_much_faster_it_is();
            columns::what_each_way_of_making_a_node_costs();
        }
        "scaling" => columns::how_many_nodes_a_second_with_many_threads(),
        "view" => columns::the_whole_view_made_from_nothing_at_several_distances(),
        "occlusion" => {
            occlusion::sanity_how_much_of_the_ground_around_is_seen();
            occlusion::how_much_of_what_is_drawn_is_hidden_by_nearer_ground();
        }
        "horizon" => {
            horizon::the_dense_oracle_how_much_of_the_surface_is_really_hidden();
            horizon::how_many_cubes_does_the_horizon_of_the_eyes_place_cull();
            horizon::debug_what_the_horizon_looks_like_from_a_middling_eye();
        }
        "sweep" => sweep::a_horizon_swept_over_the_columns_as_they_are_made(),
        "gpu" => gpu::what_submitting_very_many_nodes_costs(),
        "picture" => picture::draw_the_far_terrain_made_from_columns(),
        _ => {
            eprintln!(
                "pruning    what pruning keeps, what the walk costs, and that nothing left out had anything in it\n\
                 detail     how the cell size in pixels changes the number of nodes\n\
                 columns   height-column nodes: same bits, same faces, what each way costs\n\
                 scaling    nodes a second with many threads\n\
                 view       a whole view made from nothing, at several distances\n\
                 occlusion  how much of what is drawn is hidden behind nearer ground (rays)\n\
                 horizon    horizon culling from the eye's place, against the rays\n\
                 gpu        what drawing very many nodes costs by how they are submitted\n\
                 sweep      a horizon swept over the columns as they are made, against the rays
\n                 picture    draws the far terrain made from columns to picture_*.png"
            );
        }
    }
}
