use voxel_space::{BrickCoordinate, GridId, GridPose, Orientation, WorldPosition};

use super::{Focus, FocusFrame, Orbit, next_focus};
use crate::view::camera_at;

fn frame_at(centre: WorldPosition, orientation: Orientation, radius: f64) -> FocusFrame {
    FocusFrame {
        centre,
        orientation,
        radius,
    }
}

fn level_frame(radius: f64) -> FocusFrame {
    frame_at(
        WorldPosition::new(BrickCoordinate::new(10, 20, -30), [4.0, 5.0, 6.0]),
        Orientation::IDENTITY,
        radius,
    )
}

fn turned() -> Orientation {
    Orientation::from_yaw_pitch(0.7, -0.4)
}

/// The vector from the camera to the focus's middle, in voxels.
fn to_the_centre(camera: &renderer::Camera, frame: &FocusFrame) -> [f64; 3] {
    frame.centre.relative_to(camera.position)
}

fn unit(vector: [f64; 3]) -> [f64; 3] {
    let length = vector.iter().map(|c| c * c).sum::<f64>().sqrt();
    vector.map(|c| c / length)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

#[test]
fn the_camera_is_the_distance_from_the_middle_of_the_focus_and_looks_at_it_whatever_the_angles_and_the_turn_of_the_focus()
 {
    let template = camera_at([0.0; 3], 0.0, 0.0);
    for frame in [
        level_frame(10.0),
        frame_at(level_frame(10.0).centre, turned(), 10.0),
    ] {
        for azimuth in [0.0, 0.9, -2.5, 3.0] {
            for elevation in [0.0, 0.5, -0.5, 1.2, -1.2] {
                for distance in [30.0, 2_500.0, 3.0e5] {
                    let orbit = Orbit {
                        focus: Focus::Body,
                        azimuth,
                        elevation,
                        distance,
                    };
                    let camera = orbit.camera(&frame, &template);
                    let towards = to_the_centre(&camera, &frame);
                    let length = dot(towards, towards).sqrt();
                    assert!(
                        (length - distance).abs() <= distance * 1e-9,
                        "{length} from the middle, asked for {distance}"
                    );
                    // It looks at the middle (the turn the graphics card has is single precision: a millionth).
                    let forward = camera.forward().to_array().map(f64::from);
                    assert!(
                        dot(unit(towards), unit(forward)) > 1.0 - 1e-6,
                        "azimuth {azimuth}, elevation {elevation}, distance {distance}: not looking at the middle"
                    );
                }
            }
        }
    }
}

#[test]
fn a_camera_at_no_angle_is_straight_back_from_the_focus_along_its_own_z_and_one_over_it_is_straight_up_from_it()
 {
    let template = camera_at([0.0; 3], 0.0, 0.0);
    let frame = level_frame(10.0);
    let level = Orbit {
        focus: Focus::Body,
        azimuth: 0.0,
        elevation: 0.0,
        distance: 100.0,
    };
    let behind = level
        .camera(&frame, &template)
        .position
        .relative_to(frame.centre);
    assert!(
        (behind[0]).abs() < 1e-9 && (behind[1]).abs() < 1e-9 && (behind[2] - 100.0).abs() < 1e-9
    );
    let over = Orbit {
        elevation: Orbit::MAXIMUM_ELEVATION,
        ..level
    };
    let above = over
        .camera(&frame, &template)
        .position
        .relative_to(frame.centre);
    assert!(above[1] > 99.9 * libm::sin(Orbit::MAXIMUM_ELEVATION) && above[1] < 100.0);
}

#[test]
fn a_camera_locked_on_a_grid_keeps_its_place_in_the_grids_own_axes_while_the_grid_moves_and_turns()
{
    // (A planet's cut does not change while it moves on its rail: the orbit is made of the focus's own axes.)
    let template = camera_at([0.0; 3], 0.0, 0.0);
    let orbit = Orbit {
        focus: Focus::Grid(GridId::new(GridId::FIRST_ALLOCATED + 3)),
        azimuth: 1.1,
        elevation: 0.3,
        distance: 400.0,
    };
    let places: Vec<[f64; 3]> = [
        (WorldPosition::from_voxels([0.0; 3]), Orientation::IDENTITY),
        (WorldPosition::from_voxels([5.0e6, -2.0e6, 7.0e5]), turned()),
        (
            WorldPosition::new(
                BrickCoordinate::new(1 << 60, 5, -(1 << 50)),
                [1.0, 2.0, 3.0],
            ),
            Orientation::from_yaw_pitch(-2.0, 0.9),
        ),
    ]
    .into_iter()
    .map(|(centre, orientation)| {
        let frame = frame_at(centre, orientation, 50.0);
        let camera = orbit.camera(&frame, &template);
        let pose = GridPose::new(centre, orientation);
        let local = pose
            .world_to_local(camera.position)
            .expect("the camera is in range");
        local.relative_to(pose.world_to_local(centre).expect("so is the middle"))
    })
    .collect();
    for place in &places[1..] {
        for axis in 0..3 {
            assert!(
                (place[axis] - places[0][axis]).abs() < 1e-6,
                "{place:?} against {:?}",
                places[0]
            );
        }
    }
}

#[test]
fn the_camera_is_turned_with_the_focus_so_that_up_is_the_foci_up() {
    let template = camera_at([0.0; 3], 0.0, 0.0);
    let frame = frame_at(level_frame(10.0).centre, turned(), 10.0);
    let orbit = Orbit {
        focus: Focus::Body,
        azimuth: 0.4,
        elevation: 0.2,
        distance: 80.0,
    };
    let camera = orbit.camera(&frame, &template);
    let focus_up =
        GridPose::new(frame.centre, frame.orientation).direction_to_world([0.0, 1.0, 0.0]);
    let camera_up = camera.up().to_array().map(f64::from);
    // The camera is not rolled: its up leans toward the focus's up (the angle between them is the elevation, not more).
    assert!(dot(unit(focus_up), unit(camera_up)) > 0.97);
    assert_eq!(
        camera.frame_rotation,
        glam::Quat::from_array(turned().components_single()),
        "the camera's frame is the focus's turn"
    );
}

#[test]
fn what_the_camera_shows_of_the_template_is_kept() {
    let mut template = camera_at([1.0, 2.0, 3.0], 0.3, 0.1);
    template.vertical_field_of_view = 1.0;
    template.near_plane = 0.5;
    let camera = Orbit {
        focus: Focus::Body,
        azimuth: 0.0,
        elevation: 0.0,
        distance: 10.0,
    }
    .camera(&level_frame(1.0), &template);
    assert_eq!(camera.vertical_field_of_view, 1.0);
    assert_eq!(camera.near_plane, 0.5);
}

#[test]
fn an_orthographic_map_is_as_high_as_a_perspective_camera_at_that_distance_sees_at_the_focus_and_its_eye_is_that_far_back_with_a_box_that_reaches_past_the_middle()
 {
    let mut template = camera_at([1.0, 2.0, 3.0], 0.3, 0.1);
    template.vertical_field_of_view = 1.0;
    let orbit = Orbit {
        focus: Focus::Body,
        azimuth: 0.4,
        elevation: 0.9,
        distance: 5_000.0,
    };
    let frame = level_frame(1.0);
    let height = 2.0 * libm::tan(0.5) * 5_000.0;
    assert!((orbit.orthographic_height(&template) - height).abs() < 1e-9);
    let camera = orbit.orthographic_camera(&frame, &template);
    let perspective = orbit.camera(&frame, &template);
    // The same way of looking, and the same field of view otherwise kept.
    assert_eq!(
        (camera.yaw, camera.pitch),
        (perspective.yaw, perspective.pitch)
    );
    assert_eq!(camera.near_plane, template.near_plane);
    let renderer::Projection::Orthographic {
        height: drawn_height,
        depth,
    } = camera.projection
    else {
        panic!("{:?}", camera.projection);
    };
    assert!((f64::from(drawn_height) - height).abs() < 1.0);
    // The eye is as far back as the picture is high (here the same as the perspective camera's distance: 5,000 against 5,463), and the box reaches past the focus.
    let back = |camera: &renderer::Camera| {
        let relative = camera.position.relative_to(frame.centre);
        libm::sqrt(relative.iter().map(|part| part * part).sum::<f64>())
    };
    assert!((back(&camera) - height).abs() < 1e-6, "{}", back(&camera));
    assert!(f64::from(depth) > back(&camera) + height);
    // Zoomed all the way in, the eye keeps a distance of its own, so that what is near the focus is in the box.
    let near = Orbit {
        distance: 4.0,
        ..orbit
    }
    .orthographic_camera(&frame, &template);
    assert!((back(&near) - 128.0).abs() < 1e-6, "{}", back(&near));
}

#[test]
fn turning_wraps_the_azimuth_and_stops_the_elevation_short_of_straight_up_and_down() {
    let orbit = Orbit {
        focus: Focus::Body,
        azimuth: 3.0,
        elevation: 0.0,
        distance: 100.0,
    };
    let turned = orbit.rotated_by(0.5, 5.0);
    assert!((turned.azimuth - (3.5 - std::f64::consts::TAU)).abs() < 1e-12);
    assert_eq!(turned.elevation, Orbit::MAXIMUM_ELEVATION);
    assert_eq!(
        orbit.rotated_by(0.0, -5.0).elevation,
        -Orbit::MAXIMUM_ELEVATION
    );
    assert_eq!(orbit.rotated_by(-4.0, 0.0).azimuth, -1.0);
    let behind = Orbit {
        azimuth: -3.0,
        ..orbit
    };
    assert_eq!(
        behind.rotated_by(-1.0, 0.0).azimuth,
        -4.0 + std::f64::consts::TAU
    );
    // Whole turns are dropped, and a turn that is not a number changes nothing.
    assert!((orbit.rotated_by(10.0 * std::f64::consts::TAU, 0.0).azimuth - 3.0).abs() < 1e-9);
    assert_eq!(orbit.rotated_by(f64::NAN, f64::NAN), orbit);
}

#[test]
fn zooming_multiplies_the_distance_and_keeps_it_between_the_nearest_and_the_farthest() {
    let frame = level_frame(100.0);
    let orbit = Orbit::new(Focus::Body, &frame);
    assert_eq!(orbit.distance, 100.0 * Orbit::STARTING_DISTANCE_IN_RADII);
    let nearer = orbit.zoomed_by(0.5, &frame);
    assert_eq!(nearer.distance, orbit.distance * 0.5);
    let nearest = orbit.zoomed_by(1.0e-9, &frame);
    assert_eq!(nearest.distance, Orbit::nearest_distance(&frame));
    assert!(
        Orbit::nearest_distance(&frame) > frame.radius,
        "never inside the focus"
    );
    let farthest = orbit.zoomed_by(1.0e30, &frame);
    assert_eq!(farthest.distance, Orbit::MAXIMUM_DISTANCE);
    // A zoom that is not a number, or not over nothing, changes nothing.
    for bad in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        assert_eq!(orbit.zoomed_by(bad, &frame), orbit, "{bad}");
    }
}

#[test]
fn a_focus_too_small_to_have_a_size_is_still_a_few_voxels_away() {
    let frame = level_frame(0.0);
    assert!(Orbit::nearest_distance(&frame) >= 4.0);
    assert!(Orbit::new(Focus::Body, &frame).distance >= Orbit::nearest_distance(&frame));
}

#[test]
fn the_next_focus_goes_round_the_ones_there_are_in_order_and_one_that_is_gone_starts_the_round() {
    let ship = |number: u64| Focus::Grid(GridId::new(GridId::FIRST_ALLOCATED + number));
    let there = [Focus::Body, ship(1), ship(4), ship(9)];
    assert_eq!(next_focus(Focus::Body, &there, 1), ship(1));
    assert_eq!(next_focus(ship(1), &there, 1), ship(4));
    assert_eq!(next_focus(ship(9), &there, 1), Focus::Body, "round the end");
    assert_eq!(
        next_focus(Focus::Body, &there, -1),
        ship(9),
        "and backwards"
    );
    assert_eq!(next_focus(ship(4), &there, -1), ship(1));
    // A focus that is not there any more (the ship went out of range) starts the round, as the body.
    assert_eq!(next_focus(ship(2), &there, 1), Focus::Body);
    assert_eq!(next_focus(ship(2), &there, -1), Focus::Body);
    // Nothing but the body: the body, either way. Nothing at all: the body too.
    assert_eq!(next_focus(Focus::Body, &[Focus::Body], 1), Focus::Body);
    assert_eq!(next_focus(ship(2), &[], 1), Focus::Body);
    // A step of nothing, or many, is by that many.
    assert_eq!(next_focus(Focus::Body, &there, 0), Focus::Body);
    assert_eq!(next_focus(Focus::Body, &there, 5), ship(1));
}
