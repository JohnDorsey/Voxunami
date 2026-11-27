"""Tests for what the drawing tools do to the channels of the tiles: a stroke that paints channels, the eraser that resets
them, the fill that matches them, the rotation that carries them. No window.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import random
import sys
import unittest
from pathlib import Path

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import (  # noqa: E402
    EMISSION,
    LABPBR,
    NORMAL_X,
    NORMAL_Y,
    RED,
    SMOOTHNESS,
    neutral_planes,
    solid_tile,
)
from test_virtual_atlas import va  # noqa: E402

BLUE = (0, 0, 255, 255)
PROFILE = va.PBR_PROFILES[LABPBR]


def paint(**blends):
    """The channels a stroke paints: paint(smoothness=("replace", 0.2))."""
    return va.ChannelPaint(PROFILE, {identity: va.ChannelBlend(mode, value) for identity, (mode, value) in blends.items()})


class SessionFixture(unittest.TestCase):
    def layout(self, size=8, planes=None):
        tile_planes = neutral_planes(size) if planes is None else planes
        return va.compose_canvas({(0, 0): solid_tile(BLUE, size)}, size, planes={(0, 0): tile_planes})

    def start(self, layout, tool, pixel, color=RED, settings=None, channels=None, **keywords):
        return va.CanvasSession(layout, tool, settings or va.ToolSettings(), color, pixel, channels=channels, **keywords)

    def pixels_painted(self, session):
        return {(int(c), int(r)) for r, c in numpy.argwhere(numpy.any(session.pixels != session.base, axis=2))}

    def pixels_with_planes_changed(self, session):
        return {
            (int(c), int(r))
            for r, c in numpy.argwhere(numpy.any(session.planes != session.layout.planes, axis=2))
        }


class ChannelStrokeCoverageTests(unittest.TestCase):
    def test_setting_the_coverage_replaces_it_where_adding_keeps_the_strongest(self):
        stroke = va.ChannelStroke(PROFILE, neutral_planes(4), {"smoothness": va.ChannelBlend("replace", 0.2)})
        stroke.add_coverage(numpy.ones((4, 4)))
        stroke.add_coverage(numpy.zeros((4, 4)))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 51)
        stroke.set_coverage(numpy.zeros((4, 4)))
        self.assertEqual(int(stroke.planes[0, 0, SMOOTHNESS]), 0)
        self.assertFalse(stroke.changed)


class PencilTests(SessionFixture):
    def test_the_planes_of_a_canvas_with_none_are_none(self):
        layout = va.compose_canvas({(0, 0): solid_tile(BLUE, 8)}, 8)
        session = self.start(layout, "pencil", (3, 3), channels=paint(smoothness=("replace", 0.2)))
        self.assertIsNone(session.planes)

    def test_with_no_channels_chosen_the_planes_are_carried_along_unchanged(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3))
        self.assertTrue(numpy.array_equal(session.planes, layout.planes))
        self.assertIsNot(session.planes, layout.planes)
        self.assertEqual(tuple(session.pixels[3, 3]), RED)

    def test_every_mode_none_leaves_the_planes_as_they_were(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3), channels=paint(smoothness=("none", 0.9)))
        self.assertTrue(numpy.array_equal(session.planes, layout.planes))

    def test_the_pencil_paints_the_channel_where_it_paints_the_pixel(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3), channels=paint(smoothness=("replace", 0.2)))
        self.assertEqual(int(session.planes[3, 3, SMOOTHNESS]), 51)
        self.assertEqual(self.pixels_with_planes_changed(session), {(3, 3)})
        self.assertTrue(numpy.array_equal(layout.planes, neutral_planes(8)), "the layout is not touched")

    def test_a_stroke_that_paints_only_a_channel_is_a_change(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3), color=BLUE, channels=paint(smoothness=("replace", 0.2)))
        self.assertTrue(numpy.array_equal(session.pixels, layout.pixels))
        self.assertTrue(session.changed)

    def test_a_stroke_that_changes_nothing_is_no_change(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (3, 3), color=BLUE, channels=paint(smoothness=("none", 0.2)))
        self.assertFalse(session.changed)

    def test_going_over_pixels_again_adds_only_once(self):
        layout = self.layout()
        session = self.start(layout, "pencil", (1, 1), channels=paint(smoothness=("add", 0.2)))
        for pixel in [(2, 1), (3, 1), (2, 1), (1, 1), (3, 1)]:
            session.move(pixel)
        self.assertEqual(session.planes[1, 1:4, SMOOTHNESS].tolist(), [51, 51, 51])

    def test_two_strokes_add_twice(self):
        layout = self.layout()
        first = self.start(layout, "pencil", (1, 1), channels=paint(smoothness=("add", 0.2)))
        second_layout = va.compose_canvas({(0, 0): first.pixels}, 8, planes={(0, 0): first.planes})
        second = self.start(second_layout, "pencil", (1, 1), channels=paint(smoothness=("add", 0.2)))
        self.assertEqual(int(second.planes[1, 1, SMOOTHNESS]), 102)

    def test_the_channels_and_the_pixels_cover_the_same_pixels_across_the_edge_with_wrap_assist(self):
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (3, 3)
        session = self.start(
            self.layout(), "pencil", (0, 0), settings=settings, channels=paint(smoothness=("replace", 0.2)), wrap=True
        )
        self.assertEqual(len(self.pixels_painted(session)), 9)
        self.assertEqual(self.pixels_with_planes_changed(session), self.pixels_painted(session))

    def test_nothing_is_painted_in_the_empty_places_between_tiles(self):
        layout = va.compose_canvas(
            {(0, 0): solid_tile(BLUE, 4), (2, 0): solid_tile(BLUE, 4)},
            4,
            planes={(0, 0): neutral_planes(4), (2, 0): neutral_planes(4)},
        )
        settings = va.ToolSettings()
        settings.brush_sizes["pencil"] = (12, 1)
        # A brush 12 wide covers 6 pixels before the cursor pixel and the rest after it: the whole row from the pixel 6.
        session = self.start(layout, "pencil", (6, 0), settings=settings, channels=paint(smoothness=("replace", 0.2)))
        self.assertEqual(session.planes[0, :, SMOOTHNESS].tolist(), [51] * 4 + [0] * 4 + [51] * 4)

    def test_a_painted_normal_is_shortened_at_once(self):
        session = self.start(
            self.layout(), "pencil", (2, 2), channels=paint(normal_x=("replace", 1.0), normal_y=("replace", 1.0))
        )
        self.assertEqual(session.planes[2, 2, [NORMAL_X, NORMAL_Y]].tolist(), [218, 218])


class LineTests(SessionFixture):
    def test_a_line_that_is_dragged_back_leaves_no_channel_behind(self):
        layout = self.layout()
        session = self.start(layout, "line", (1, 1), channels=paint(smoothness=("replace", 0.2)))
        session.move((6, 1))
        self.assertEqual(session.planes[1, :, SMOOTHNESS].tolist(), [0, 51, 51, 51, 51, 51, 51, 0])
        session.move((3, 1))
        self.assertEqual(session.planes[1, :, SMOOTHNESS].tolist(), [0, 51, 51, 51, 0, 0, 0, 0])
        self.assertEqual(self.pixels_with_planes_changed(session), self.pixels_painted(session))

    def test_a_line_adds_only_once_however_it_is_dragged(self):
        session = self.start(self.layout(), "line", (1, 1), channels=paint(smoothness=("add", 0.2)))
        for pixel in [(6, 1), (2, 1), (6, 1), (5, 5), (6, 1)]:
            session.move(pixel)
        self.assertEqual(session.planes[1, 1:7, SMOOTHNESS].tolist(), [51] * 6)


class NoiseAndAirbrushTests(SessionFixture):
    def test_the_noise_brush_paints_the_channel_on_the_pixels_it_hit(self):
        settings = va.ToolSettings()
        settings.fractions["noise_density"] = 0.5
        session = self.start(
            self.layout(),
            "noise_brush",
            (4, 4),
            settings=settings,
            channels=paint(smoothness=("replace", 0.2)),
            random_source=random.Random(3),
        )
        for pixel in [(5, 4), (6, 4)]:
            session.move(pixel)
        painted = self.pixels_painted(session)
        self.assertGreater(len(painted), 3)
        self.assertEqual(self.pixels_with_planes_changed(session), painted)

    def test_a_pixel_with_half_the_opacity_gets_half_of_the_change(self):
        settings = va.ToolSettings()
        settings.fractions["noise_density"] = 1.0
        settings.fractions["noise_opacity"] = 0.5
        session = self.start(
            self.layout(), "noise_brush", (4, 4), settings=settings, channels=paint(smoothness=("replace", 1.0))
        )
        # 0.5 of the way from 0 to 1 is the byte 127.5, written as 128.
        self.assertEqual(int(session.planes[4, 4, SMOOTHNESS]), 128)

    def test_the_airbrush_paints_the_channel_as_far_as_it_covers(self):
        settings = va.ToolSettings()
        settings.diameters["airbrush"] = 7
        session = self.start(
            self.layout(), "airbrush", (4, 4), settings=settings, channels=paint(smoothness=("replace", 1.0))
        )
        coverage = session.airbrush.coverage
        self.assertEqual(int(session.planes[4, 4, SMOOTHNESS]), 255)
        partial = [(row, column) for row, column in numpy.argwhere((coverage > 0) & (coverage < 1))]
        self.assertTrue(partial, "the soft edge has pixels that are only partly covered")
        for row, column in partial:
            self.assertEqual(int(session.planes[row, column, SMOOTHNESS]), int(numpy.rint(coverage[row, column] * 255)))
        self.assertEqual(self.pixels_with_planes_changed(session), {(int(c), int(r)) for r, c in numpy.argwhere(coverage > 0)})


class EraserTests(SessionFixture):
    def painted_planes(self):
        planes = neutral_planes(8)
        planes[:, :, SMOOTHNESS] = 200
        planes[:, :, EMISSION] = 100
        return planes

    def test_the_eraser_gives_the_pixels_it_passes_their_neutral_channels_whatever_the_modes(self):
        layout = self.layout(planes=self.painted_planes())
        session = self.start(layout, "eraser", (2, 2), channels=paint(smoothness=("replace", 0.9)))
        self.assertEqual(session.planes[2, 2].tolist(), neutral_planes(1)[0, 0].tolist())
        self.assertEqual(session.planes[3, 3, SMOOTHNESS], 200)
        self.assertEqual(session.pixels[2, 2].tolist(), [0, 0, 0, 0])

    def test_the_eraser_resets_the_channels_where_it_erases_the_pixels(self):
        layout = self.layout(planes=self.painted_planes())
        settings = va.ToolSettings()
        settings.brush_sizes["eraser"] = (3, 3)
        session = self.start(layout, "eraser", (2, 2), settings=settings, channels=paint())
        session.move((5, 5))
        self.assertEqual(self.pixels_with_planes_changed(session), self.pixels_painted(session))

    def test_without_channels_chosen_the_eraser_leaves_the_planes_alone(self):
        layout = self.layout(planes=self.painted_planes())
        session = self.start(layout, "eraser", (2, 2))
        self.assertTrue(numpy.array_equal(session.planes, layout.planes))


class FillTests(SessionFixture):
    def wall(self):
        planes = neutral_planes(8)
        planes[:, 4, SMOOTHNESS] = 10  # a column whose channel differs and whose pixels do not
        return planes

    def test_the_fill_stops_where_a_channel_differs_though_the_pixels_are_alike(self):
        session = self.start(self.layout(planes=self.wall()), "fill", (0, 0))
        painted = self.pixels_painted(session)
        self.assertEqual(painted, {(column, row) for column in range(4) for row in range(8)})

    def test_the_fill_inside_the_wall_is_the_wall_only(self):
        session = self.start(self.layout(planes=self.wall()), "fill", (4, 3))
        self.assertEqual(self.pixels_painted(session), {(4, row) for row in range(8)})

    def test_the_fill_paints_the_channels_of_what_it_filled(self):
        session = self.start(
            self.layout(planes=self.wall()), "fill", (0, 0), channels=paint(emission=("replace", 0.2))
        )
        self.assertEqual(session.planes[0, :, EMISSION].tolist(), [50] * 4 + [255] * 4)

    def test_the_fill_of_tiles_with_no_channels_is_as_it_was(self):
        layout = va.compose_canvas({(0, 0): solid_tile(BLUE, 8)}, 8)
        session = self.start(layout, "fill", (0, 0))
        self.assertEqual(len(self.pixels_painted(session)), 64)


class RotateTests(SessionFixture):
    def test_rotating_a_row_rotates_its_channels_with_it(self):
        planes = neutral_planes(8)
        planes[2, :, SMOOTHNESS] = [10, 20, 30, 40, 50, 60, 70, 80]
        session = self.start(self.layout(planes=planes), "rotate", (1, 2))
        session.move((3, 2))
        self.assertEqual(session.planes[2, :, SMOOTHNESS].tolist(), [70, 80, 10, 20, 30, 40, 50, 60])

    def test_rotating_a_column_rotates_its_channels_with_it(self):
        planes = neutral_planes(8)
        planes[:, 5, SMOOTHNESS] = [10, 20, 30, 40, 50, 60, 70, 80]
        session = self.start(self.layout(planes=planes), "rotate", (5, 1))
        session.move((5, 2))
        self.assertEqual(session.planes[:, 5, SMOOTHNESS].tolist(), [80, 10, 20, 30, 40, 50, 60, 70])

    def test_dragging_back_puts_the_channels_back(self):
        planes = neutral_planes(8)
        planes[2, :, SMOOTHNESS] = [10, 20, 30, 40, 50, 60, 70, 80]
        layout = self.layout(planes=planes)
        session = self.start(layout, "rotate", (1, 2))
        session.move((4, 2))
        session.move((1, 2))
        self.assertTrue(numpy.array_equal(session.planes, layout.planes))
        self.assertFalse(session.changed)

    def test_a_rotation_of_only_the_channels_is_a_change(self):
        planes = neutral_planes(8)
        planes[2, 0, SMOOTHNESS] = 10  # the pixels of a row are all alike, so rolling them changes nothing
        session = self.start(self.layout(planes=planes), "rotate", (1, 2))
        session.move((2, 2))
        self.assertTrue(session.changed)


from test_virtual_atlas import EditorDriver  # noqa: E402


class WindowStrokeTests(EditorDriver):
    """The editor with a LabPBR atlas: the strokes of the mouse reach the channels of the tiles, and Ctrl+Z takes them back."""

    def setUp(self):
        super().setUp()
        atlas_file = va.read_atlas_file(self.folder / "atlas.json")
        atlas_file.pbr_profile = LABPBR
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        self.project = va.AtlasProject.open(self.folder)
        self.application = va.Application(self.project, va.pygame.Surface((1280, 800)))
        self.editor = self.application.editor
        self.editor.clipboard = self.clipboard

    def planes_at(self, column, row):
        return self.project.tiles[(0, 0)].planes[row, column].tolist()

    def test_the_canvas_has_the_channels_of_the_shown_tiles(self):
        self.select((0, 0), (1, 0))
        self.assertEqual(self.editor.layout.planes.shape, (8, 16, 8))

    def test_a_pencil_stroke_paints_the_channels_chosen_and_ctrl_z_takes_them_back(self):
        self.editor.controls.set_mode("smoothness", "replace")
        self.editor.controls.set_value("smoothness", "primary", 0.2)
        self.select((0, 0))
        self.click_tool("pencil")
        self.drag(self.position(2, 2), self.position(4, 2))
        self.assertEqual([self.planes_at(c, 2)[SMOOTHNESS] for c in range(1, 6)], [0, 51, 51, 51, 0])
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual([self.planes_at(c, 2)[SMOOTHNESS] for c in range(1, 6)], [0, 0, 0, 0, 0])
        self.assertEqual(self.project.unsaved_tiles(), [])
        self.key(va.pygame.K_y, va.pygame.KMOD_CTRL)
        self.assertEqual(self.planes_at(3, 2)[SMOOTHNESS], 51)

    def test_with_no_channel_chosen_a_stroke_leaves_the_channels_alone(self):
        self.select((0, 0))
        self.click_tool("pencil")
        self.drag(self.position(2, 2), self.position(4, 2))
        self.assertTrue(numpy.array_equal(self.project.tiles[(0, 0)].planes, neutral_planes(8)))

    def test_the_eraser_resets_the_channels_and_ctrl_z_brings_them_back(self):
        self.project.tiles[(0, 0)].planes[:, :, SMOOTHNESS] = 200
        self.project.tiles[(0, 0)].disk_planes = self.project.tiles[(0, 0)].planes.copy()
        self.select((0, 0))
        self.editor.sync()
        self.click_tool("eraser")
        self.drag(self.position(2, 2), self.position(3, 2))
        self.assertEqual([self.planes_at(c, 2)[SMOOTHNESS] for c in range(1, 5)], [200, 0, 0, 200])
        self.key(va.pygame.K_z, va.pygame.KMOD_CTRL)
        self.assertEqual([self.planes_at(c, 2)[SMOOTHNESS] for c in range(1, 5)], [200, 200, 200, 200])

    def test_a_stroke_that_only_paints_a_channel_makes_the_tile_unsaved_and_saving_writes_the_file(self):
        self.editor.controls.set_mode("emission", "replace")
        self.editor.controls.set_value("emission", "primary", 0.2)
        self.select((0, 0))
        self.click_tool("pencil")
        self.send("down", self.position(2, 2))
        self.send("up", self.position(2, 2))
        self.assertEqual(self.project.unsaved_tiles(), [(0, 0)])
        self.project.save_tile((0, 0))
        self.assertEqual(va.read_image_pixels(self.folder / "a_s.png")[2, 2].tolist(), [0, 0, 0, 50])


if __name__ == "__main__":
    unittest.main()
