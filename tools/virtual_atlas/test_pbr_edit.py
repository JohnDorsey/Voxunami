"""Tests for the channels of a tile as part of editing: the canvas that shows planes beside pixels, an edit that changes
planes, undo and redo of it, and where the planes of a tile with no file yet are kept (atlas.json). No window.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import json
import sys
import unittest
from pathlib import Path
from unittest import mock

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import (  # noqa: E402
    EMISSION,
    NORMAL_X,
    RED,
    SMOOTHNESS,
    PbrProjectFixture,
    neutral_planes,
    solid_tile,
)
from test_virtual_atlas import va  # noqa: E402

GREEN = (0, 255, 0, 255)


class CanvasPlanesTests(unittest.TestCase):
    def test_a_canvas_arranges_the_planes_of_its_tiles_as_it_does_their_pixels(self):
        first, second = neutral_planes(4), neutral_planes(4)
        first[:, :, SMOOTHNESS] = 10
        second[:, :, SMOOTHNESS] = 20
        layout = va.compose_canvas(
            {(0, 0): solid_tile(RED), (2, 0): solid_tile(GREEN)}, 4, planes={(0, 0): first, (2, 0): second}
        )
        self.assertEqual(layout.planes.shape, (4, 12, 8))
        self.assertEqual(int(layout.planes[0, 0, SMOOTHNESS]), 10)
        self.assertEqual(int(layout.planes[3, 11, SMOOTHNESS]), 20)
        self.assertTrue((layout.planes[:, 4:8] == 0).all(), "the empty place between the tiles has nothing")

    def test_a_canvas_with_no_planes_has_none(self):
        layout = va.compose_canvas({(0, 0): solid_tile(RED)}, 4)
        self.assertIsNone(layout.planes)

    def test_planes_of_an_edited_canvas_go_back_to_their_tiles_as_copies(self):
        first, second = neutral_planes(4), neutral_planes(4)
        layout = va.compose_canvas(
            {(0, 0): solid_tile(RED), (1, 0): solid_tile(GREEN)}, 4, planes={(0, 0): first, (1, 0): second}
        )
        edited = layout.planes.copy()
        edited[1, 5, SMOOTHNESS] = 99
        tiles = va.extract_canvas_planes(layout, edited)
        self.assertEqual(int(tiles[(1, 0)][1, 1, SMOOTHNESS]), 99)
        self.assertEqual(int(tiles[(0, 0)][1, 1, SMOOTHNESS]), 0)
        tiles[(1, 0)][1, 1, SMOOTHNESS] = 5
        self.assertEqual(int(edited[1, 5, SMOOTHNESS]), 99)


class EditPlanesTests(PbrProjectFixture):
    def project_with_tiles(self, count=1):
        placed = {(index, 0): (f"tile{index}.png", solid_tile(RED)) for index in range(count)}
        project = self.make_pbr_project(placed)
        project.selection = set(placed)
        return project

    def layout_of(self, project):
        records = {coordinate: project.tiles[coordinate] for coordinate in project.selection}
        return va.compose_canvas(
            {c: r.pixels for c, r in records.items()}, project.tile_size, planes={c: r.planes for c, r in records.items()}
        )

    def test_an_edit_of_only_the_planes_is_an_edit(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        edited = layout.planes.copy()
        edited[2, 1, SMOOTHNESS] = 90
        self.assertTrue(project.apply_canvas_edit(layout, layout.pixels, edited))
        record = project.tiles[(0, 0)]
        self.assertEqual(int(record.planes[2, 1, SMOOTHNESS]), 90)
        self.assertTrue((record.pixels == solid_tile(RED)).all())
        self.assertEqual(project.unsaved_tiles(), [(0, 0)])

    def test_an_edit_that_changes_nothing_is_not_an_edit(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        self.assertFalse(project.apply_canvas_edit(layout, layout.pixels.copy(), layout.planes.copy()))
        self.assertFalse(project.history.can_undo(project.shown_records()))

    def test_leaving_the_planes_out_edits_the_pixels_only(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        edited = layout.pixels.copy()
        edited[0, 0] = (1, 2, 3, 4)
        self.assertTrue(project.apply_canvas_edit(layout, edited))
        self.assertEqual(project.tiles[(0, 0)].pixels[0, 0].tolist(), [1, 2, 3, 4])
        self.assertTrue(numpy.array_equal(project.tiles[(0, 0)].planes, neutral_planes(4)))

    def test_undo_and_redo_step_the_pixels_and_the_planes_together(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        edited_pixels, edited_planes = layout.pixels.copy(), layout.planes.copy()
        edited_pixels[0, 0] = (1, 2, 3, 4)
        edited_planes[0, 0, EMISSION] = 7
        project.apply_canvas_edit(layout, edited_pixels, edited_planes)
        self.assertTrue(project.undo())
        record = project.tiles[(0, 0)]
        self.assertTrue((record.pixels == solid_tile(RED)).all())
        self.assertTrue(numpy.array_equal(record.planes, neutral_planes(4)))
        self.assertFalse(record.is_modified)
        self.assertTrue(project.redo())
        self.assertEqual(project.tiles[(0, 0)].pixels[0, 0].tolist(), [1, 2, 3, 4])
        self.assertEqual(int(project.tiles[(0, 0)].planes[0, 0, EMISSION]), 7)

    def test_only_the_tile_whose_planes_changed_gets_a_history_entry(self):
        project = self.project_with_tiles(2)
        layout = self.layout_of(project)
        edited = layout.planes.copy()
        edited[0, 5, SMOOTHNESS] = 90  # in the second tile
        project.apply_canvas_edit(layout, layout.pixels, edited)
        self.assertEqual(project.unsaved_tiles(), [(1, 0)])
        project.selection = {(0, 0)}
        self.assertFalse(project.history.can_undo(project.shown_records()))
        project.selection = {(1, 0)}
        self.assertTrue(project.history.can_undo(project.shown_records()))

    def test_planes_painted_and_painted_back_are_not_unsaved(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        edited = layout.planes.copy()
        edited[0, 0, NORMAL_X] = 200
        project.apply_canvas_edit(layout, layout.pixels, edited)
        layout = self.layout_of(project)
        project.apply_canvas_edit(layout, layout.pixels, neutral_planes(4))
        self.assertEqual(project.unsaved_tiles(), [])

    def test_painted_planes_are_saved_as_companion_files(self):
        project = self.project_with_tiles()
        layout = self.layout_of(project)
        edited = layout.planes.copy()
        edited[:, :, SMOOTHNESS] = 33
        project.apply_canvas_edit(layout, layout.pixels, edited)
        project.save_tile((0, 0))
        self.assertTrue((self.read("tile0_s.png") == [33, 0, 0, 255]).all())

    def test_if_atlas_json_cannot_be_written_the_old_planes_come_back(self):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        project.selection = {(0, 0)}
        layout = self.layout_of(project)
        edited = layout.planes.copy()
        edited[:, :, SMOOTHNESS] = 33
        with mock.patch.object(project, "save_atlas_file", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                project.apply_canvas_edit(layout, layout.pixels, edited)
        self.assertTrue(numpy.array_equal(project.tiles[(0, 0)].planes, neutral_planes(4)))


class NewTileChannelsTests(PbrProjectFixture):
    def new_tile_with(self, planes_change):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        project.selection = {(0, 0)}
        record = project.tiles[(0, 0)]
        layout = va.compose_canvas({(0, 0): record.pixels}, 4, planes={(0, 0): record.planes})
        edited = layout.planes.copy()
        planes_change(edited)
        project.apply_canvas_edit(layout, layout.pixels, edited)
        return project

    def paint_smoothness(self, planes):
        planes[1, 2, SMOOTHNESS] = 77

    def test_the_planes_of_a_tile_with_no_file_are_kept_in_atlas_json(self):
        project = self.new_tile_with(self.paint_smoothness)
        stored = self.saved().coordinates_to_channel_data
        self.assertEqual(list(stored), [(0, 0)])
        expected = project.tiles[(0, 0)].planes
        self.assertEqual(va.decode_channel_data(stored[(0, 0)], 4, 8).tolist(), expected.tolist())

    def test_a_tile_with_no_channel_painted_keeps_nothing_and_atlas_json_has_no_such_key(self):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        self.assertEqual(self.saved().coordinates_to_channel_data, {})
        document = json.loads((self.folder / "atlas.json").read_text(encoding="utf-8"))
        self.assertNotIn("coordinates_to_channel_data", document)

    def test_painting_the_channels_back_to_neutral_removes_the_entry(self):
        project = self.new_tile_with(self.paint_smoothness)
        record = project.tiles[(0, 0)]
        layout = va.compose_canvas({(0, 0): record.pixels}, 4, planes={(0, 0): record.planes})
        project.apply_canvas_edit(layout, layout.pixels, neutral_planes(4))
        self.assertEqual(self.saved().coordinates_to_channel_data, {})

    def test_opening_the_folder_again_brings_the_planes_back_and_the_tile_is_still_new(self):
        self.new_tile_with(self.paint_smoothness)
        reopened = va.AtlasProject.open(self.folder)
        record = reopened.tiles[(0, 0)]
        self.assertTrue(record.is_new)
        self.assertEqual(int(record.planes[1, 2, SMOOTHNESS]), 77)
        self.assertEqual(int(record.planes[0, 0, SMOOTHNESS]), 0)
        self.assertEqual(reopened.unsaved_tiles(), [(0, 0)])

    def test_saving_the_tile_writes_its_files_and_takes_the_entry_out(self):
        project = self.new_tile_with(self.paint_smoothness)
        project.save_tile((0, 0))
        self.assertEqual(self.saved().coordinates_to_channel_data, {})
        self.assertEqual(self.read("new_tile_s.png")[1, 2].tolist(), [77, 0, 0, 255])

    def test_stored_channels_that_do_not_fit_make_an_error_tile_and_are_kept_as_they_were(self):
        self.new_tile_with(self.paint_smoothness)
        atlas_file = self.saved()
        atlas_file.coordinates_to_channel_data[(0, 0)] = "AAAA"
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        reopened = va.AtlasProject.open(self.folder)
        self.assertIn("channel", reopened.tiles[(0, 0)].error)
        reopened.save_atlas_file()
        self.assertEqual(self.saved().coordinates_to_channel_data, {(0, 0): "AAAA"})

    def test_the_key_is_read_and_written_by_the_atlas_file(self):
        text = json.dumps(
            {"tile_size": 4, "pbr": LABPBR_NAME, "coordinates_to_names": {"(0, 0)": "a.png"},
             "coordinates_to_data": {"(0, 0)": "AAAA"}, "coordinates_to_channel_data": {"(0, 0)": "BBBB"}}
        )
        atlas_file = va.parse_atlas_file(text)
        self.assertEqual(atlas_file.coordinates_to_channel_data, {(0, 0): "BBBB"})
        self.assertEqual(va.parse_atlas_file(va.serialize_atlas_file(atlas_file)).coordinates_to_channel_data, {(0, 0): "BBBB"})

    def test_channel_data_for_a_place_with_no_name_is_a_problem(self):
        text = json.dumps({"tile_size": 4, "coordinates_to_names": {}, "coordinates_to_channel_data": {"(1, 1)": "AA"}})
        with self.assertRaises(va.AtlasFileError) as raised:
            va.parse_atlas_file(text)
        self.assertIn("coordinates_to_channel_data", str(raised.exception))

    def test_channel_data_that_is_not_text_is_a_problem(self):
        text = json.dumps(
            {"tile_size": 4, "coordinates_to_names": {"(0, 0)": "a.png"}, "coordinates_to_channel_data": {"(0, 0)": 5}}
        )
        with self.assertRaises(va.AtlasFileError):
            va.parse_atlas_file(text)

    def test_the_text_of_channel_data_is_the_raw_bytes_row_by_row_in_base64(self):
        planes = neutral_planes(2)
        planes[0, 1] = [1, 2, 3, 4, 5, 6, 7, 8]
        text = va.encode_channel_data(planes)
        self.assertEqual(va.decode_channel_data(text, 2, 8).tolist(), planes.tolist())
        raw = __import__("base64").b64decode(text)
        self.assertEqual(list(raw[8:16]), [1, 2, 3, 4, 5, 6, 7, 8])

    def test_text_of_the_wrong_length_or_not_base64_is_refused(self):
        for bad in ("AAAA", "not base64!", va.encode_channel_data(neutral_planes(2))):
            with self.assertRaises(va.TileDataError):
                va.decode_channel_data(bad, 4, 8)


LABPBR_NAME = "labpbr_1_3"

if __name__ == "__main__":
    unittest.main()
