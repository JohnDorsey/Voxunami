"""Tests for the channels of a tile when pixels are selected, copied, cut, deleted and pasted. No window for the first
classes; the last one uses the editor with mouse and key events.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import LABPBR, SMOOTHNESS, EMISSION, neutral_planes, solid_tile  # noqa: E402
from test_virtual_atlas import BLUE, GREEN, RED, EditorDriver, va  # noqa: E402

PROFILE = va.PBR_PROFILES[LABPBR]


def source_of(size, smoothness, alpha):
    """Planes (smoothness set) and pixels (alpha set) of a size x size picture."""
    planes = neutral_planes(size)
    planes[:, :, SMOOTHNESS] = smoothness
    return planes, solid_tile((1, 2, 3, alpha), size)


class PastePlanesTests(unittest.TestCase):
    def test_the_channels_of_the_pasted_picture_replace_those_under_it(self):
        planes, pixels = source_of(2, 9, 255)
        destination = neutral_planes(4)
        destination[:, :, SMOOTHNESS] = 77
        result = va.paste_planes(destination, planes, 1, 1, pixels, PROFILE)
        expected = [[77] * 4, [77, 9, 9, 77], [77, 9, 9, 77], [77] * 4]
        self.assertEqual(result[:, :, SMOOTHNESS].tolist(), expected)

    def test_a_pixel_that_is_transparent_and_has_no_channel_data_leaves_what_is_under_it(self):
        planes, pixels = source_of(2, 0, 0)
        destination = neutral_planes(4)
        destination[:, :, SMOOTHNESS] = 77
        self.assertTrue(numpy.array_equal(va.paste_planes(destination, planes, 1, 1, pixels, PROFILE), destination))

    def test_a_transparent_pixel_with_channel_data_still_brings_its_channels(self):
        planes, pixels = source_of(2, 9, 0)
        destination = neutral_planes(4)
        result = va.paste_planes(destination, planes, 0, 0, pixels, PROFILE)
        self.assertEqual(int(result[1, 1, SMOOTHNESS]), 9)

    def test_the_part_off_the_canvas_is_dropped(self):
        planes, pixels = source_of(3, 9, 255)
        result = va.paste_planes(neutral_planes(4), planes, 2, -1, pixels, PROFILE)
        self.assertEqual(result[:, :, SMOOTHNESS].tolist(), [[0, 0, 9, 9], [0, 0, 9, 9], [0] * 4, [0] * 4])
        self.assertEqual(va.paste_planes(neutral_planes(4), planes, 9, 9, pixels, PROFILE).shape, (4, 4, 8))

    def test_cells_that_are_not_editable_keep_their_channels(self):
        planes, pixels = source_of(2, 9, 255)
        editable = numpy.ones((4, 4), dtype=bool)
        editable[1, 1] = False
        result = va.paste_planes(neutral_planes(4), planes, 1, 1, pixels, PROFILE, editable)
        self.assertEqual(result[1:3, 1:3, SMOOTHNESS].tolist(), [[0, 9], [9, 9]])

    def test_the_destination_is_not_changed(self):
        planes, pixels = source_of(2, 9, 255)
        destination = neutral_planes(4)
        va.paste_planes(destination, planes, 0, 0, pixels, PROFILE)
        self.assertTrue(numpy.array_equal(destination, neutral_planes(4)))


class ClearPlanesTests(unittest.TestCase):
    def test_a_rectangle_goes_back_to_neutral_and_the_rest_stays(self):
        planes = neutral_planes(4)
        planes[:, :, SMOOTHNESS] = 50
        planes[:, :, EMISSION] = 7
        result = va.clear_planes(PROFILE, planes, (1, 1, 3, 2))
        self.assertEqual(result[:, :, SMOOTHNESS].tolist(), [[50] * 4, [50, 0, 0, 50], [50] * 4, [50] * 4])
        self.assertEqual(result[1, 1, EMISSION], 255)
        self.assertEqual(result[0, 0, EMISSION], 7)

    def test_cells_that_are_not_editable_are_kept(self):
        planes = neutral_planes(4)
        planes[:, :, SMOOTHNESS] = 50
        editable = numpy.ones((4, 4), dtype=bool)
        editable[1, 1] = False
        result = va.clear_planes(PROFILE, planes, (0, 0, 4, 4), editable)
        self.assertEqual(int(result[1, 1, SMOOTHNESS]), 50)
        self.assertEqual(int(result[0, 0, SMOOTHNESS]), 0)


class WindowClipboardTests(EditorDriver):
    def setUp(self):
        super().setUp()
        atlas_file = va.read_atlas_file(self.folder / "atlas.json")
        atlas_file.pbr_profile = LABPBR
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        self.project = va.AtlasProject.open(self.folder)
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))
        self.editor = self.application.editor
        self.editor.clipboard = self.clipboard
        planes = self.project.tiles[(0, 0)].planes
        planes[1:3, 1:3, SMOOTHNESS] = 99
        planes[:, :, EMISSION] = 255
        self.project.tiles[(0, 0)].disk_planes = planes.copy()
        self.select((0, 0))

    def smoothness(self, column, row):
        return int(self.project.tiles[(0, 0)].planes[row, column, SMOOTHNESS])

    def test_copy_and_paste_inside_the_program_carry_the_channels(self):
        self.select_rectangle((1, 1), (2, 2))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.select_rectangle((5, 5), (6, 6))
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.key(va.pygame.K_RETURN)
        self.assertEqual([[self.smoothness(c, r) for c in (5, 6)] for r in (5, 6)], [[99, 99], [99, 99]])
        self.assertEqual(self.smoothness(7, 7), 0)
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual(self.smoothness(5, 5), 0)

    def test_a_picture_from_elsewhere_leaves_the_channels_alone(self):
        self.select_rectangle((5, 5), (6, 6))
        self.paste_picture(solid_tile(GREEN, 2))
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.pixel((0, 0), 5, 5), GREEN)
        self.assertEqual(self.smoothness(1, 1), 99)
        self.assertEqual(self.smoothness(5, 5), 0)

    def test_a_different_picture_of_the_same_size_after_a_copy_does_not_bring_the_channels(self):
        self.select_rectangle((1, 1), (2, 2))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        self.select_rectangle((5, 5), (6, 6))
        self.paste_picture(solid_tile(GREEN, 2))
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.pixel((0, 0), 5, 5), GREEN)
        self.assertEqual(self.smoothness(5, 5), 0)

    def test_a_picture_that_differs_only_in_a_pixel_that_was_transparent_is_not_the_copy(self):
        self.project.tiles[(0, 0)].pixels[1, 1] = (0, 0, 0, 0)
        self.select()
        self.select((0, 0))
        self.select_rectangle((1, 1), (2, 2))
        self.key(va.pygame.K_c, va.pygame.KMOD_CTRL)
        other = self.editor.internal_image.copy()
        other[0, 0] = (0, 0, 255, 255)
        self.select_rectangle((5, 5), (6, 6))
        self.paste_picture(other)
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.smoothness(5, 5), 0)

    def test_delete_takes_the_channels_back_to_neutral_with_the_pixels(self):
        self.select_rectangle((1, 1), (1, 2))
        self.key(va.pygame.K_DELETE)
        self.assertEqual([self.smoothness(1, 1), self.smoothness(1, 2), self.smoothness(2, 1)], [0, 0, 99])
        self.assertEqual(self.pixel((0, 0), 1, 1), (0, 0, 0, 0))
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual([self.smoothness(1, 1), self.smoothness(1, 2)], [99, 99])

    def test_cut_copies_the_channels_and_resets_them(self):
        self.select_rectangle((1, 1), (2, 2))
        self.key(va.pygame.K_x, va.pygame.KMOD_CTRL)
        self.assertEqual(self.smoothness(1, 1), 0)
        self.select_rectangle((4, 4), (5, 5))
        self.key(va.pygame.K_v, va.pygame.KMOD_CTRL)
        self.key(va.pygame.K_RETURN)
        self.assertEqual(self.smoothness(4, 4), 99)

    def test_a_selection_whose_pixels_are_already_clear_is_still_cleared_of_its_channels(self):
        self.project.tiles[(0, 0)].pixels[6, 6] = (0, 0, 0, 0)
        self.project.tiles[(0, 0)].planes[6, 6, SMOOTHNESS] = 5
        self.select()
        self.select((0, 0))
        self.select_rectangle((6, 6), (7, 6))
        self.key(va.pygame.K_DELETE)
        self.assertEqual(self.smoothness(6, 6), 0)


if __name__ == "__main__":
    unittest.main()
