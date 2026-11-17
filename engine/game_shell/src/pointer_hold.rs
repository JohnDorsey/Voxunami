//! What is owed to the mouse capture. Left Alt, held, gives the mouse pointer back: while it is down the mouse moves the pointer and not the camera, and when it is let go the mouse is
//! captured again. And a window that was left with the mouse captured (or with Left Alt holding it free) captures it again when the player is back, as soon as Left Alt is up.
//!
//! The window does the capturing; this only says when it is to, so that the rule (what is given back, what is taken again, and what is forgotten) is tested.

/// What the window owes the capture of the mouse.
#[derive(Debug, PartialEq, Eq)]
pub struct PointerHold {
    /// The mouse was captured and is to be captured again when it can be: Left Alt gave it back, or the window was left with it captured.
    owed: bool,
    /// Left Alt is down (as far as the window has heard).
    alt_down: bool,
    /// The window has the focus.
    focused: bool,
}

impl Default for PointerHold {
    fn default() -> Self {
        Self {
            owed: false,
            alt_down: false,
            focused: true,
        }
    }
}

impl PointerHold {
    /// Left Alt went down (not a repeat of the press). `captured` is whether the mouse is captured now. Returns whether the window is to release it.
    pub fn alt_pressed(&mut self, captured: bool) -> bool {
        self.alt_down = true;
        if captured {
            self.owed = true;
        }
        captured
    }

    /// The window was given the focus with Left Alt down (the press that was before it is not heard, and the system tells of it): the mouse is taken when it comes up.
    pub fn alt_found_down(&mut self) {
        self.alt_down = true;
    }

    /// Left Alt came up.
    pub fn alt_released(&mut self) {
        self.alt_down = false;
    }

    /// The window lost the focus. `captured` is whether the mouse was captured: if it was, it is owed back. Left Alt is forgotten, since it may come up in another window.
    pub fn focus_lost(&mut self, captured: bool) {
        self.owed |= captured;
        self.alt_down = false;
        self.focused = false;
    }

    /// The window has the focus again.
    pub fn focus_gained(&mut self) {
        self.focused = true;
    }

    /// Asked once a frame (and when a mouse button goes down in paint mode): whether the window is to capture the mouse now, which it is when it is owed, the window has the
    /// focus and Left Alt is up. The debt is paid by asking: once true, it is not true again. A mouse that is captured, however it came to be, owes nothing.
    pub fn recapture_due(&mut self, captured: bool) -> bool {
        if captured {
            self.owed = false;
            return false;
        }
        if !self.owed || !self.focused || self.alt_down {
            return false;
        }
        self.owed = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_gives_the_pointer_back_while_it_is_down_and_takes_it_again_when_it_comes_up() {
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true), "a captured mouse is released");
        assert!(!hold.recapture_due(false), "not while Left Alt is down");
        hold.alt_released();
        assert!(
            hold.recapture_due(false),
            "and captured again when it is up"
        );
        assert!(!hold.recapture_due(false), "once only");
    }

    #[test]
    fn alt_with_the_pointer_already_free_changes_nothing_and_so_does_letting_go() {
        let mut hold = PointerHold::default();
        assert!(!hold.alt_pressed(false));
        hold.alt_released();
        assert!(
            !hold.recapture_due(false),
            "a pointer that was free is not captured by letting go of the key"
        );
    }

    #[test]
    fn pressing_alt_again_while_it_holds_keeps_holding() {
        // (A second press with the mouse already free, as a repeat that was not told as one would be.)
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true));
        assert!(!hold.alt_pressed(false));
        assert!(!hold.recapture_due(false));
        hold.alt_released();
        assert!(hold.recapture_due(false), "the first press still holds it");
    }

    #[test]
    fn a_window_left_with_the_mouse_captured_captures_it_again_when_the_player_is_back() {
        let mut hold = PointerHold::default();
        hold.focus_lost(true);
        assert!(
            !hold.recapture_due(false),
            "not while the window has no focus"
        );
        hold.focus_gained();
        assert!(hold.recapture_due(false), "and at once when it has");
        assert!(!hold.recapture_due(false), "once only");
    }

    #[test]
    fn a_window_left_with_the_mouse_free_does_not_capture_it_when_the_player_is_back() {
        let mut hold = PointerHold::default();
        hold.focus_lost(false);
        hold.focus_gained();
        assert!(!hold.recapture_due(false));
    }

    #[test]
    fn a_window_left_while_alt_held_the_pointer_free_captures_it_when_the_player_is_back_whether_or_not_alt_came_up_in_the_window()
     {
        // Alt+Tab: Left Alt goes down, the window loses the focus, and Left Alt comes up in the window (the system makes the release up as the focus goes, before or after),
        // or in another one (never heard).
        for (released_before_the_focus_goes, released_after_it_goes) in
            [(true, false), (false, true), (false, false)]
        {
            let mut hold = PointerHold::default();
            assert!(hold.alt_pressed(true));
            if released_before_the_focus_goes {
                hold.alt_released();
            }
            hold.focus_lost(false);
            if released_after_it_goes {
                hold.alt_released();
            }
            assert!(
                !hold.recapture_due(false),
                "not while the window has no focus"
            );
            hold.focus_gained();
            assert!(
                hold.recapture_due(false),
                "back with Left Alt up: captured ({released_before_the_focus_goes}, {released_after_it_goes})"
            );
        }
    }

    #[test]
    fn a_window_that_is_back_with_alt_still_down_captures_the_mouse_when_alt_comes_up() {
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true));
        hold.focus_lost(false);
        hold.focus_gained();
        hold.alt_found_down();
        assert!(!hold.recapture_due(false), "Left Alt is down");
        hold.alt_released();
        assert!(hold.recapture_due(false));
    }

    #[test]
    fn finding_alt_down_on_gaining_the_focus_owes_nothing() {
        let mut hold = PointerHold::default();
        hold.alt_found_down();
        hold.alt_released();
        assert!(!hold.recapture_due(false));
    }

    #[test]
    fn a_mouse_that_gets_captured_otherwise_owes_nothing() {
        // The player clicks the window while Left Alt holds the pointer free (or just back): the click captured it.
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true));
        assert!(!hold.recapture_due(true));
        hold.alt_released();
        assert!(!hold.recapture_due(false), "nothing is taken a second time");

        let mut hold = PointerHold::default();
        hold.focus_lost(true);
        hold.focus_gained();
        assert!(!hold.recapture_due(true));
        assert!(!hold.recapture_due(false));
    }
}
