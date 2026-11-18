"""Tests for the state behind the channel controls: a mode and two values (primary and secondary) for each channel, the
modes of RGB and alpha, what a typed number or a slider position means, and what the eye dropper and the swap do to them.
No window except in the last class.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import LABPBR, REFLECTANCE, SMOOTHNESS, neutral_planes, solid_tile  # noqa: E402
from test_virtual_atlas import BLUE, EditorDriver, va  # noqa: E402

PROFILE = va.PBR_PROFILES[LABPBR]
IDENTITIES = [channel.identity for channel in PROFILE.channels]


def controls():
    state = va.ChannelControls()
    state.follow(PROFILE)
    return state


class RowTests(unittest.TestCase):
    def test_the_rows_are_the_channels_of_the_profile_in_order(self):
        self.assertEqual(controls().rows, IDENTITIES)

    def test_under_none_there_are_no_rows(self):
        state = va.ChannelControls()
        state.follow(va.PBR_PROFILES["none"])
        self.assertEqual(state.rows, [])
        self.assertEqual(state.blends("primary"), {})

    def test_every_channel_starts_with_mode_none_and_its_neutral_meaning_in_both_values(self):
        state = controls()
        for identity in IDENTITIES:
            self.assertEqual(state.mode(identity), "none")
        for slot in ("primary", "secondary"):
            self.assertEqual(state.value("height", slot), 1.0)
            self.assertEqual(state.value("smoothness", slot), 0.0)
            self.assertEqual(state.value("normal_x", slot), 0.0)
            self.assertEqual(state.value("ambient_occlusion", slot), 0.0)

    def test_the_colour_starts_with_replace_for_rgb_and_alpha(self):
        state = controls()
        self.assertEqual((state.mode("rgb"), state.mode("alpha")), ("replace", "replace"))
        self.assertTrue(state.color_blend().is_replace)

    def test_a_row_that_stays_when_the_profile_changes_keeps_its_state_and_the_others_go(self):
        state = controls()
        state.set_mode("smoothness", "add")
        state.set_value("smoothness", "primary", 0.3)
        state.set_mode("rgb", "multiply")
        small = va.PbrProfile(
            "small",
            (
                va.PbrChannel("smoothness", "_s", 0, 0),
                va.PbrChannel("height", "_s", 1, 255),
                va.PbrChannel("reflectance", "_s", 2, 0),
                va.PbrChannel("emission", "_s", 3, 255),
            ),
        )
        state.follow(small)
        self.assertEqual(state.rows, ["smoothness", "height", "reflectance", "emission"])
        self.assertEqual((state.mode("smoothness"), state.value("smoothness", "primary")), ("add", 0.3))
        self.assertEqual(state.mode("rgb"), "multiply")
        state.follow(PROFILE)
        self.assertEqual(state.mode("normal_x"), "none", "a row that was gone starts again")

    def test_an_unknown_row_or_mode_is_refused(self):
        state = controls()
        with self.assertRaises(ValueError):
            state.set_mode("colour", "add")
        with self.assertRaises(ValueError):
            state.set_mode("smoothness", "overlay")
        with self.assertRaises(ValueError):
            state.set_value("colour", "primary", 0.5)
        with self.assertRaises(ValueError):
            state.set_value("smoothness", "tertiary", 0.5)


class ValueTests(unittest.TestCase):
    def test_values_are_kept_in_the_range_of_the_channel(self):
        state = controls()
        state.set_value("smoothness", "primary", 7.0)
        state.set_value("normal_x", "secondary", -7.0)
        self.assertEqual(state.value("smoothness", "primary"), 1.0)
        self.assertEqual(state.value("normal_x", "secondary"), -1.0)
        state.set_value("smoothness", "primary", -2.0)
        self.assertEqual(state.value("smoothness", "primary"), 0.0)

    def test_a_metal_is_kept_for_reflectance_only(self):
        state = controls()
        state.set_value("reflectance", "primary", va.Metal(5))
        self.assertEqual(state.value("reflectance", "primary"), va.Metal(5))
        with self.assertRaises(ValueError):
            state.set_value("smoothness", "primary", va.Metal(5))

    def test_the_blends_of_a_slot_carry_that_slots_values_and_the_modes(self):
        state = controls()
        state.set_mode("smoothness", "add")
        state.set_value("smoothness", "primary", 0.2)
        state.set_value("smoothness", "secondary", 0.9)
        self.assertEqual(state.blends("primary")["smoothness"], va.ChannelBlend("add", 0.2))
        self.assertEqual(state.blends("secondary")["smoothness"], va.ChannelBlend("add", 0.9))
        self.assertEqual(state.blends("primary")["emission"], va.ChannelBlend("none", 0.0))
        self.assertEqual(list(state.blends("primary")), IDENTITIES)

    def test_the_colour_blend_follows_the_modes_of_rgb_and_alpha(self):
        state = controls()
        state.set_mode("rgb", "add")
        state.set_mode("alpha", "none")
        self.assertEqual(state.color_blend(), va.ColorBlend("add", "none"))

    def test_swapping_exchanges_the_values_of_every_channel_and_leaves_the_modes(self):
        state = controls()
        state.set_mode("smoothness", "add")
        state.set_value("smoothness", "primary", 0.2)
        state.set_value("smoothness", "secondary", 0.9)
        state.set_value("reflectance", "secondary", va.Metal(3))
        state.swap()
        self.assertEqual((state.value("smoothness", "primary"), state.value("smoothness", "secondary")), (0.9, 0.2))
        self.assertEqual(state.value("reflectance", "primary"), va.Metal(3))
        self.assertEqual(state.value("reflectance", "secondary"), 0.0)
        self.assertEqual(state.mode("smoothness"), "add")

    def test_loading_values_sets_the_slot_and_not_the_modes_and_ignores_unknown_rows(self):
        state = controls()
        state.load({"smoothness": 0.4, "reflectance": va.Metal(2), "colour": 0.1}, "secondary")
        self.assertEqual(state.value("smoothness", "secondary"), 0.4)
        self.assertEqual(state.value("reflectance", "secondary"), va.Metal(2))
        self.assertEqual(state.value("smoothness", "primary"), 0.0)
        self.assertEqual(state.mode("smoothness"), "none")


class TextAndSliderTests(unittest.TestCase):
    def test_a_value_is_written_with_two_decimals_and_a_metal_as_m_and_its_number(self):
        state = controls()
        state.set_value("smoothness", "primary", 0.2)
        state.set_value("normal_x", "primary", -0.5)
        state.set_value("reflectance", "primary", va.Metal(5))
        self.assertEqual(state.value_text("smoothness", "primary"), "0.20")
        self.assertEqual(state.value_text("normal_x", "primary"), "-0.50")
        self.assertEqual(state.value_text("reflectance", "primary"), "m5")

    def test_typed_numbers_are_read_and_kept_in_range(self):
        parse = va.ChannelControls.parse_value
        self.assertEqual(parse("smoothness", "0.2"), 0.2)
        self.assertEqual(parse("smoothness", " 1.5 "), 1.0)
        self.assertEqual(parse("normal_x", "-3"), -1.0)
        self.assertEqual(parse("normal_x", "-.25"), -0.25)

    def test_typed_text_that_is_not_a_value_is_refused(self):
        parse = va.ChannelControls.parse_value
        for text in ("", "abc", "0,5", "--1", "m5", "nan", "inf"):
            self.assertIsNone(parse("smoothness", text), text)

    def test_a_metal_can_be_typed_for_reflectance_only_and_only_from_0_to_25(self):
        parse = va.ChannelControls.parse_value
        self.assertEqual(parse("reflectance", "m5"), va.Metal(5))
        self.assertEqual(parse("reflectance", "M25"), va.Metal(25))
        self.assertIsNone(parse("reflectance", "m26"))
        self.assertIsNone(parse("reflectance", "m-1"))
        self.assertIsNone(parse("reflectance", "m"))

    def test_the_slider_runs_over_the_range_of_the_channel(self):
        state = controls()
        self.assertEqual(state.value_at("smoothness", 0.25), 0.25)
        self.assertEqual(state.value_at("normal_x", 0.25), -0.5)
        self.assertEqual(state.value_at("normal_x", 1.0), 1.0)
        state.set_value("normal_x", "primary", 0.5)
        self.assertEqual(state.fraction("normal_x", "primary"), 0.75)
        state.set_value("reflectance", "primary", va.Metal(5))
        self.assertEqual(state.fraction("reflectance", "primary"), 5 / 25)

    def test_the_arrow_keys_step_a_value_by_a_hundredth_or_by_a_tenth_with_shift(self):
        state = controls()
        state.set_value("smoothness", "primary", 0.2)
        state.step("smoothness", "primary", 1, False)
        self.assertEqual(state.value("smoothness", "primary"), 0.21)
        state.step("smoothness", "primary", -1, True)
        self.assertEqual(state.value("smoothness", "primary"), 0.11)
        state.step("smoothness", "primary", -5, True)
        self.assertEqual(state.value("smoothness", "primary"), 0.0)

    def test_stepping_a_metal_goes_to_the_next_metal(self):
        state = controls()
        state.set_value("reflectance", "primary", va.Metal(5))
        state.step("reflectance", "primary", 1, False)
        self.assertEqual(state.value("reflectance", "primary"), va.Metal(6))
        state.step("reflectance", "primary", 100, False)
        self.assertEqual(state.value("reflectance", "primary"), va.Metal(25))


class PickChannelValuesTests(unittest.TestCase):
    def layout(self):
        planes = neutral_planes(4)
        planes[1, 2, SMOOTHNESS] = 51
        planes[3, 0, REFLECTANCE] = 235
        return va.compose_canvas({(0, 0): solid_tile(BLUE, 4)}, 4, planes={(0, 0): planes})

    def test_the_values_of_a_pixel_as_a_stroke_would_paint_them(self):
        values = va.pick_channel_values(self.layout(), (2, 1), PROFILE)
        self.assertEqual(values["smoothness"], 0.2)
        self.assertEqual(va.pick_channel_values(self.layout(), (0, 3), PROFILE)["reflectance"], va.Metal(5))

    def test_a_pixel_outside_or_in_an_empty_place_has_none(self):
        self.assertIsNone(va.pick_channel_values(self.layout(), (9, 9), PROFILE))
        layout = va.compose_canvas(
            {(0, 0): solid_tile(BLUE, 4), (2, 0): solid_tile(BLUE, 4)}, 4,
            planes={(0, 0): neutral_planes(4), (2, 0): neutral_planes(4)},
        )
        self.assertIsNone(va.pick_channel_values(layout, (5, 1), PROFILE))

    def test_with_wrap_a_pixel_of_any_copy_counts(self):
        self.assertEqual(va.pick_channel_values(self.layout(), (6, 5), PROFILE, wrap=True)["smoothness"], 0.2)

    def test_a_canvas_with_no_channels_has_none(self):
        layout = va.compose_canvas({(0, 0): solid_tile(BLUE, 4)}, 4)
        self.assertIsNone(va.pick_channel_values(layout, (1, 1), PROFILE))


class WindowControlsTests(EditorDriver):
    def setUp(self):
        super().setUp()
        atlas_file = va.read_atlas_file(self.folder / "atlas.json")
        atlas_file.pbr_profile = LABPBR
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        self.project = va.AtlasProject.open(self.folder)
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))
        self.editor = self.application.editor
        self.editor.clipboard = self.clipboard
        self.select((0, 0))

    def test_the_controls_follow_the_profile_of_the_project(self):
        self.assertEqual(self.editor.controls.rows, IDENTITIES)

    def test_when_the_setting_is_changed_the_rows_follow_it_after_the_next_sync(self):
        self.editor.controls.set_mode("smoothness", "add")
        self.project.switch_pbr_profile("none")
        self.editor.sync()
        self.assertEqual(self.editor.controls.rows, [])
        self.project.switch_pbr_profile(LABPBR)
        self.editor.sync()
        self.assertEqual(self.editor.controls.rows, IDENTITIES)

    def test_a_stroke_paints_the_channels_the_controls_say_with_the_value_of_the_button(self):
        self.editor.controls.set_mode("smoothness", "replace")
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.editor.controls.set_value("smoothness", "secondary", 1.0)
        self.click_tool("pencil")
        self.send("down", self.position(2, 2), 1)
        self.send("up", self.position(2, 2), 1)
        self.send("down", self.position(4, 4), 3)
        self.send("up", self.position(4, 4), 3)
        planes = self.project.tiles[(0, 0)].planes
        self.assertEqual((int(planes[2, 2, SMOOTHNESS]), int(planes[4, 4, SMOOTHNESS])), (51, 255))

    def test_the_colour_modes_of_the_controls_are_the_ones_of_the_stroke(self):
        self.editor.controls.set_mode("rgb", "add")
        self.editor.controls.set_mode("alpha", "none")
        self.editor.colors.set_color((51, 51, 51, 255), "primary")
        self.click_tool("pencil")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(self.pixel((0, 0), 2, 2), (51, 51, 255, 255))

    def test_the_eye_dropper_loads_the_values_of_the_pixel_into_the_slot_and_not_the_modes(self):
        self.project.tiles[(0, 0)].planes[3, 3, SMOOTHNESS] = 51
        self.select()
        self.select((0, 0))
        self.click_tool("eyedropper")
        self.send("down", self.position(3, 3), 3)
        self.send("up", self.position(3, 3), 3)
        self.assertEqual(self.editor.controls.value("smoothness", "secondary"), 0.2)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.0)
        self.assertEqual(self.editor.controls.mode("smoothness"), "none")

    def test_the_key_x_swaps_the_two_colours_and_the_channel_values_together(self):
        self.editor.colors.set_color((1, 2, 3, 255), "primary")
        self.editor.colors.set_color((9, 8, 7, 255), "secondary")
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.editor.controls.set_value("smoothness", "secondary", 0.9)
        self.key(va.pygame.K_x)
        self.assertEqual(self.editor.colors.get("primary"), (9, 8, 7, 255))
        self.assertEqual(self.editor.colors.get("secondary"), (1, 2, 3, 255))
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.9)

    def test_typing_an_x_in_the_name_does_not_swap(self):
        self.editor.colors.set_color((1, 2, 3, 255), "primary")
        self.send("down", self.editor.areas.header.center)
        self.send("up", self.editor.areas.header.center)
        self.assertTrue(self.editor.name_focused)
        self.application.handle_event(
            va.pygame.event.Event(va.pygame.KEYDOWN, key=va.pygame.K_x, mod=0, unicode="x", scancode=0)
        )
        self.application.handle_event(va.pygame.event.Event(va.pygame.TEXTINPUT, text="x"))
        self.assertIn("x", self.editor.name_field.text)
        self.assertEqual(self.editor.colors.get("primary"), (1, 2, 3, 255))

    def test_ctrl_x_still_cuts_and_does_not_swap(self):
        self.editor.colors.set_color((1, 2, 3, 255), "primary")
        self.key(va.pygame.K_x, va.pygame.KMOD_CTRL)
        self.assertEqual(self.editor.colors.get("primary"), (1, 2, 3, 255))


if __name__ == "__main__":
    unittest.main()
