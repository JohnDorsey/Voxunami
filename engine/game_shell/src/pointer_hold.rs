//! Left Alt, held, gives the mouse pointer back: while it is down the mouse moves the pointer and not the camera, and when it is let go the mouse is captured again.
//!
//! The window does the capturing; this only says what Left Alt asks of it, so that the rule (what is given back, what is taken again, and what is forgotten) is tested.

/// What Left Alt has done to the mouse capture.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PointerHold {
    /// The pointer was given back by Left Alt, which is still down, and is to be captured when it comes up.
    given_back: bool,
}

impl PointerHold {
    /// Left Alt went down (not a repeat of the press). `captured` is whether the mouse is captured now. Returns whether the window is to release it.
    pub fn alt_pressed(&mut self, captured: bool) -> bool {
        if captured {
            self.given_back = true;
        }
        captured
    }

    /// Left Alt came up. Returns whether the window is to capture the mouse again: only if this key had given it back.
    pub fn alt_released(&mut self) -> bool {
        std::mem::take(&mut self.given_back)
    }

    /// The window lost the focus: the key may come up in another window, so what was given back is not taken again.
    pub fn focus_lost(&mut self) {
        self.given_back = false;
    }

    /// Whether Left Alt is holding the pointer free now.
    #[cfg(test)]
    pub fn is_holding(&self) -> bool {
        self.given_back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_gives_the_pointer_back_while_it_is_down_and_takes_it_again_when_it_comes_up() {
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true), "a captured mouse is released");
        assert!(hold.is_holding());
        assert!(hold.alt_released(), "and captured again");
        assert!(!hold.is_holding());
    }

    #[test]
    fn alt_with_the_pointer_already_free_changes_nothing_and_so_does_letting_go() {
        let mut hold = PointerHold::default();
        assert!(!hold.alt_pressed(false));
        assert!(!hold.is_holding());
        assert!(
            !hold.alt_released(),
            "a pointer that was free is not captured by letting go of the key"
        );
    }

    #[test]
    fn a_release_is_taken_again_once_only() {
        let mut hold = PointerHold::default();
        hold.alt_pressed(true);
        assert!(hold.alt_released());
        assert!(!hold.alt_released());
    }

    #[test]
    fn losing_the_focus_with_alt_down_forgets_it_so_the_mouse_is_not_taken_when_the_window_gets_it_back()
     {
        let mut hold = PointerHold::default();
        hold.alt_pressed(true);
        hold.focus_lost();
        assert!(!hold.is_holding());
        assert!(!hold.alt_released(), "the key came up in another window");
    }

    #[test]
    fn pressing_alt_again_while_it_holds_keeps_holding() {
        // (A second press with the mouse already free, as a repeat that was not told as one would be.)
        let mut hold = PointerHold::default();
        assert!(hold.alt_pressed(true));
        assert!(!hold.alt_pressed(false));
        assert!(hold.is_holding(), "the first press still holds it");
        assert!(hold.alt_released());
    }
}
