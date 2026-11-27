"""Tests for the blend modes of the colour itself: RGB and alpha each get a mode, as every channel does. The mode replace
for both is what the tools always did and is left to the older tests. No window except in the last class.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import LABPBR, neutral_planes, solid_tile  # noqa: E402
from test_virtual_atlas import BLUE, EditorDriver, va  # noqa: E402

SHADES = [0, 51, 102, 255]


def row_of_grey(alpha=255):
    pixels = numpy.zeros((1, 4, 4), dtype=numpy.uint8)
    pixels[0, :, :3] = numpy.array(SHADES, dtype=numpy.uint8)[:, None]
    pixels[0, :, 3] = alpha
    return pixels


def row_with_alpha(rgb=0):
    pixels = numpy.zeros((1, 4, 4), dtype=numpy.uint8)
    pixels[..., :3] = rgb
    pixels[0, :, 3] = SHADES
    return pixels


class ColorBlendTests(unittest.TestCase):
    def test_replace_for_both_is_the_old_way_and_nothing_else_is(self):
        self.assertTrue(va.ColorBlend().is_replace)
        self.assertTrue(va.ColorBlend("replace", "replace").is_replace)
        self.assertFalse(va.ColorBlend("add", "replace").is_replace)
        self.assertFalse(va.ColorBlend("replace", "none").is_replace)

    def test_an_unknown_mode_is_refused(self):
        with self.assertRaises(ValueError):
            va.ColorBlend("overlay", "replace")
        with self.assertRaises(ValueError):
            va.ColorBlend("replace", "overlay")


class BlendColorTests(unittest.TestCase):
    COLOR = (51, 51, 51, 51)

    def rgb(self, mode, coverage=1.0, color=COLOR):
        old = row_of_grey()
        result = va.blend_color(old, color, mode, "none", numpy.full((1, 4), coverage))
        self.assertTrue((result[..., 3] == 255).all(), "alpha is not painted")
        return result[0, :, 0].tolist()

    def alpha(self, mode, coverage=1.0):
        old = row_with_alpha(7)
        result = va.blend_color(old, self.COLOR, "none", mode, numpy.full((1, 4), coverage))
        self.assertTrue((result[..., :3] == 7).all(), "the colour is not painted")
        return result[0, :, 3].tolist()

    def test_none_for_both_changes_nothing(self):
        old = row_of_grey()
        self.assertTrue(numpy.array_equal(va.blend_color(old, self.COLOR, "none", "none", numpy.ones((1, 4))), old))

    def test_rgb_modes_work_on_the_meaning_of_every_component(self):
        self.assertEqual(self.rgb("replace"), [51, 51, 51, 51])
        self.assertEqual(self.rgb("add"), [51, 102, 153, 255])
        self.assertEqual(self.rgb("subtract"), [0, 0, 51, 204])
        self.assertEqual(self.rgb("multiply"), [0, 10, 20, 51])
        self.assertEqual(self.rgb("min"), [0, 51, 51, 51])
        self.assertEqual(self.rgb("max"), [51, 51, 102, 255])

    def test_alpha_modes_work_the_same_on_alpha_alone(self):
        self.assertEqual(self.alpha("replace"), [51, 51, 51, 51])
        self.assertEqual(self.alpha("add"), [51, 102, 153, 255])
        self.assertEqual(self.alpha("subtract"), [0, 0, 51, 204])
        self.assertEqual(self.alpha("multiply"), [0, 10, 20, 51])
        self.assertEqual(self.alpha("min"), [0, 51, 51, 51])
        self.assertEqual(self.alpha("max"), [51, 51, 102, 255])

    def test_each_component_has_its_own_value(self):
        old = numpy.zeros((1, 1, 4), dtype=numpy.uint8)
        result = va.blend_color(old, (51, 102, 153, 204), "add", "add", numpy.ones((1, 1)))
        self.assertEqual(result[0, 0].tolist(), [51, 102, 153, 204])

    def test_partial_coverage_goes_that_far_and_none_leaves_the_bytes(self):
        # A quarter of the way from 0 to 0.2 is 0.05, the byte 12.75, written as 13.
        self.assertEqual(self.rgb("replace", 0.25)[0], 13)
        self.assertEqual(self.rgb("replace", 0.0), SHADES)

    def test_a_combined_value_past_the_end_stops_at_the_end_before_the_coverage_mixes_it_in(self):
        old = numpy.array([[[204, 0, 0, 255]]], dtype=numpy.uint8)
        # 0.8 + 0.4 stops at 1.0; half of the way from 0.8 to 1.0 is 0.9, the byte 229.5, written as 230.
        result = va.blend_color(old, (102, 0, 0, 255), "add", "none", numpy.full((1, 1), 0.5))
        self.assertEqual(int(result[0, 0, 0]), 230)

    def test_rgb_and_alpha_can_have_different_modes_in_one_stroke(self):
        old = numpy.array([[[0, 51, 102, 255]]], dtype=numpy.uint8)
        result = va.blend_color(old, (51, 51, 51, 51), "add", "subtract", numpy.ones((1, 1)))
        self.assertEqual(result[0, 0].tolist(), [51, 102, 153, 204])

    def test_the_old_pixels_are_not_changed(self):
        old = row_of_grey()
        before = old.copy()
        va.blend_color(old, self.COLOR, "add", "add", numpy.ones((1, 4)))
        self.assertTrue(numpy.array_equal(old, before))


class SessionColorModeTests(unittest.TestCase):
    def layout(self, size=8):
        return va.compose_canvas({(0, 0): solid_tile(BLUE, size)}, size, planes={(0, 0): neutral_planes(size)})

    def start(self, tool, pixel, color_blend, color=(51, 51, 51, 51), settings=None, channels=None, layout=None):
        return va.CanvasSession(
            layout or self.layout(), tool, settings or va.ToolSettings(), color, pixel, color_blend=color_blend, channels=channels
        )

    def test_rgb_add_with_alpha_left_alone(self):
        session = self.start("pencil", (2, 2), va.ColorBlend("add", "none"))
        self.assertEqual(tuple(session.pixels[2, 2]), (51, 51, 255, 255))
        self.assertEqual(tuple(session.pixels[3, 3]), BLUE)
        self.assertTrue(session.changed)

    def test_alpha_subtract_with_the_colour_left_alone(self):
        session = self.start("pencil", (2, 2), va.ColorBlend("none", "subtract"))
        self.assertEqual(tuple(session.pixels[2, 2]), (0, 0, 255, 204))

    def test_going_over_pixels_again_adds_only_once(self):
        session = self.start("pencil", (1, 1), va.ColorBlend("add", "none"))
        for pixel in [(2, 1), (3, 1), (2, 1), (1, 1)]:
            session.move(pixel)
        self.assertEqual(session.pixels[1, 1:4, 0].tolist(), [51, 51, 51])

    def test_a_line_that_is_dragged_back_leaves_nothing_behind(self):
        session = self.start("line", (1, 1), va.ColorBlend("add", "none"))
        session.move((6, 1))
        session.move((3, 1))
        self.assertEqual(session.pixels[1, :, 0].tolist(), [0, 51, 51, 51, 0, 0, 0, 0])

    def test_a_pixel_the_tool_drew_again_without_more_coverage_is_still_blended(self):
        settings = va.ToolSettings()
        settings.diameters["noise_brush"] = 1
        settings.fractions["noise_density"] = 1.0
        session = self.start("noise_brush", (4, 4), va.ColorBlend("add", "none"), settings=settings)
        session.move((5, 4))
        session.move((4, 4))  # stamps the first pixel again: the noise brush writes the colour there as it always did
        self.assertEqual(tuple(session.pixels[4, 4]), (51, 51, 255, 255))

    def test_both_replace_is_the_old_stroke(self):
        session = self.start("pencil", (2, 2), va.ColorBlend("replace", "replace"))
        self.assertEqual(tuple(session.pixels[2, 2]), (51, 51, 51, 51))

    def test_the_eraser_ignores_the_modes(self):
        session = self.start("eraser", (2, 2), va.ColorBlend("add", "none"))
        self.assertEqual(tuple(session.pixels[2, 2]), (0, 0, 0, 0))

    def test_the_fill_uses_the_modes(self):
        session = self.start("fill", (0, 0), va.ColorBlend("add", "none"))
        self.assertEqual(tuple(session.pixels[7, 7]), (51, 51, 255, 255))

    def test_airbrush_changes_only_the_pixels_it_covers_whatever_the_mode(self):
        settings = va.ToolSettings()
        session = self.start("airbrush", (4, 4), va.ColorBlend("add", "none"), settings=settings)
        covered = {(int(c), int(r)) for r, c in numpy.argwhere(session.airbrush.coverage > 0)}
        changed = {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(session.pixels != session.base, axis=2))}
        self.assertTrue(changed <= covered and len(changed) > 5, "only pixels the brush covers change, and more than the middle one")
        self.assertEqual(tuple(session.pixels[4, 4]), (51, 51, 255, 255))

    def test_channels_and_colour_are_painted_in_the_same_stroke(self):
        channels = va.ChannelPaint(
            va.PBR_PROFILES[LABPBR], {"smoothness": va.ChannelBlend("replace", 0.2)}
        )
        session = self.start("pencil", (2, 2), va.ColorBlend("add", "none"), channels=channels)
        self.assertEqual(tuple(session.pixels[2, 2]), (51, 51, 255, 255))
        self.assertEqual(int(session.planes[2, 2, 0]), 51)

    def test_without_channels_in_the_layout_the_modes_still_work(self):
        layout = va.compose_canvas({(0, 0): solid_tile(BLUE, 8)}, 8)
        session = self.start("pencil", (2, 2), va.ColorBlend("add", "none"), layout=layout)
        self.assertEqual(tuple(session.pixels[2, 2]), (51, 51, 255, 255))


class WindowColorModeTests(EditorDriver):
    def test_the_modes_of_the_editor_are_the_ones_the_stroke_uses_and_ctrl_z_takes_it_back(self):
        self.editor.controls.set_mode("rgb", "add")
        self.editor.controls.set_mode("alpha", "none")
        self.editor.colors.set_color((51, 51, 51, 255), "primary")
        self.select((0, 0))
        self.click_tool("pencil")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(self.pixel((0, 0), 2, 2), (51, 51, 255, 255))
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual(self.pixel((0, 0), 2, 2), BLUE)


if __name__ == "__main__":
    unittest.main()
