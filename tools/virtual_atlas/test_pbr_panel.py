"""Tests for the channel controls as drawn in the editor: where the parts are, and what the mouse and the keyboard do to them.
They drive the editor with events, as test_virtual_atlas.EditorDriver does.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import LABPBR, SMOOTHNESS  # noqa: E402
from test_virtual_atlas import BLUE, EditorDriver, solid_tile, va  # noqa: E402

IDENTITIES = [channel.identity for channel in va.PBR_PROFILES[LABPBR].channels]


class PanelDriver(EditorDriver):
    profile = LABPBR
    window_size = (1280, 800)

    def setUp(self):
        super().setUp()
        atlas_file = va.read_atlas_file(self.folder / "atlas.json")
        atlas_file.pbr_profile = self.profile
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        self.project = va.AtlasProject.open(self.folder)
        self.application = va.Application(self.project, va.pygame.Surface(self.window_size))
        self.editor = self.application.editor
        self.editor.clipboard = self.clipboard
        self.select((0, 0))

    def row(self, identity):
        return self.editor.channel_row_areas(identity)

    def text(self, text):
        self.application.handle_event(va.pygame.event.Event(va.pygame.TEXTINPUT, text=text))

    def press_key(self, key, modifiers=0):
        self.key(key, modifiers)

    def click(self, position, button=1):
        self.send("down", position, button)
        self.send("up", position, button)

    def choose_mode(self, key, mode):
        button = self.editor.mode_button(key)
        self.click(button.center)
        self.click(self.editor.mode_option_rectangles()[mode].center)


class LayoutTests(unittest.TestCase):
    SIZES = [(1280, 800), (1600, 900), (1000, 700), (900, 600)]

    def areas(self, size):
        rect = va.pygame.Rect(size[0] // 2, 0, size[0] // 2, size[1])
        return rect, va.compute_editor_areas(rect, list(va.TOOL_LABELS))

    def test_the_history_of_colours_is_under_the_hex_field_and_the_rgb_mode_under_it(self):
        rect, areas = self.areas((1280, 800))
        self.assertTrue(areas.history)
        for swatch in areas.history:
            self.assertGreaterEqual(swatch.top, areas.hex_field.bottom)
            self.assertGreaterEqual(swatch.left, areas.hex_field.left)
            self.assertLessEqual(swatch.right, areas.hex_field.right)
        self.assertGreaterEqual(areas.rgb_mode.top, max(swatch.bottom for swatch in areas.history))
        self.assertGreaterEqual(areas.alpha_mode.top, areas.rgb_mode.bottom)
        self.assertEqual((areas.rgb_mode.left, areas.rgb_mode.width), (areas.hex_field.left, areas.hex_field.width))
        self.assertLessEqual(areas.alpha_mode.bottom, rect.bottom)

    def test_all_sixteen_colours_of_the_history_have_a_place_at_the_usual_size(self):
        _, areas = self.areas((1280, 800))
        self.assertEqual(len(areas.history), va.COLOR_HISTORY_LENGTH)

    def test_the_colour_area_is_taller_than_it_was(self):
        _, areas = self.areas((1280, 800))
        self.assertGreater(areas.square.height, va.scaled(120))
        self.assertEqual(areas.hue_bar.height, areas.square.height)
        self.assertEqual(areas.alpha_bar.height, areas.square.height)

    def test_the_parts_do_not_overlap_and_stay_inside_the_editor(self):
        for size in self.SIZES:
            rect, areas = self.areas(size)
            parts = {
                "canvas": areas.canvas, "sidebar": areas.sidebar, "options": areas.options, "controls": areas.controls,
                "square": areas.square, "hue": areas.hue_bar, "alpha": areas.alpha_bar,
                "primary": areas.primary_swatch, "secondary": areas.secondary_swatch, "hex": areas.hex_field,
                "rgb": areas.rgb_mode, "alpha mode": areas.alpha_mode, "channels": areas.channel_panel,
            }
            parts.update({f"tool {name}": rectangle for name, rectangle in areas.tool_buttons.items()})
            for name, part in parts.items():
                self.assertTrue(rect.contains(part), f"{size}: {name} {part} is not inside {rect}")
            names = list(parts)
            for first_index, first in enumerate(names):
                for second in names[first_index + 1 :]:
                    self.assertFalse(parts[first].colliderect(parts[second]), f"{size}: {first} and {second} overlap")

    def test_the_channel_panel_is_right_of_the_colours_and_below_the_tools(self):
        _, areas = self.areas((1280, 800))
        self.assertGreaterEqual(areas.channel_panel.left, areas.rgb_mode.right)
        self.assertGreaterEqual(areas.channel_panel.top, max(button.bottom for button in areas.tool_buttons.values()))


class RowTests(PanelDriver):
    def test_every_channel_has_a_row_with_all_its_parts_inside_the_panel(self):
        panel = self.editor.areas.channel_panel
        for identity in IDENTITIES:
            parts = self.row(identity)
            self.assertEqual(set(parts), {"label", "mode", "primary", "secondary", "value"})
            for name, rectangle in parts.items():
                self.assertTrue(panel.contains(rectangle), f"{identity} {name}")
            order = [parts[name] for name in ("label", "mode", "primary", "secondary", "value")]
            for left, right in zip(order, order[1:]):
                self.assertLessEqual(left.right, right.left)

    def test_the_rows_are_in_the_order_of_the_profile_one_under_the_other(self):
        tops = [self.row(identity)["mode"].top for identity in IDENTITIES]
        self.assertEqual(tops, sorted(tops))
        self.assertEqual(len(set(tops)), len(IDENTITIES))

    def test_at_the_usual_size_every_row_fits_without_scrolling(self):
        self.assertEqual(self.editor.channel_scroll_limit(), 0)

    def test_a_label_is_written_for_each_channel(self):
        for identity in IDENTITIES:
            self.assertTrue(va.CHANNEL_LABELS[identity])
        self.assertEqual(va.CHANNEL_LABELS["normal_x"], "Normal X")


class ModeMenuTests(PanelDriver):
    def test_pressing_the_mode_of_a_row_opens_a_menu_above_it_with_all_seven_modes(self):
        button = self.editor.mode_button("smoothness")
        self.click(button.center)
        self.assertEqual(self.editor.mode_menu_open, "smoothness")
        options = self.editor.mode_option_rectangles()
        self.assertEqual(list(options), list(va.BLEND_MODES))
        for rectangle in options.values():
            self.assertLessEqual(rectangle.bottom, button.top)
            self.assertGreaterEqual(rectangle.top, 0)

    def test_choosing_an_option_sets_the_mode_and_closes_the_menu(self):
        self.choose_mode("smoothness", "add")
        self.assertEqual(self.editor.controls.mode("smoothness"), "add")
        self.assertIsNone(self.editor.mode_menu_open)

    def test_the_rgb_and_alpha_modes_have_menus_too(self):
        self.choose_mode("rgb", "multiply")
        self.choose_mode("alpha", "none")
        self.assertEqual(self.editor.controls.color_blend(), va.ColorBlend("multiply", "none"))

    def test_a_press_elsewhere_closes_the_menu_and_changes_nothing_and_does_not_draw(self):
        self.click(self.editor.mode_button("smoothness").center)
        self.click(self.position(2, 2))
        self.assertIsNone(self.editor.mode_menu_open)
        self.assertEqual(self.editor.controls.mode("smoothness"), "none")
        self.assertEqual(self.pixel((0, 0), 2, 2), BLUE, "the press that closed the menu did not draw")

    def test_a_press_on_the_atlas_closes_the_menu(self):
        self.click(self.editor.mode_button("smoothness").center)
        self.click((100, 400))
        self.assertIsNone(self.editor.mode_menu_open)

    def test_pressing_the_mode_that_opened_the_menu_closes_it(self):
        button = self.editor.mode_button("smoothness")
        self.click(button.center)
        self.click(button.center)
        self.assertIsNone(self.editor.mode_menu_open)

    def test_the_mode_of_a_row_is_written_on_its_button(self):
        self.assertEqual(self.editor.mode_button_text("smoothness"), "None")
        self.assertEqual(self.editor.mode_button_text("rgb"), "RGB: Replace")
        self.assertEqual(self.editor.mode_button_text("alpha"), "Alpha: Replace")
        self.choose_mode("smoothness", "subtract")
        self.assertEqual(self.editor.mode_button_text("smoothness"), "Subtract")


class SliderTests(PanelDriver):
    def test_pressing_the_left_end_of_a_slider_gives_the_lowest_value_and_the_right_end_the_highest(self):
        track = self.row("normal_x")["primary"]
        self.click((track.right - 1, track.centery))
        self.assertEqual(self.editor.controls.value("normal_x", "primary"), 1.0)
        self.click((track.x, track.centery))
        self.assertEqual(self.editor.controls.value("normal_x", "primary"), -1.0)

    def test_the_two_sliders_are_the_two_values(self):
        primary, secondary = self.row("smoothness")["primary"], self.row("smoothness")["secondary"]
        self.click((primary.right - 1, primary.centery))
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 1.0)
        self.assertEqual(self.editor.controls.value("smoothness", "secondary"), 0.0)
        self.click((secondary.right - 1, secondary.centery))
        self.assertEqual(self.editor.controls.value("smoothness", "secondary"), 1.0)

    def test_dragging_follows_the_pointer_and_stops_at_release(self):
        track = self.row("smoothness")["primary"]
        self.send("down", (track.x, track.centery))
        self.move((track.centerx, track.centery + 40))
        self.assertAlmostEqual(self.editor.controls.value("smoothness", "primary"), (track.centerx - track.x) / (track.width - 1), places=6)
        self.send("up", (track.centerx, track.centery))
        self.move((track.right - 1, track.centery))
        self.assertAlmostEqual(self.editor.controls.value("smoothness", "primary"), (track.centerx - track.x) / (track.width - 1), places=6)

    def test_a_press_on_a_slider_does_not_draw_on_the_canvas(self):
        track = self.row("smoothness")["primary"]
        self.click((track.centerx, track.centery))
        self.assertEqual(self.project.unsaved_tiles(), [])


class ValueFieldTests(PanelDriver):
    def focus(self, identity):
        self.click(self.row(identity)["value"].center)
        self.assertEqual(self.editor.value_focus, identity)

    def test_the_field_shows_the_value_of_the_active_colour_slot(self):
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.editor.controls.set_value("smoothness", "secondary", 0.7)
        self.assertEqual(self.editor.value_field_text("smoothness"), "0.20")
        self.click(self.editor.areas.secondary_swatch.center)
        self.assertEqual(self.editor.value_field_text("smoothness"), "0.70")

    def test_a_typed_number_replaces_the_value_when_enter_is_pressed(self):
        self.focus("smoothness")
        self.text("0.35")
        self.press_key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.35)
        self.assertIsNone(self.editor.value_focus)

    def test_the_value_goes_to_the_secondary_slot_when_that_is_active(self):
        self.click(self.editor.areas.secondary_swatch.center)
        self.focus("smoothness")
        self.text("0.6")
        self.press_key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.controls.value("smoothness", "secondary"), 0.6)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.0)

    def test_escape_gives_up_and_clicking_elsewhere_applies(self):
        self.focus("smoothness")
        self.text("0.9")
        self.press_key(va.pygame.K_ESCAPE)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.0)
        self.assertIsNone(self.editor.value_focus)
        self.focus("smoothness")
        self.text("0.9")
        self.click(self.editor.areas.primary_swatch.center)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.9)

    def test_text_that_is_not_a_value_is_dropped_and_the_old_value_stays(self):
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.focus("smoothness")
        self.text("1.2.3")
        self.press_key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.2)

    def test_the_arrow_keys_step_the_value_while_the_field_has_the_focus(self):
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.focus("smoothness")
        self.press_key(va.pygame.K_UP)
        self.press_key(va.pygame.K_UP)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.22)
        self.press_key(va.pygame.K_DOWN, va.pygame.KMOD_SHIFT)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.12)
        self.assertEqual(self.editor.value_field.text, "0.12")

    def test_a_metal_can_be_typed_for_reflectance(self):
        self.focus("reflectance")
        self.text("m5")
        self.press_key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.controls.value("reflectance", "primary"), va.Metal(5))
        self.assertEqual(self.editor.value_field_text("reflectance"), "m5")

    def test_a_number_past_the_range_is_kept_in_it(self):
        self.focus("smoothness")
        self.text("7")
        self.press_key(va.pygame.K_RETURN)
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 1.0)

    def test_keys_typed_in_the_field_do_not_reach_the_editor(self):
        self.focus("smoothness")
        self.editor.colors.set_color((1, 2, 3, 255), "primary")
        self.application.handle_event(
            va.pygame.event.Event(va.pygame.KEYDOWN, key=va.pygame.K_x, mod=0, unicode="x", scancode=0)
        )
        self.assertEqual(self.editor.colors.get("primary"), (1, 2, 3, 255), "x did not swap the colours")

    def test_starting_a_stroke_applies_what_was_typed(self):
        self.editor.controls.set_mode("smoothness", "replace")
        self.focus("smoothness")
        self.text("0.2")
        self.click_tool("pencil")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(int(self.project.tiles[(0, 0)].planes[2, 2, SMOOTHNESS]), 51)


class ScrollTests(PanelDriver):
    window_size = (900, 520)

    def test_when_the_rows_do_not_fit_the_wheel_scrolls_them_and_stops_at_the_ends(self):
        limit = self.editor.channel_scroll_limit()
        self.assertGreater(limit, 0)
        before = self.row(IDENTITIES[0])
        panel = self.editor.areas.channel_panel
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=panel.center, rel=(0, 0), buttons=(0, 0, 0)))
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=-1))
        self.assertGreater(self.editor.channel_scroll, 0)
        after = self.row(IDENTITIES[0])
        self.assertTrue(after is None or after["mode"].top < before["mode"].top)
        for _ in range(30):
            self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=-1))
        self.assertEqual(self.editor.channel_scroll, limit)
        for _ in range(60):
            self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        self.assertEqual(self.editor.channel_scroll, 0)

    def test_a_row_that_is_scrolled_out_has_no_areas_and_the_last_one_can_be_reached(self):
        self.assertIsNotNone(self.row(IDENTITIES[0]))
        self.assertIsNone(self.row(IDENTITIES[-1]))
        self.editor.channel_scroll = self.editor.channel_scroll_limit()
        self.assertIsNone(self.row(IDENTITIES[0]))
        self.assertIsNotNone(self.row(IDENTITIES[-1]))
        panel = self.editor.areas.channel_panel
        self.assertTrue(panel.contains(self.row(IDENTITIES[-1])["mode"]))

    def test_the_wheel_over_the_canvas_still_zooms_it(self):
        zoom = self.editor.view.zoom
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=self.editor.areas.canvas.center, rel=(0, 0), buttons=(0, 0, 0)))
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        self.assertNotEqual(self.editor.view.zoom, zoom)


class EnabledTests(PanelDriver):
    def test_the_rows_are_for_the_tools_that_paint(self):
        for tool in ("pencil", "line", "noise_brush", "airbrush", "fill"):
            self.click_tool(tool)
            self.assertTrue(self.editor.channels_enabled(), tool)
        for tool in ("eraser", "rotate", "select"):
            self.click_tool(tool)
            self.assertFalse(self.editor.channels_enabled(), tool)

    def test_the_eye_dropper_keeps_the_rows_since_it_loads_the_values(self):
        self.click_tool("eyedropper")
        self.assertTrue(self.editor.channels_enabled())

    def test_a_press_on_a_greyed_row_does_nothing(self):
        self.click_tool("eraser")
        self.click(self.editor.mode_button("smoothness").center)
        self.assertIsNone(self.editor.mode_menu_open)
        track = self.row("smoothness")["primary"]
        self.click((track.right - 1, track.centery))
        self.assertEqual(self.editor.controls.value("smoothness", "primary"), 0.0)


class NoneProfileTests(PanelDriver):
    profile = "none"

    def test_there_are_no_channel_rows_but_the_colour_modes_work(self):
        self.assertEqual(self.editor.controls.rows, [])
        self.assertIsNone(self.row("smoothness"))
        self.choose_mode("rgb", "add")
        self.assertEqual(self.editor.controls.mode("rgb"), "add")

    def test_a_press_in_the_empty_panel_does_nothing(self):
        self.click(self.editor.areas.channel_panel.center)
        self.assertIsNone(self.editor.mode_menu_open)
        self.assertIsNone(self.editor.value_focus)


class DrawTests(unittest.TestCase):
    def test_the_window_draws_with_and_without_channels_at_several_sizes(self):
        import tempfile

        va.pygame.init()
        for profile in ("none", LABPBR):
            for size in ((1280, 800), (900, 520), (1600, 900)):
                with tempfile.TemporaryDirectory() as name:
                    folder = Path(name)
                    va.write_png_atomically(folder / "a.png", solid_tile(BLUE, 8))
                    va.write_atlas_file(folder / "atlas.json", va.AtlasFile(8, {(0, 0): "a.png"}, {}, {}, profile))
                    project = va.AtlasProject.open(folder)
                    project.selection = {(0, 0)}
                    surface = va.pygame.Surface(size)
                    application = va.Application(project, surface)
                    application.editor.sync()
                    application.editor.controls.set_mode("rgb", "add")
                    application.draw()
                    if profile == LABPBR:
                        application.editor.mode_menu_open = "smoothness"
                        application.editor.controls.set_mode("smoothness", "add")
                        application.draw()
                        application.editor.value_focus = "smoothness"
                        application.draw()
                    self.assertEqual(surface.get_size(), size)


if __name__ == "__main__":
    unittest.main()
