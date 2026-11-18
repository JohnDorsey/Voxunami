"""Tests for the channel breakout (the "Ch" button): the canvas shown as a grid of panels, one for RGB, one for alpha and one for each
channel, in grey where they are not colour; strokes bound to the panel they began in; zoom and pan shared by all panels; and the read-out
of the values under the pointer.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import EMISSION, LABPBR, SMOOTHNESS, neutral_planes, solid_tile  # noqa: E402
from test_pbr_panel import PanelDriver  # noqa: E402
from test_virtual_atlas import BLUE, RED, va  # noqa: E402

PROFILE = va.PBR_PROFILES[LABPBR]


class CellTests(unittest.TestCase):
    AREA = va.pygame.Rect(100, 50, 600, 400)

    def cells(self, count, area=None, canvas=(16, 16), copies=1):
        return va.compute_breakout_cells(area or self.AREA, count, canvas, copies)

    def test_there_is_a_label_and_a_cell_for_each_panel_and_the_label_is_right_above_its_cell(self):
        cells = self.cells(10)
        self.assertEqual(len(cells), 10)
        for label, cell in cells:
            self.assertEqual(label.bottom, cell.top)
            self.assertEqual((label.left, label.width), (cell.left, cell.width))

    def test_everything_is_inside_the_area_and_nothing_overlaps(self):
        for count in (1, 2, 3, 5, 10):
            for area in (self.AREA, va.pygame.Rect(0, 0, 300, 700), va.pygame.Rect(0, 0, 900, 200)):
                rectangles = [rectangle for pair in self.cells(count, area) for rectangle in pair]
                for rectangle in rectangles:
                    self.assertTrue(area.contains(rectangle), (count, area, rectangle))
                for index, first in enumerate(rectangles):
                    for second in rectangles[index + 1 :]:
                        self.assertFalse(first.colliderect(second), (count, area))

    def test_the_cells_are_of_one_size_and_in_reading_order(self):
        cells = [cell for _, cell in self.cells(7)]
        self.assertEqual(len({cell.size for cell in cells}), 1)
        positions = [(cell.y, cell.x) for cell in cells]
        self.assertEqual(positions, sorted(positions))

    def test_one_panel_has_the_whole_area_and_two_panels_in_a_wide_area_stand_side_by_side(self):
        (label, cell), = self.cells(1)
        self.assertGreater(cell.width, self.AREA.width - va.scaled(20))
        wide = va.pygame.Rect(0, 0, 800, 200)
        first, second = (cell for _, cell in self.cells(2, wide))
        self.assertEqual(first.y, second.y)
        self.assertLess(first.x, second.x)

    def test_the_layout_that_lets_the_canvas_be_biggest_is_chosen(self):
        tall = va.pygame.Rect(0, 0, 200, 800)
        cells = [cell for _, cell in self.cells(4, tall)]
        self.assertEqual(len({cell.x for cell in cells}), 1, "a tall narrow area holds four panels in a column")
        wide = va.pygame.Rect(0, 0, 800, 120)
        cells = [cell for _, cell in self.cells(4, wide)]
        self.assertEqual(len({cell.y for cell in cells}), 1, "a wide low area holds them in a row")

    def test_wrap_assist_asks_for_room_for_several_copies(self):
        one = self.cells(4, copies=1)
        three = self.cells(4, copies=3)
        self.assertEqual(len(one), len(three))


class PanelImageTests(unittest.TestCase):
    def pixels(self):
        pixels = solid_tile((10, 20, 30, 40), 4)
        return pixels

    def image(self, key, planes=None):
        return va.panel_image(key, self.pixels(), planes, PROFILE)

    def test_the_rgb_panel_shows_the_colour_and_ignores_alpha(self):
        self.assertEqual(self.image("rgb")[0, 0].tolist(), [10, 20, 30, 255])

    def test_the_alpha_panel_is_grey_levels_of_alpha(self):
        self.assertEqual(self.image("alpha")[0, 0].tolist(), [40, 40, 40, 255])

    def test_a_channel_panel_is_the_grey_of_its_meaning(self):
        planes = neutral_planes(4)
        planes[1, 2, SMOOTHNESS] = 51
        planes[1, 2, EMISSION] = 50
        self.assertEqual(self.image("smoothness", planes)[1, 2].tolist(), [51, 51, 51, 255])
        self.assertEqual(self.image("smoothness", planes)[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(self.image("emission", planes)[1, 2].tolist(), [51, 51, 51, 255])
        self.assertEqual(self.image("emission", planes)[0, 0].tolist(), [0, 0, 0, 255], "no emission is black")
        self.assertEqual(self.image("normal_x", planes)[0, 0].tolist(), [128, 128, 128, 255], "no tilt is the middle grey")
        self.assertEqual(self.image("ambient_occlusion", planes)[0, 0].tolist(), [0, 0, 0, 255], "no occlusion is black")
        self.assertEqual(self.image("height", planes)[0, 0].tolist(), [255, 255, 255, 255], "the top is white")

    def test_an_unknown_panel_is_an_error(self):
        with self.assertRaises(ValueError):
            self.image("colour")


class BreakoutDriver(PanelDriver):
    def toggle(self):
        button = self.editor.areas.sidebar_buttons["breakout"]
        self.send("down", button.center)
        self.send("up", button.center)

    def cell_position(self, index, column, row):
        """The screen position of the middle of a canvas pixel in a panel."""
        return self.editor.panel_views[index].pixel_rectangle(column, row).center

    def hover(self, position):
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEMOTION, pos=position, rel=(0, 0), buttons=(0, 0, 0)))


class BreakoutTests(BreakoutDriver):
    def test_the_button_turns_the_breakout_on_and_off(self):
        self.assertFalse(self.editor.breakout)
        self.toggle()
        self.assertTrue(self.editor.breakout)
        self.assertTrue(self.editor.sidebar_button_checked("breakout"))
        self.toggle()
        self.assertFalse(self.editor.breakout)

    def test_the_panels_are_rgb_alpha_and_the_channels_of_the_setting(self):
        self.toggle()
        self.assertEqual(
            [label for _, label in self.editor.breakout_panels()],
            ["RGB", "Alpha", "Smoothness", "Reflectance", "Porosity or scattering", "Emission", "Normal X", "Normal Y",
             "Occlusion", "Height"],
        )
        self.assertEqual([key for key, _ in self.editor.breakout_panels()][:3], ["rgb", "alpha", "smoothness"])

    def test_the_cells_are_in_the_canvas_area_and_show_the_whole_canvas(self):
        self.toggle()
        area = self.editor.areas.canvas
        self.assertEqual(len(self.editor.panel_views), 10)
        for index, view in enumerate(self.editor.panel_views):
            cell = self.editor.panel_cells[index][1]
            self.assertTrue(area.contains(cell))
            self.assertEqual(view.rect, cell)
            self.assertTrue(cell.contains(view.pixel_rectangle(0, 0, 8, 8)), index)

    def test_every_panel_shows_the_canvas_at_the_same_zoom(self):
        self.toggle()
        self.assertEqual(len({view.zoom for view in self.editor.panel_views}), 1)

    def test_a_stroke_in_any_panel_paints_the_canvas_pixel_under_it(self):
        self.toggle()
        self.click_tool("pencil")
        self.editor.colors.set_color(RED, "primary")
        for index, pixel in ((0, (1, 1)), (3, (2, 2)), (9, (3, 3))):
            self.send("down", self.cell_position(index, *pixel))
            self.send("up", self.cell_position(index, *pixel))
            self.assertEqual(self.pixel((0, 0), *pixel), RED, index)

    def test_a_stroke_goes_on_in_the_panel_it_began_in_when_the_pointer_leaves_it(self):
        self.toggle()
        self.click_tool("pencil")
        self.editor.colors.set_color(RED, "primary")
        away = self.cell_position(2, 5, 5)
        self.send("down", self.cell_position(0, 1, 1))
        self.assertEqual(self.editor.bound_panel, 0)
        self.move(away)
        expected_pixel = self.editor.panel_views[0].pixel_at(away)
        self.send("up", away)
        self.assertIsNone(self.editor.bound_panel)
        reference = va.CanvasSession(
            va.compose_canvas({(0, 0): solid_tile(BLUE, 8)}, 8), "pencil", va.ToolSettings(), RED, (1, 1)
        )
        reference.move(expected_pixel)
        painted = {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(self.project.tiles[(0, 0)].pixels != solid_tile(BLUE, 8), axis=2))}
        expected = {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(reference.pixels != reference.base, axis=2))}
        self.assertEqual(painted, expected)
        self.assertNotEqual(expected_pixel, self.editor.panel_views[2].pixel_at(away), "the panel's own mapping would differ")

    def test_a_stroke_paints_the_channels_chosen_wherever_it_is_drawn(self):
        self.editor.controls.set_mode("smoothness", "replace")
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.toggle()
        self.click_tool("pencil")
        self.send("down", self.cell_position(4, 2, 2))
        self.send("up", self.cell_position(4, 2, 2))
        self.assertEqual(int(self.project.tiles[(0, 0)].planes[2, 2, SMOOTHNESS]), 51)

    def test_the_wheel_over_a_panel_zooms_every_panel_and_keeps_them_alike(self):
        self.toggle()
        before = self.editor.panel_views[0].zoom
        index = 4
        position = self.cell_position(index, 4, 4)
        self.hover(position)
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        zooms = {view.zoom for view in self.editor.panel_views}
        self.assertEqual(zooms, {before + 1})
        offsets = {(view.origin_x - cell.x, view.origin_y - cell.y) for view, (_, cell) in zip(self.editor.panel_views, self.editor.panel_cells)}
        self.assertEqual(len(offsets), 1, "the panels show the same part of the canvas")

    def test_the_pixel_under_the_pointer_stays_under_it_when_zooming(self):
        self.toggle()
        position = self.cell_position(4, 5, 5)
        self.hover(position)
        before = self.editor.panel_views[4].pixel_at(position)
        self.application.handle_event(va.pygame.event.Event(va.pygame.MOUSEWHEEL, x=0, y=1))
        self.assertEqual(self.editor.panel_views[4].pixel_at(position), before)

    def test_the_middle_button_pans_every_panel_together(self):
        self.toggle()
        start = self.cell_position(1, 4, 4)
        before = [(view.origin_x, view.origin_y) for view in self.editor.panel_views]
        self.send("down", start, 2)
        self.move((start[0] + 7, start[1] + 5))
        during = [(view.origin_x, view.origin_y) for view in self.editor.panel_views]
        self.assertEqual([(x - 7, y - 5) for x, y in during], before, "they move while the button is down, not only when it is let go")
        self.send("up", (start[0] + 7, start[1] + 5), 2)
        after = [(view.origin_x, view.origin_y) for view in self.editor.panel_views]
        self.assertEqual([(x - 7, y - 5) for x, y in after], before)

    def test_the_brush_guide_is_in_every_panel(self):
        self.toggle()
        self.click_tool("pencil")
        self.hover(self.cell_position(2, 3, 3))
        self.application.draw()
        surface = self.application.surface
        for index, view in enumerate(self.editor.panel_views):
            rectangle = view.pixel_rectangle(3, 3)
            self.assertEqual(tuple(surface.get_at((rectangle.x, rectangle.centery)))[:3], (255, 255, 255), index)

    def test_each_panel_is_drawn_as_what_it_stands_for(self):
        self.project.tiles[(0, 0)].planes[2, 3, SMOOTHNESS] = 255
        self.project.tiles[(0, 0)].pixels[2, 3] = (10, 200, 30, 90)
        self.project.tiles[(0, 0)].revision += 1
        self.toggle()
        self.application.draw()
        surface = self.application.surface
        keys = [key for key, _ in self.editor.breakout_panels()]

        def colour(key):
            view = self.editor.panel_views[keys.index(key)]
            rectangle = view.pixel_rectangle(3, 2)
            return tuple(surface.get_at((rectangle.centerx + 1, rectangle.centery + 1)))[:3]

        self.assertEqual(colour("rgb"), (10, 200, 30))
        self.assertEqual(colour("alpha"), (90, 90, 90))
        self.assertEqual(colour("smoothness"), (255, 255, 255))
        self.assertEqual(colour("height"), (255, 255, 255))
        self.assertEqual(colour("emission"), (0, 0, 0))

    def test_with_wrap_assist_each_panel_repeats_the_canvas_and_a_stroke_comes_back_in(self):
        self.toggle()
        self.send("down", self.editor.areas.sidebar_buttons["wrap"].center)
        self.send("up", self.editor.areas.sidebar_buttons["wrap"].center)
        self.assertTrue(self.editor.wrap_assist)
        self.assertTrue(self.editor.breakout)
        self.click_tool("pencil")
        self.editor.colors.set_color(RED, "primary")
        # From the pixel 6 to the pixel 9 of the repeating canvas: the stroke leaves the right side and comes back in at the left.
        self.send("down", self.cell_position(3, 6, 2))
        self.move(self.cell_position(3, 9, 2))
        self.send("up", self.cell_position(3, 9, 2))
        self.assertEqual([self.pixel((0, 0), column, 2) for column in range(8)], [RED, RED, BLUE, BLUE, BLUE, BLUE, RED, RED])

    def test_turning_the_breakout_off_gives_back_the_one_canvas_and_the_strokes_on_it(self):
        self.toggle()
        self.toggle()
        self.click_tool("pencil")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(self.pixel((0, 0), 2, 2), RED)

    def test_the_select_tool_works_in_a_panel_and_its_rectangle_is_in_every_panel(self):
        self.toggle()
        self.click_tool("select")
        self.send("down", self.cell_position(3, 1, 1))
        self.move(self.cell_position(3, 3, 3))
        self.send("up", self.cell_position(3, 3, 3))
        self.assertEqual(self.editor.selection_rectangle, (1, 1, 4, 4))
        self.application.draw()

    def test_the_cells_follow_the_size_of_the_editor(self):
        self.toggle()
        before = [cell for _, cell in self.editor.panel_cells]
        self.editor.set_rect(va.pygame.Rect(0, 0, 700, 560))
        after = [cell for _, cell in self.editor.panel_cells]
        self.assertNotEqual(before, after)
        area = self.editor.areas.canvas
        self.assertTrue(all(area.contains(cell) for cell in after))


class BreakoutWithoutChannelsTests(BreakoutDriver):
    profile = "none"

    def test_there_are_two_panels_and_strokes_work(self):
        self.toggle()
        self.assertEqual([label for _, label in self.editor.breakout_panels()], ["RGB", "Alpha"])
        self.click_tool("pencil")
        self.editor.colors.set_color(RED, "primary")
        self.send("down", self.cell_position(1, 2, 2))
        self.send("up", self.cell_position(1, 2, 2))
        self.assertEqual(self.pixel((0, 0), 2, 2), RED)
        self.application.draw()


class HoverTests(BreakoutDriver):
    def test_the_read_out_gives_the_pixel_the_colour_and_every_channel_decoded(self):
        record = self.project.tiles[(0, 0)]
        record.pixels[2, 3] = (10, 20, 30, 40)
        record.planes[2, 3, SMOOTHNESS] = 51
        record.revision += 1
        self.select()
        self.select((0, 0))
        self.hover(self.position(3, 2))
        text = self.editor.hover_text()
        self.assertTrue(text.startswith("(3, 2)"), text)
        self.assertIn("#0a141e28", text)
        self.assertIn("Smooth 0.20", text)
        self.assertIn("Emission 0.00", text)
        self.assertIn("Height 1.00", text)

    def test_the_read_out_is_there_in_a_panel_too_and_gives_the_canvas_pixel(self):
        self.toggle()
        self.hover(self.cell_position(5, 4, 1))
        self.assertTrue(self.editor.hover_text().startswith("(4, 1)"))

    def test_a_metal_is_read_as_its_number(self):
        record = self.project.tiles[(0, 0)]
        record.planes[0, 0, 1] = 235
        record.revision += 1
        self.select()
        self.select((0, 0))
        self.hover(self.position(0, 0))
        self.assertIn("Reflect m5", self.editor.hover_text())

    def test_off_the_canvas_there_is_nothing_to_read(self):
        self.hover((2, 2))
        self.assertIsNone(self.editor.hover_text())
        self.hover(self.editor.view.pixel_rectangle(-3, -3).center)
        self.assertIsNone(self.editor.hover_text())

    def test_with_wrap_assist_a_pixel_of_any_copy_gives_the_canvas_pixel(self):
        self.send("down", self.editor.areas.sidebar_buttons["wrap"].center)
        self.send("up", self.editor.areas.sidebar_buttons["wrap"].center)
        self.hover(self.editor.view.pixel_rectangle(8 + 3, 8 + 2).center)
        self.assertTrue(self.editor.hover_text().startswith("(3, 2)"))

    def test_the_text_is_drawn_without_trouble(self):
        self.hover(self.position(3, 2))
        self.application.draw()


class BreakoutNoneProfileHoverTests(BreakoutDriver):
    profile = "none"

    def test_under_none_the_read_out_is_the_colour_alone(self):
        self.hover(self.position(1, 1))
        text = self.editor.hover_text()
        self.assertTrue(text.startswith("(1, 1)"), text)
        self.assertNotIn("Smooth", text)
        self.assertIn("#", text)


class DrawSmokeTests(unittest.TestCase):
    def test_the_window_draws_with_the_breakout_at_several_sizes_and_settings(self):
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
                    application = va.Application(project, va.pygame.Surface(size))
                    editor = application.editor
                    editor.sync()
                    editor.toggle_sidebar_button("breakout")
                    application.draw()
                    editor.toggle_sidebar_button("wrap")
                    application.draw()
                    editor.toggle_sidebar_button("breakout")
                    application.draw()


if __name__ == "__main__":
    unittest.main()
