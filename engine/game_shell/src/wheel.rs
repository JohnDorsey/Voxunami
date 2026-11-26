//! The mouse wheel as presses: what the window turns the turns of a wheel (or the slide of two fingers on a touchpad) into, so that a game binds an action to
//! [`crate::client_plugin::MouseButton::WheelUp`] and `WheelDown` as it binds one to a button.
//!
//! A wheel with notches says one line for each click; a smooth wheel or a touchpad says fractions of a line or pixels, many times a second. [`WheelNotches`] adds what the window
//! hears and says when a whole notch has gone by, so that a touchpad does not turn the hotbar round ten times for one stroke.

/// How many pixels of a touchpad's slide are one notch of a wheel (the height of a line of text, as most systems have it).
const PIXELS_PER_NOTCH: f64 = 40.0;

/// The most notches that one event can give (a number that is not a real turn of a wheel, such as a driver's mistake, must not make the game do the same thing a billion times).
const MOST_NOTCHES_AT_ONCE: f64 = 32.0;

/// A slide of a touchpad in lines of a wheel (the sign as it is).
pub(crate) fn lines_of_pixels(pixels: f64) -> f64 {
    pixels / PIXELS_PER_NOTCH
}

/// The turns of a wheel not yet a whole notch.
#[derive(Debug, Default)]
pub(crate) struct WheelNotches {
    /// Less than one notch, with the sign of the way it was going.
    remainder: f64,
}

impl WheelNotches {
    /// Adds `lines` (the window's own sign: positive is the wheel turned away from the player, which is also the content going down) and gives the whole notches now done, positive
    /// for away and negative for toward. Not a number, or nothing, gives none. A turn the other way than the part notch waiting drops that part: a wheel that was stopped short and
    /// turned back is a new turn.
    pub(crate) fn add_lines(&mut self, lines: f64) -> i32 {
        if !lines.is_finite() || lines == 0.0 {
            return 0;
        }
        if self.remainder * lines < 0.0 {
            self.remainder = 0.0;
        }
        let total = self.remainder + lines;
        let whole = total.trunc();
        self.remainder = total - whole;
        whole.clamp(-MOST_NOTCHES_AT_ONCE, MOST_NOTCHES_AT_ONCE) as i32
    }

    /// The same for a slide of `pixels` (the sign as for lines).
    pub(crate) fn add_pixels(&mut self, pixels: f64) -> i32 {
        self.add_lines(lines_of_pixels(pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_of_a_wheel_is_a_notch_the_way_it_turned() {
        let mut wheel = WheelNotches::default();
        assert_eq!(wheel.add_lines(1.0), 1, "away from the player");
        assert_eq!(wheel.add_lines(-1.0), -1, "toward the player");
        assert_eq!(
            wheel.add_lines(3.0),
            3,
            "a fast turn is three notches in one event"
        );
        assert_eq!(wheel.add_lines(-2.0), -2);
    }

    #[test]
    fn parts_of_a_line_are_added_until_they_make_a_notch_and_the_rest_is_kept() {
        let mut wheel = WheelNotches::default();
        assert_eq!(wheel.add_lines(0.4), 0);
        assert_eq!(wheel.add_lines(0.4), 0);
        assert_eq!(wheel.add_lines(0.4), 1, "1.2 in all");
        // The 0.2 that was left counts toward the next.
        assert_eq!(wheel.add_lines(0.7), 0);
        assert_eq!(wheel.add_lines(0.2), 1, "0.2 + 0.7 + 0.2 = 1.1");
    }

    #[test]
    fn a_touchpad_slide_of_forty_pixels_is_a_notch() {
        let mut wheel = WheelNotches::default();
        // (Halves and quarters of a notch are exact in a double, so no sum here is a hair under a whole one.)
        assert_eq!(wheel.add_pixels(20.0), 0);
        assert_eq!(wheel.add_pixels(20.0), 1);
        assert_eq!(wheel.add_pixels(-80.0), -2);
        // A stroke that comes in small steps is the same as one that comes whole.
        let steps: i32 = (0..20).map(|_| wheel.add_pixels(10.0)).sum();
        assert_eq!(steps, 5, "200 pixels");
    }

    #[test]
    fn turning_back_drops_the_part_of_a_notch_that_was_waiting() {
        let mut wheel = WheelNotches::default();
        assert_eq!(wheel.add_lines(0.6), 0);
        // Not 0.6 - 0.6 = 0 then 0.6 + 0.6 = 1.2 -> a notch in the wrong way after a reversal: the turn back starts again from nothing.
        assert_eq!(wheel.add_lines(-0.6), 0);
        assert_eq!(
            wheel.add_lines(-0.6),
            -1,
            "-1.2 from the start of the turn back"
        );
        assert_eq!(
            wheel.add_lines(0.6),
            0,
            "and the part left of that is dropped when it turns away again"
        );
        assert_eq!(wheel.add_lines(0.6), 1);
    }

    #[test]
    fn no_turn_and_no_number_give_nothing_and_leave_what_was_waiting() {
        let mut wheel = WheelNotches::default();
        assert_eq!(wheel.add_lines(0.5), 0);
        assert_eq!(wheel.add_lines(0.0), 0);
        assert_eq!(wheel.add_lines(f64::NAN), 0);
        assert_eq!(wheel.add_lines(f64::INFINITY), 0);
        assert_eq!(wheel.add_lines(f64::NEG_INFINITY), 0);
        assert_eq!(
            wheel.add_lines(0.5),
            1,
            "the half that was waiting is still there"
        );
    }

    #[test]
    fn one_event_is_never_more_than_thirty_two_notches() {
        let mut wheel = WheelNotches::default();
        assert_eq!(wheel.add_lines(1.0e300), 32);
        assert_eq!(wheel.add_lines(-1.0e300), -32);
        assert_eq!(wheel.add_lines(32.0), 32);
        assert_eq!(wheel.add_lines(33.0), 32);
        assert_eq!(
            wheel.add_lines(1.0),
            1,
            "nothing is left over from the clamp"
        );
    }
}
