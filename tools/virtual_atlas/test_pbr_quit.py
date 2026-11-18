"""Tests for the dialog that asks what to do with a tile that has unsaved changes: a row for what differs (RGB together, alpha, then each
channel, named), and for each row the picture on disk (old), the picture now (new) and where they differ (bright red on black).

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import EMISSION, LABPBR, NORMAL_X, SMOOTHNESS, neutral_planes, solid_tile  # noqa: E402
from test_pbr_panel import PanelDriver  # noqa: E402
from test_virtual_atlas import BLUE, RED, va  # noqa: E402

PROFILE = va.PBR_PROFILES[LABPBR]
RED_ON_BLACK = ((255, 0, 0, 255), (0, 0, 0, 255))


def record_with(pixels, planes, disk_pixels, disk_planes):
    return va.TileRecord("a.png", pixels, disk_pixels, planes=planes, disk_planes=disk_planes)


def grey(value):
    return [value, value, value, 255]


class DifferenceRowTests(unittest.TestCase):
    def rows(self, record, profile=PROFILE):
        return va.tile_difference_rows(profile, record)

    def labels(self, record, profile=PROFILE):
        return [row.label for row in self.rows(record, profile)]

    def test_a_channel_that_changed_is_a_row_named_after_it_with_the_pictures_in_grey(self):
        planes = neutral_planes(4)
        planes[3, 2, SMOOTHNESS] = 51
        record = record_with(solid_tile(BLUE), planes, solid_tile(BLUE), neutral_planes(4))
        rows = self.rows(record)
        self.assertEqual([row.label for row in rows], ["Smoothness"])
        row = rows[0]
        self.assertEqual(row.old[3, 2].tolist(), grey(0))
        self.assertEqual(row.new[3, 2].tolist(), grey(51))
        self.assertEqual(row.new[0, 0].tolist(), grey(0))

    def test_the_difference_is_bright_red_where_the_row_differs_and_black_elsewhere(self):
        planes = neutral_planes(4)
        planes[3, 2, SMOOTHNESS] = 51
        row = self.rows(record_with(solid_tile(BLUE), planes, solid_tile(BLUE), neutral_planes(4)))[0]
        self.assertEqual(row.difference[3, 2].tolist(), [255, 0, 0, 255])
        self.assertEqual(row.difference[0, 0].tolist(), [0, 0, 0, 255])
        self.assertEqual(int((row.difference[..., 0] == 255).sum()), 1)

    def test_the_colour_is_one_row_for_red_green_and_blue_together_and_shows_no_transparency(self):
        pixels = solid_tile(BLUE)
        pixels[1, 1] = (0, 255, 0, 40)
        record = record_with(pixels, neutral_planes(4), solid_tile(BLUE), neutral_planes(4))
        rows = self.rows(record)
        self.assertEqual([row.label for row in rows], ["RGB", "Alpha"])
        self.assertEqual(rows[0].new[1, 1].tolist(), [0, 255, 0, 255], "alpha is not part of this row")
        self.assertEqual(rows[0].old[1, 1].tolist(), [0, 0, 255, 255])

    def test_alpha_is_a_row_of_its_own_in_grey(self):
        pixels = solid_tile(BLUE)
        pixels[1, 1, 3] = 40
        record = record_with(pixels, neutral_planes(4), solid_tile(BLUE), neutral_planes(4))
        rows = self.rows(record)
        self.assertEqual([row.label for row in rows], ["Alpha"])
        self.assertEqual(rows[0].new[1, 1].tolist(), grey(40))
        self.assertEqual(rows[0].old[1, 1].tolist(), grey(255))

    def test_the_rows_are_rgb_then_alpha_then_the_channels_in_the_order_of_the_profile(self):
        pixels = solid_tile(BLUE)
        pixels[0, 0] = (1, 2, 3, 4)
        planes = neutral_planes(4)
        planes[0, 0, NORMAL_X] = 200
        planes[0, 0, SMOOTHNESS] = 9
        planes[0, 0, EMISSION] = 9
        record = record_with(pixels, planes, solid_tile(BLUE), neutral_planes(4))
        self.assertEqual(self.labels(record), ["RGB", "Alpha", "Smoothness", "Emission", "Normal X"])

    def test_a_row_that_did_not_change_is_not_shown(self):
        planes = neutral_planes(4)
        planes[0, 0, SMOOTHNESS] = 9
        record = record_with(solid_tile(BLUE), planes, solid_tile(BLUE), neutral_planes(4))
        self.assertNotIn("RGB", self.labels(record))
        self.assertNotIn("Alpha", self.labels(record))
        self.assertNotIn("Emission", self.labels(record))

    def test_a_tile_with_no_file_has_its_colour_row_and_an_empty_old_picture(self):
        record = record_with(solid_tile(BLUE), neutral_planes(4), None, neutral_planes(4))
        rows = self.rows(record)
        self.assertEqual(rows[0].label, "RGB")
        self.assertTrue((rows[0].old[..., 3] == 0).all(), "nothing is on disk: the old picture is empty")
        self.assertTrue((rows[0].difference[..., 0] == 255).all())

    def test_a_tile_with_no_file_whose_colour_is_clear_still_has_its_colour_row(self):
        record = record_with(numpy.zeros((4, 4, 4), dtype=numpy.uint8), neutral_planes(4), None, neutral_planes(4))
        self.assertEqual(self.labels(record), ["RGB"])

    def test_without_channels_there_are_only_the_colour_rows(self):
        none = va.PBR_PROFILES["none"]
        pixels = solid_tile(BLUE)
        pixels[0, 0] = (1, 2, 3, 4)
        record = record_with(pixels, None, solid_tile(BLUE), None)
        self.assertEqual(self.labels(record, none), ["RGB", "Alpha"])

    def test_a_grey_is_the_meaning_of_the_channel_from_its_lowest_to_its_highest(self):
        planes = neutral_planes(4)
        planes[0, 0, EMISSION] = 50
        planes[0, 0, NORMAL_X] = 255
        record = record_with(solid_tile(BLUE), planes, solid_tile(BLUE), neutral_planes(4))
        rows = {row.label: row for row in self.rows(record)}
        self.assertEqual(rows["Emission"].old[0, 0].tolist(), grey(0))
        self.assertEqual(rows["Emission"].new[0, 0].tolist(), grey(51))
        self.assertEqual(rows["Normal X"].old[0, 0].tolist(), grey(128), "no tilt is the middle grey")
        self.assertEqual(rows["Normal X"].new[0, 0].tolist(), grey(255))


class QuitAreasTests(unittest.TestCase):
    def test_every_row_has_its_label_and_three_pictures_of_one_size_in_a_line(self):
        areas = va.quit_dialog_areas((1280, 800), True, 4)
        for index in range(4):
            old, new, difference = (areas[f"{name}{index}"] for name in ("old", "new", "difference"))
            self.assertEqual(old.size, new.size)
            self.assertEqual(new.size, difference.size)
            self.assertEqual((old.y, new.y, difference.y), (old.y,) * 3)
            self.assertLess(areas[f"label{index}"].right, old.left + 1)
            self.assertLessEqual(old.right, new.left)
            self.assertLessEqual(new.right, difference.left)
            if index:
                self.assertGreaterEqual(old.top, areas[f"old{index - 1}"].bottom)

    def test_the_parts_fit_the_panel_and_the_window_and_do_not_overlap_for_any_number_of_rows(self):
        for size in [(1280, 800), (900, 600), (640, 480)]:
            for rows in (1, 3, 10):
                for checkbox in (True, False):
                    areas = va.quit_dialog_areas(size, checkbox, rows)
                    window = va.pygame.Rect(0, 0, *size)
                    self.assertTrue(window.contains(areas["panel"]), (size, rows))
                    parts = {name: rectangle for name, rectangle in areas.items() if name != "panel"}
                    for name, rectangle in parts.items():
                        self.assertTrue(areas["panel"].contains(rectangle), (name, size, rows))
                    names = list(parts)
                    for index, first in enumerate(names):
                        for second in names[index + 1 :]:
                            self.assertFalse(parts[first].colliderect(parts[second]), (first, second, size, rows))

    def test_rows_that_cannot_fit_are_left_out_and_a_line_says_so(self):
        areas = va.quit_dialog_areas((640, 480), True, 10)
        self.assertIn("more", areas)
        self.assertIn("old0", areas)
        self.assertNotIn("old9", areas)
        shown = sum(1 for name in areas if name.startswith("old") and name[3:].isdigit())
        self.assertGreaterEqual(shown, 1)
        self.assertLess(shown, 10)
        self.assertNotIn("more", va.quit_dialog_areas((1280, 800), True, 10))
        self.assertIn("old9", va.quit_dialog_areas((1280, 800), True, 10))

    def test_more_rows_make_smaller_pictures_but_never_tiny_ones_at_the_usual_size(self):
        one = va.quit_dialog_areas((1280, 800), False, 1)["old0"].height
        ten = va.quit_dialog_areas((1280, 800), False, 10)["old0"].height
        self.assertGreater(one, ten)
        self.assertGreaterEqual(ten, va.scaled(30))


class QuitWindowTests(PanelDriver):
    def quit_event(self):
        return self.application.handle_event(va.pygame.event.Event(va.pygame.QUIT))

    def test_the_dialog_has_a_row_for_each_thing_that_changed(self):
        self.project.tiles[(0, 0)].planes[2, 3, SMOOTHNESS] = 51
        self.project.tiles[(0, 0)].pixels[0, 0] = (1, 2, 3, 255)
        self.quit_event()
        dialog = self.application.quit_dialog
        self.assertEqual([row.label for row in dialog.rows], ["RGB", "Smoothness"])
        areas = self.application.dialog_areas()
        self.assertIn("difference1", areas)
        self.assertNotIn("difference2", areas)

    def test_the_pictures_are_drawn_old_then_new_then_the_difference(self):
        self.project.tiles[(0, 0)].planes[2, 3, SMOOTHNESS] = 255
        self.quit_event()
        self.application.draw()
        areas = self.application.dialog_areas()
        surface = self.application.surface
        for name, expected in (("old0", (0, 0, 0)), ("new0", (255, 255, 255)), ("difference0", (255, 0, 0))):
            rectangle = va.fit_picture_rectangle(areas[name], 8)
            cell = va.pygame.Rect(rectangle.x + 3 * (rectangle.width // 8), rectangle.y + 2 * (rectangle.height // 8), 4, 4)
            self.assertEqual(tuple(surface.get_at(cell.center))[:3], expected, name)
        rectangle = va.fit_picture_rectangle(areas["difference0"], 8)
        self.assertEqual(tuple(surface.get_at((rectangle.x + 2, rectangle.y + 2)))[:3], (0, 0, 0))

    def test_the_dialog_draws_for_many_rows_and_a_new_tile_in_a_small_window(self):
        self.project.tiles[(0, 0)].pixels[0, 0] = (1, 2, 3, 4)
        for index in range(8):
            self.project.tiles[(0, 0)].planes[index, 0, index] = 7
        application = va.Application(self.project, va.pygame.Surface((700, 520)))
        application.handle_event(va.pygame.event.Event(va.pygame.QUIT))
        self.assertEqual(len(application.quit_dialog.rows), 10)
        application.draw()
        record = self.project.tiles[(0, 0)]
        record.disk_pixels = None
        record.disk_planes = neutral_planes(8)
        application.quit_dialog = va.QuitDialog(self.project)
        application.draw()

    def test_a_tile_whose_only_change_is_in_a_channel_is_asked_about_and_saving_writes_the_companion(self):
        self.project.tiles[(0, 0)].planes[2, 3, SMOOTHNESS] = 51
        self.assertTrue(self.quit_event())
        self.assertIsNotNone(self.application.quit_dialog)
        self.assertEqual(self.application.quit_dialog.save(), "quit")
        self.assertEqual(va.read_image_pixels(self.folder / "a_s.png")[2, 3, 0], 51)


if __name__ == "__main__":
    unittest.main()
