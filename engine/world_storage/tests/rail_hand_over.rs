//! The hand-over of a body from the sphere of influence of a planet to that of what the planet goes round (E16 phase B of `docs/orbits-and-thrust-plan.md`): when a body on an orbit round
//! the planet leaves the sphere (on a grid of 2^-31 of a second, in closed form), and the orbit round the planet's own centre that the body is on at that instant. The state is exact at the
//! instant, so the path is continuous there, and it is the same whatever the tick rate of the server is.

use std::cmp::Ordering;

use voxel_space::GridId;
use wide_float::{Precision, WideFloat};
use wide_kepler::{Elements, Time};
use world_storage::rail::{Centre, Orbit, RailTime};

const STAR: GridId = GridId::new(1000);
const PLANET: GridId = GridId::new(100);
const MOON_PLANET: GridId = GridId::new(50);

/// `G M` of the star, of the planet (a millionth of the star) and of a moon of the planet (a millionth of that).
const MU_STAR: f64 = 1.3e20;
const MU_PLANET: f64 = 1.3e14;

/// How far the planet is from the star, and the radius of its sphere of influence: a (m / M)^(2/5) = 1e9 x (1e-6)^0.4 = 3.98e6 voxels (4e6 here).
const PLANET_DISTANCE: f64 = 1.0e9;
const SPHERE: f64 = 4.0e6;

fn at(bits: u64) -> Precision {
    Precision::new(bits).unwrap()
}

fn tick(tick: u64) -> RailTime {
    RailTime::of_tick(tick, 20)
}

fn wide_time(time: RailTime) -> Time {
    Time {
        units: time.units(),
        units_per_second: time.units_per_second(),
    }
}

/// The planet's orbit round the star: a circle in the plane z = 0, found from its state at time 0.
fn planet_orbit() -> Orbit {
    let speed = libm::sqrt(MU_STAR / PLANET_DISTANCE);
    Orbit::from_place(
        Centre::Grid(STAR),
        [PLANET_DISTANCE, 0.0, 0.0],
        [0.0, speed, 0.0],
        MU_STAR,
        tick(0),
    )
    .unwrap()
}

/// A ship's orbit round the planet from its state against it at time 0: 1e6 voxels out along x, going `times_circular` times the speed of a circle there across the radius (and `along` of it
/// along the radius, so that it is not at an apsis).
fn ship_orbit(times_circular: f64, along: f64) -> Orbit {
    let circular = libm::sqrt(MU_PLANET / 1.0e6);
    Orbit::from_place(
        Centre::Grid(PLANET),
        [1.0e6, 0.0, 0.0],
        [circular * along, circular * times_circular, 0.0],
        MU_PLANET,
        tick(0),
    )
    .unwrap()
}

fn distance_squared(place: &[WideFloat; 3]) -> WideFloat {
    &(&place[0].square() + &place[1].square()) + &place[2].square()
}

fn wide(value: f64, bits: u64) -> WideFloat {
    WideFloat::from_f64(value, at(bits)).unwrap()
}

/// The instant after `time` on the grid: one unit.
fn one_unit_before(time: RailTime) -> Time {
    Time {
        units: time.units() - 1,
        units_per_second: time.units_per_second(),
    }
}

#[test]
fn an_ellipse_that_goes_past_the_sphere_leaves_it_at_the_first_instant_of_the_grid_it_is_out_at() {
    // 1.3 times the speed of a circle: an ellipse from 1e6 out to 5.4e6, past the sphere at 4e6.
    let orbit = ship_orbit(1.3, 0.0);
    assert!(orbit.farthest_distance() > SPHERE);
    let found = orbit
        .leaves_at(SPHERE, tick(0))
        .unwrap()
        .expect("it leaves");
    assert_eq!(found.units_per_second(), 1 << 31);
    let limit = wide(SPHERE, 256).square();
    let place =
        |time: Time| distance_squared(&orbit.elements.place_at(time, at(256)).unwrap().position);
    assert!(place(wide_time(found)).compare(&limit).is_ge());
    assert!(place(one_unit_before(found)).compare(&limit).is_lt());
}

#[test]
fn an_orbit_that_stays_inside_the_sphere_never_leaves_it() {
    // 1.1 times the speed of a circle at 1e6: the far end is 1.5e6, well inside.
    let orbit = ship_orbit(1.1, 0.0);
    assert!(orbit.farthest_distance() < SPHERE);
    assert_eq!(orbit.leaves_at(SPHERE, tick(0)).unwrap(), None);
}

#[test]
fn a_hyperbola_leaves_every_sphere() {
    let orbit = ship_orbit(1.6, 0.0);
    assert!(orbit.is_a_hyperbola());
    let found = orbit
        .leaves_at(SPHERE, tick(0))
        .unwrap()
        .expect("it goes out");
    assert!(found.compare(tick(0)) == Ordering::Greater);
}

#[test]
fn the_instant_is_the_same_asked_at_any_tick_rate() {
    let orbit = ship_orbit(1.3, 0.2);
    let slow = orbit.leaves_at(SPHERE, RailTime::of_tick(3, 20)).unwrap();
    let fast = orbit
        .leaves_at(SPHERE, RailTime::of_tick(300, 2000))
        .unwrap();
    let odd = orbit.leaves_at(SPHERE, RailTime::of_tick(7, 46)).unwrap();
    assert!(slow.is_some());
    assert_eq!(slow, fast);
    // (7 / 46 s is not 3 / 20 s, but the crossing is later than both and the instant does not depend on where it is asked from.)
    assert_eq!(slow, odd);
}

#[test]
fn a_radius_that_is_no_distance_is_refused() {
    let orbit = ship_orbit(1.3, 0.0);
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(orbit.leaves_at(radius, tick(0)).is_err(), "{radius}");
    }
}

/// The state of the ship from the star at an instant, from the two orbits, in 320 bits.
fn state_from_the_star(
    ship: &Orbit,
    planet: &Orbit,
    time: Time,
) -> ([WideFloat; 3], [WideFloat; 3]) {
    let own = ship.elements.place_at(time, at(320)).unwrap();
    let centre = planet.elements.place_at(time, at(320)).unwrap();
    (
        [0, 1, 2].map(|axis| &own.position[axis] + &centre.position[axis]),
        [0, 1, 2].map(|axis| &own.velocity[axis] + &centre.velocity[axis]),
    )
}

/// How many bits of the state the orbit holds at the instant: `-log2(|difference| / |state|)`, the least over the place and the speed.
fn bits_that_agree(orbit: &Orbit, expected: &([WideFloat; 3], [WideFloat; 3]), time: Time) -> i64 {
    let got = orbit.elements.place_at(time, at(320)).unwrap();
    let mut worst = i64::MAX;
    for (got, expected) in [(&got.position, &expected.0), (&got.velocity, &expected.1)] {
        let size = distance_squared(expected).sqrt().unwrap();
        let error = distance_squared(&[0, 1, 2].map(|axis| &got[axis] - &expected[axis]))
            .sqrt()
            .unwrap();
        let bits = error
            .floor_log2()
            .map_or(i64::MAX / 2, |e| size.floor_log2().unwrap() - e);
        worst = worst.min(bits);
    }
    worst
}

#[test]
fn the_orbit_after_a_hand_over_holds_the_state_at_the_instant_to_nearly_the_width_of_its_elements()
{
    let planet = planet_orbit();
    let ship = ship_orbit(1.3, 0.2);
    let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
    let after = ship
        .handed_over_to(&planet, MU_STAR, instant, 65_536)
        .unwrap();
    assert_eq!(after.around, Centre::Grid(STAR));
    // 128 bits and the 32 that a hand-over adds.
    assert_eq!(after.elements.radius.precision().bits(), 160);
    let expected = state_from_the_star(&ship, &planet, wide_time(instant));
    let agree = bits_that_agree(&after, &expected, wide_time(instant));
    assert!(agree >= 150, "{agree} bits agree");
    // It is round the star: its mass parameter is the star's.
    assert!((after.mass_parameter() / MU_STAR - 1.0).abs() < 1e-12);
    // And the ship is out of the sphere of the planet from there on: farther than the sphere from the planet a little after, and the orbit passes through the state a little before as
    // well (it is the orbit of that state, not a path that starts there).
    let later = Time {
        units: wide_time(instant).units + (1 << 31) * 100,
        units_per_second: wide_time(instant).units_per_second,
    };
    let planet_then = planet.elements.place_at(later, at(256)).unwrap().position;
    let ship_then = after.elements.place_at(later, at(256)).unwrap().position;
    let apart = distance_squared(&[0, 1, 2].map(|axis| &ship_then[axis] - &planet_then[axis]));
    assert!(apart.compare(&wide(SPHERE, 256).square()).is_gt());
}

#[test]
fn a_hand_over_adds_32_bits_and_never_goes_past_the_widest_rail_or_below_128() {
    let planet = planet_orbit();
    let ship = ship_orbit(1.6, 0.1);
    let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
    let bits = |widest: u64| {
        ship.handed_over_to(&planet, MU_STAR, instant, widest)
            .unwrap()
            .elements
            .radius
            .precision()
            .bits()
    };
    assert_eq!(bits(65_536), 160);
    assert_eq!(bits(160), 160);
    assert_eq!(bits(150), 150);
    // The least a rail has is 128 bits, whatever the widest is said to be.
    assert_eq!(bits(128), 128);
    assert_eq!(bits(64), 128);
}

#[test]
fn the_width_is_that_of_the_wider_of_the_ship_and_the_planet_it_is_handed_over_through() {
    // A planet whose orbit was made in 256 bits (it came from a hand-over of its own, say) and a ship of 128: the ship's new orbit has 256 and the 32 more.
    let narrow = planet_orbit();
    let speed = libm::sqrt(MU_STAR / PLANET_DISTANCE);
    let exactly = |value: f64| wide(value, 256);
    let wide_planet = Orbit::from_wide_state(
        Centre::Grid(STAR),
        &[exactly(PLANET_DISTANCE), exactly(0.0), exactly(0.0)],
        &[exactly(0.0), exactly(speed), exactly(0.0)],
        &exactly(MU_STAR),
        tick(0),
        at(256),
    )
    .unwrap();
    let ship = ship_orbit(1.3, 0.2);
    let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
    let width = |planet: &Orbit| {
        ship.handed_over_to(planet, MU_STAR, instant, 65_536)
            .unwrap()
            .elements
            .radius
            .precision()
            .bits()
    };
    assert_eq!(width(&narrow), 160);
    assert_eq!(width(&wide_planet), 288);
}

#[test]
fn the_orbit_is_the_one_through_the_exact_state_and_not_through_a_rounded_one() {
    // A planet that hardly moves (its star pulls with a hundred-billionth of the strength) and a ship that leaves it nearly straight out: after the hand-over the orbit is a hyperbola
    // of an eccentricity of hundreds, which magnifies what the state does not hold. The orbit is the one the state is on **to the last bit of its width**: the same as the fit of
    // the state worked out in 320 bits, 160 more than the orbit has.
    let slow_mu = 1.0e11;
    let slow_speed = libm::sqrt(slow_mu / PLANET_DISTANCE);
    let planet = Orbit::from_place(
        Centre::Grid(STAR),
        [PLANET_DISTANCE, 0.0, 0.0],
        [0.0, slow_speed, 0.0],
        slow_mu,
        tick(0),
    )
    .unwrap();
    for (times_circular, along) in [(1.0e-4, 1.35), (1.0e-3, 1.4), (0.2, 1.3)] {
        let ship = ship_orbit(times_circular, along);
        let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
        let after = ship
            .handed_over_to(&planet, slow_mu, instant, 65_536)
            .unwrap();
        assert!(
            after.is_a_hyperbola() && after.ratio() < -10.0,
            "{}",
            after.ratio()
        );
        let (position, velocity) = state_from_the_star(&ship, &planet, wide_time(instant));
        let through_the_exact_state = Orbit::from_wide_state(
            planet.around,
            &position,
            &velocity,
            &wide(slow_mu, 320),
            instant,
            at(160),
        )
        .unwrap();
        // (The fit does not know it is a change along a path.)
        let through_the_exact_state = Orbit {
            changes: 1,
            ..through_the_exact_state
        };
        assert_eq!(after, through_the_exact_state, "{times_circular}");
    }
}

#[test]
fn hand_overs_in_a_row_each_add_bits_and_each_hold_the_state() {
    // A moon's ship leaves the moon's sphere into the planet's frame, and then the planet's sphere into the star's frame.
    let moon_distance = 2.0e5;
    let moon_mu = MU_PLANET * 1.0e-6;
    let moon_sphere = 2.0e4;
    let planet = planet_orbit();
    // The moon: a circle round the planet.
    let moon = Orbit::from_place(
        Centre::Grid(PLANET),
        [moon_distance, 0.0, 0.0],
        [0.0, libm::sqrt(MU_PLANET / moon_distance), 0.0],
        MU_PLANET,
        tick(0),
    )
    .unwrap();
    let circular = libm::sqrt(moon_mu / 5.0e3);
    let ship = Orbit::from_place(
        Centre::Grid(MOON_PLANET),
        [5.0e3, 0.0, 0.0],
        [0.1 * circular, 200.0 * circular, 0.0],
        moon_mu,
        tick(0),
    )
    .unwrap();
    assert!(ship.is_a_hyperbola());
    let first = ship.leaves_at(moon_sphere, tick(0)).unwrap().unwrap();
    let in_the_planet_frame = ship
        .handed_over_to(&moon, MU_PLANET, first, 65_536)
        .unwrap();
    assert_eq!(in_the_planet_frame.around, Centre::Grid(PLANET));
    assert_eq!(in_the_planet_frame.elements.radius.precision().bits(), 160);
    let expected = state_from_the_star(&ship, &moon, wide_time(first));
    assert!(bits_that_agree(&in_the_planet_frame, &expected, wide_time(first)) >= 150);

    let second = in_the_planet_frame
        .leaves_at(SPHERE, first)
        .unwrap()
        .unwrap();
    assert!(second.compare(first) == Ordering::Greater);
    let in_the_star_frame = in_the_planet_frame
        .handed_over_to(&planet, MU_STAR, second, 65_536)
        .unwrap();
    assert_eq!(in_the_star_frame.around, Centre::Grid(STAR));
    assert_eq!(in_the_star_frame.elements.radius.precision().bits(), 192);
    let expected = state_from_the_star(&in_the_planet_frame, &planet, wide_time(second));
    assert!(bits_that_agree(&in_the_star_frame, &expected, wide_time(second)) >= 180);
}

#[test]
fn a_state_that_is_on_no_orbit_is_refused() {
    let planet = planet_orbit();
    // A ship going straight away from the planet has no plane to be in.
    let straight = Orbit::from_place(
        Centre::Grid(PLANET),
        [1.0e6, 0.0, 0.0],
        [5.0e4, 0.0, 0.0],
        MU_PLANET,
        tick(0),
    );
    assert!(straight.is_err());
    // A star that weighs nothing.
    let ship = ship_orbit(1.3, 0.2);
    let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
    for mu in [0.0, -1.0, f64::NAN] {
        assert!(
            ship.handed_over_to(&planet, mu, instant, 65_536).is_err(),
            "{mu}"
        );
    }
}

#[test]
fn elements_are_made_of_the_wide_kepler_ones() {
    // (A guard that the types of the tests are the types of the crate: an orbit's elements are `wide_kepler::Elements`.)
    let ship = ship_orbit(1.3, 0.0);
    let _: &Elements = &ship.elements;
}

#[test]
fn times_are_compared_as_moments_whatever_their_units() {
    use Ordering::{Equal, Greater, Less};
    let seconds = |seconds: f64| RailTime::from(seconds);
    let cases = [
        // The same moment in two rates is neither earlier nor later.
        (
            RailTime::of_tick(3, 20),
            RailTime::of_tick(300, 2000),
            Equal,
        ),
        (seconds(0.0), RailTime::of_tick(0, 7), Equal),
        (seconds(0.5), RailTime::of_tick(10, 20), Equal),
        // Earlier and later, in rates that do not divide each other.
        (RailTime::of_tick(1, 20), seconds(0.049), Greater),
        (seconds(0.049), RailTime::of_tick(1, 20), Less),
        (RailTime::of_tick(7, 46), RailTime::of_tick(3, 20), Greater),
        // A moment before the world began.
        (seconds(-1.0), RailTime::of_tick(0, 20), Less),
        (RailTime::of_tick(0, 20), seconds(-1.0), Greater),
        (seconds(-0.5), seconds(-1.0), Greater),
        (seconds(-1.0), seconds(-0.5), Less),
        (seconds(-1.0), seconds(-1.0), Equal),
        // The largest ticks at the fastest rate (a product of up to 128 bits).
        (
            RailTime::of_tick(u64::MAX, u32::MAX),
            RailTime::of_tick(u64::MAX - 1, u32::MAX),
            Greater,
        ),
        (
            RailTime::of_tick(u64::MAX, 1),
            RailTime::of_tick(u64::MAX, 2),
            Greater,
        ),
    ];
    for (first, second, expected) in cases {
        assert_eq!(
            first.compare(second),
            expected,
            "{first:?} against {second:?}"
        );
    }
}

#[test]
fn a_hand_over_is_one_change_further_along_the_path_and_keeps_the_prediction_that_was_asked_for() {
    let planet = planet_orbit();
    let mut ship = ship_orbit(1.3, 0.2);
    assert_eq!((ship.changes, ship.predicted), (0, 0));
    ship.predicted = 7;
    let instant = ship.leaves_at(SPHERE, tick(0)).unwrap().unwrap();
    let after = ship
        .handed_over_to(&planet, MU_STAR, instant, 65_536)
        .unwrap();
    assert_eq!((after.changes, after.predicted), (1, 7));
    // And once more from an orbit that is some changes in.
    let mut later = ship.clone();
    later.changes = 41;
    let after = later
        .handed_over_to(&planet, MU_STAR, instant, 65_536)
        .unwrap();
    assert_eq!((after.changes, after.predicted), (42, 7));
}
