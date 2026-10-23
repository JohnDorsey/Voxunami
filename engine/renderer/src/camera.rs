use glam::{Mat4, Quat, Vec3, Vec4};
use voxel_space::WorldPosition;

/// A first-person camera. Its position is a [`WorldPosition`], so it stays precise at any distance
/// from the origin; everything drawn is placed relative to it.
///
/// Conventions: right-handed, x right, y up, and the camera looks along -z when `yaw` and `pitch`
/// are zero. Positive yaw turns left (counter-clockwise seen from above), positive pitch looks up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub position: WorldPosition,
    pub yaw: f32,
    pub pitch: f32,
    /// The turn of the frame `yaw` and `pitch` are measured in: the identity for a player standing in the world, and the turn of a
    /// player's room for one aboard a ship that has turned, so that the camera looks where the angles point in that room. The angles are
    /// still what the player's mouse turns.
    pub frame_rotation: Quat,
    /// Vertical field of view in radians.
    pub vertical_field_of_view: f32,
    /// The distance of the nearest thing drawn. There is no far plane: depth is reversed and goes on for ever (see [`Camera::view_projection`]).
    pub near_plane: f32,
}

impl Camera {
    /// Pitch is limited just short of straight up or down so the view never flips.
    pub const MAXIMUM_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

    /// Looking along -z with a 70 degree field of view.
    pub fn new(position: WorldPosition) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            frame_rotation: Quat::IDENTITY,
            vertical_field_of_view: 70.0 * std::f32::consts::PI / 180.0,
            near_plane: 0.1,
        }
    }

    /// The turn of the view: the angles' turn, then the frame's. A camera in no frame is the angles' turn alone, to the bit.
    pub fn rotation(&self) -> Quat {
        let angles = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch);
        if self.frame_rotation == Quat::IDENTITY {
            angles
        } else {
            self.frame_rotation * angles
        }
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::NEG_Z
    }

    pub fn right(&self) -> Vec3 {
        self.rotation() * Vec3::X
    }

    pub fn up(&self) -> Vec3 {
        self.rotation() * Vec3::Y
    }

    /// Turns the camera, clamping pitch to [`Camera::MAXIMUM_PITCH`].
    pub fn look_by(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(-Self::MAXIMUM_PITCH, Self::MAXIMUM_PITCH);
    }

    /// Moves by `movement` given in the camera's own axes (x right, y up, z backwards).
    pub fn move_relative(&mut self, movement: Vec3) {
        let world_movement = self.rotation() * movement;
        self.position = self.position.translated(world_movement.to_array());
    }

    /// Moves by `movement` given in world axes.
    pub fn move_in_world(&mut self, movement: Vec3) {
        self.position = self.position.translated(movement.to_array());
    }

    /// Maps positions relative to the camera (not world positions) to clip space; the
    /// camera-relative translation is applied by the renderer in exact integer arithmetic.
    ///
    /// Depth is **reversed** and the far plane is at infinity: a point at distance `d` ahead has depth `near / d`, so the nearest thing drawn is 1 and what is
    /// farther is between 0 and 1, however far. A floating point depth buffer has most of its precision near 0, which is where the far things are, so
    /// surfaces keep their order at planet distances: measured, walls 2 metres apart are in order at 100 kilometres and at 10,000 kilometres, and at 100,000
    /// kilometres the limit is between 4 and 8 metres (the test draws them, in both orders of drawing). The depth buffer is cleared to 0 and the test is "greater".
    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        // Right-handed, y up, clip depth 0..1: wgpu's conventions, with depth reversed.
        let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            self.vertical_field_of_view,
            aspect_ratio,
            self.near_plane,
        );
        projection * Mat4::from_quat(self.rotation().inverse())
    }
}

/// The five planes bounding what a camera can see (left, right, bottom, top and near: there is no far one), for cheap visibility tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// Each plane is `(normal, distance)`; points with `normal . p + distance >= 0` are inside.
    planes: [Vec4; 5],
}

impl Frustum {
    /// Extracts the planes of a clip volume with depth in `0..=1` (wgpu's convention) that is reversed (1 at the near plane) and has no far plane, as
    /// [`Camera::view_projection`] makes.
    pub fn from_view_projection(view_projection: Mat4) -> Self {
        let (row0, row1, row2, row3) = (
            view_projection.row(0),
            view_projection.row(1),
            view_projection.row(2),
            view_projection.row(3),
        );
        let normalize = |plane: Vec4| plane / plane.truncate().length();
        Self {
            planes: [
                normalize(row3 + row0),
                normalize(row3 - row0),
                normalize(row3 + row1),
                normalize(row3 - row1),
                // Depth at most 1 is not nearer than the near plane. (Depth at least 0 is every distance: there is no plane for it.)
                normalize(row3 - row2),
            ],
        }
    }

    /// False only if the sphere is entirely outside one plane. It may be true for spheres that are
    /// just outside a corner, which is fine for culling.
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.truncate().dot(center) + plane.w >= -radius)
    }
}

#[cfg(test)]
mod tests {
    use voxel_space::BrickCoordinate;

    use super::*;

    fn camera_at_origin() -> Camera {
        Camera::new(WorldPosition::new(BrickCoordinate::new(0, 0, 0), [0.0; 3]))
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    /// The depth a point straight ahead at this distance gets, and whether it is inside the view at all.
    fn depth_ahead(camera: &Camera, distance: f32) -> (f32, bool) {
        let clip = camera.view_projection(1.0) * Vec4::new(0.0, 0.0, -distance, 1.0);
        let depth = clip.z / clip.w;
        (depth, (0.0..=1.0).contains(&depth) && clip.w > 0.0)
    }

    #[test]
    fn depth_is_reversed_and_has_no_far_plane_so_the_nearest_thing_is_1_and_everything_beyond_is_between_0_and_1()
     {
        let camera = camera_at_origin();
        let near = camera.near_plane;
        // The near plane is depth 1, and depth is the near distance over the distance, all the way out: nothing is clipped for being far.
        assert!((depth_ahead(&camera, near).0 - 1.0).abs() < 1e-5);
        for distance in [1.0f32, 100.0, 4096.0, 1.0e5, 1.0e7, 1.0e9, 1.0e12] {
            let (depth, inside) = depth_ahead(&camera, distance);
            assert!(inside, "{distance} is clipped");
            assert!(
                (depth - near / distance).abs() <= 1e-5 * (near / distance),
                "{distance}: {depth}"
            );
        }
        // Nearer than the near plane is outside (depth over 1), and what is behind the camera is behind it.
        assert!(!depth_ahead(&camera, near * 0.5).1);
        assert!(!depth_ahead(&camera, -10.0).1);
    }

    #[test]
    fn the_frustum_has_no_far_side_and_still_culls_what_is_behind_beside_and_too_near() {
        let camera = camera_at_origin();
        let frustum = Frustum::from_view_projection(camera.view_projection(1.0));
        // Far ahead, however far, is in view (the sphere is a planet).
        for distance in [100.0f32, 1.0e6, 1.0e12] {
            assert!(frustum.intersects_sphere(Vec3::new(0.0, 0.0, -distance), 1.0));
        }
        // Behind, and far to the side, are not.
        assert!(!frustum.intersects_sphere(Vec3::new(0.0, 0.0, 50.0), 1.0));
        assert!(!frustum.intersects_sphere(Vec3::new(1.0e6, 0.0, -1.0e3), 1.0));
        assert!(!frustum.intersects_sphere(Vec3::new(0.0, -1.0e6, -1.0e3), 1.0));
        // A sphere that reaches into the view from the side is kept.
        assert!(frustum.intersects_sphere(Vec3::new(1.0e3, 0.0, -1.0e3), 1.0e3));
    }

    #[test]
    fn a_default_camera_looks_down_negative_z() {
        let camera = camera_at_origin();
        assert!(close(camera.forward(), Vec3::NEG_Z));
        assert!(close(camera.right(), Vec3::X));
        assert!(close(camera.up(), Vec3::Y));
    }

    #[test]
    fn a_frame_turns_where_the_angles_point_and_a_camera_in_no_frame_is_the_angles_alone_to_the_bit()
     {
        let mut camera = camera_at_origin();
        camera.look_by(0.7, -0.3);
        let alone = camera.rotation();
        assert_eq!(
            alone,
            Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.3)
        );
        // A frame tipped a quarter about x (up there is the room's back, +z): the room's ahead, -z, is the world's up, +y.
        camera = camera_at_origin();
        camera.frame_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        assert!(close(camera.forward(), Vec3::Y));
        assert!(close(camera.up(), Vec3::Z));
        assert!(close(camera.right(), Vec3::X));
        // The angles are the room's: a quarter yaw in it turns toward the room's -x, which a tip about x leaves where it is.
        camera.look_by(std::f32::consts::FRAC_PI_2, 0.0);
        assert!(close(camera.forward(), Vec3::NEG_X));
        // Moving by the camera's own axes goes along the turned view, and what is seen follows it too.
        camera = camera_at_origin();
        camera.frame_rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        camera.move_relative(Vec3::NEG_Z * 40.0);
        assert_eq!(camera.position.brick(), BrickCoordinate::new(0, 1, 0));
        assert!((camera.position.offset()[1] - 8.0).abs() < 1e-3);
        let clip = camera.view_projection(1.0) * Vec4::new(0.0, 10.0, 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5,
            "straight up is ahead: {ndc:?}"
        );
    }

    #[test]
    fn positive_yaw_turns_left_and_positive_pitch_looks_up() {
        let mut camera = camera_at_origin();
        camera.look_by(std::f32::consts::FRAC_PI_2, 0.0);
        assert!(close(camera.forward(), Vec3::NEG_X));
        let mut camera = camera_at_origin();
        camera.look_by(0.0, std::f32::consts::FRAC_PI_4);
        assert!(camera.forward().y > 0.7 && camera.forward().z < -0.7);
    }

    #[test]
    fn pitch_is_clamped_short_of_vertical() {
        let mut camera = camera_at_origin();
        camera.look_by(0.0, 10.0);
        assert_eq!(camera.pitch, Camera::MAXIMUM_PITCH);
        camera.look_by(0.0, -100.0);
        assert_eq!(camera.pitch, -Camera::MAXIMUM_PITCH);
    }

    #[test]
    fn relative_movement_follows_the_facing_direction_and_crosses_bricks() {
        let mut camera = camera_at_origin();
        camera.look_by(std::f32::consts::FRAC_PI_2, 0.0); // facing -x
        camera.move_relative(Vec3::NEG_Z * 40.0); // forward 40 voxels
        assert_eq!(camera.position.brick(), BrickCoordinate::new(-2, 0, 0));
        assert!((camera.position.offset()[0] - 24.0).abs() < 1e-3);
        assert!(camera.position.offset()[2].abs() < 1e-3);
    }

    #[test]
    fn a_point_straight_ahead_projects_to_the_middle_of_the_screen() {
        let camera = camera_at_origin();
        let clip = camera.view_projection(1.0) * Vec4::new(0.0, 0.0, -10.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x.abs() < 1e-6 && ndc.y.abs() < 1e-6);
        assert!((0.0..=1.0).contains(&ndc.z));
        // To the right of the camera is to the right on screen; above is up.
        let clip = camera.view_projection(1.0) * Vec4::new(2.0, 3.0, -10.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        assert!(ndc.x > 0.0 && ndc.y > 0.0);
    }

    #[test]
    fn the_frustum_keeps_what_is_ahead_and_drops_what_is_not() {
        let frustum = Frustum::from_view_projection(camera_at_origin().view_projection(1.0));
        let visible = |x: f32, y: f32, z: f32, radius: f32| {
            frustum.intersects_sphere(Vec3::new(x, y, z), radius)
        };
        assert!(visible(0.0, 0.0, -10.0, 1.0), "straight ahead");
        assert!(!visible(0.0, 0.0, 10.0, 1.0), "behind the camera");
        assert!(!visible(100.0, 0.0, -10.0, 1.0), "far to the right");
        assert!(!visible(0.0, -100.0, -10.0, 1.0), "far below");
        assert!(visible(9.0, 0.0, -10.0, 3.0), "overlapping the right edge");
        assert!(visible(0.0, 0.0, -5000.0, 1.0), "there is no far plane");
        assert!(!visible(0.0, 0.0, 0.5, 0.1), "behind the near plane");
        assert!(
            visible(0.0, 0.0, 0.0, 1.0),
            "a large sphere around the camera"
        );
    }

    #[test]
    fn the_frustum_follows_the_cameras_rotation() {
        let mut camera = camera_at_origin();
        camera.look_by(std::f32::consts::FRAC_PI_2, 0.0); // facing -x
        let frustum = Frustum::from_view_projection(camera.view_projection(1.0));
        assert!(frustum.intersects_sphere(Vec3::new(-10.0, 0.0, 0.0), 1.0));
        assert!(!frustum.intersects_sphere(Vec3::new(0.0, 0.0, -10.0), 1.0));
    }
}
