"""Tests for changing the PBR setting of a project (and for Reload), which moves a tile's companion files between formats.

Only "none" and "labpbr_1_3" exist, so the tests that move channels between two formats install made-up profiles
(`test_split`, `test_swapped`) for the length of a test.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import json
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import (  # noqa: E402
    LABPBR, RED, PbrProjectFixture, normal_picture, solid_tile, specular_picture, va,
)

# A profile whose two files are named differently from LabPBR's and hold the channels in other bytes, and that has no place
# for four of LabPBR's channels (reflectance, porosity_scattering, normal_y, ambient_occlusion).
SPLIT = va.PbrProfile(
    "test_split",
    (
        va.PbrChannel("smoothness", "_spec", 0, 0),
        va.PbrChannel("emission", "_spec", 1, 255),
        va.PbrChannel("extra_a", "_spec", 2, 0),
        va.PbrChannel("extra_b", "_spec", 3, 0),
        va.PbrChannel("height", "_bump", 0, 255),
        va.PbrChannel("normal_x", "_bump", 1, 128),
        va.PbrChannel("extra_c", "_bump", 2, 0),
        va.PbrChannel("extra_d", "_bump", 3, 0),
    ),
)
# LabPBR with smoothness and reflectance in each other's byte: the same two file endings, other bytes.
SWAPPED = va.PbrProfile(
    "test_swapped",
    (
        va.PbrChannel("smoothness", "_s", 1, 0),
        va.PbrChannel("reflectance", "_s", 0, 0),
        va.PbrChannel("porosity_scattering", "_s", 2, 0),
        va.PbrChannel("emission", "_s", 3, 255),
        va.PbrChannel("normal_x", "_n", 0, 128),
        va.PbrChannel("normal_y", "_n", 1, 128),
        va.PbrChannel("ambient_occlusion", "_n", 2, 255),
        va.PbrChannel("height", "_n", 3, 255),
    ),
)


class SwitchFixture(PbrProjectFixture):
    def setUp(self):
        super().setUp()
        patch = mock.patch.dict(va.PBR_PROFILES, {SPLIT.name: SPLIT, SWAPPED.name: SWAPPED})
        patch.start()
        self.addCleanup(patch.stop)

    def read_setting(self):
        return json.loads((self.folder / "atlas.json").read_text(encoding="utf-8")).get("pbr", "none")

    def all_files(self):
        return sorted(entry.name for entry in os.scandir(self.folder) if entry.name != "atlas.json")

    def labpbr_dirt_and_lamp(self):
        return self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("lamp.png", solid_tile(RED))},
            companions={
                "dirt_s.png": specular_picture(4, 10, 20, 30, 40),
                "dirt_n.png": normal_picture(4, 50, 60, 70, 80),
            },
        )


class FilesAllOrNoneWithDeletionsTests(SwitchFixture):
    def test_deleted_files_come_back_when_a_write_fails(self):
        (self.folder / "old.bin").write_bytes(b"old")
        (self.folder / "gone.bin").write_bytes(b"gone")
        real_replace = os.replace

        def fail(source, target):
            raise OSError("no")

        with mock.patch.object(va.os, "replace", fail):
            with self.assertRaises(OSError):
                va.write_files_all_or_none(
                    [(self.folder / "old.bin", b"new")], deletions=[self.folder / "gone.bin"]
                )
        self.assertEqual((self.folder / "gone.bin").read_bytes(), b"gone")
        self.assertEqual((self.folder / "old.bin").read_bytes(), b"old")
        self.assertEqual(self.all_files(), ["gone.bin", "old.bin"])
        va.write_files_all_or_none([(self.folder / "old.bin", b"new")], deletions=[self.folder / "gone.bin"])
        self.assertEqual(self.all_files(), ["old.bin"])
        self.assertEqual((self.folder / "old.bin").read_bytes(), b"new")


    def test_a_file_already_deleted_and_a_write_already_made_come_back_when_a_later_deletion_fails(self):
        (self.folder / "old.bin").write_bytes(b"old")
        (self.folder / "first.bin").write_bytes(b"first")
        (self.folder / "second.bin").write_bytes(b"second")
        real_rename = os.rename
        calls = []

        def fail_the_second_deletion(source, target):
            calls.append(source)
            if len(calls) == 2:
                raise OSError("locked")
            return real_rename(source, target)

        with mock.patch.object(va.os, "rename", fail_the_second_deletion):
            with self.assertRaises(OSError):
                va.write_files_all_or_none(
                    [(self.folder / "old.bin", b"new")],
                    deletions=[self.folder / "first.bin", self.folder / "second.bin"],
                )
        self.assertEqual(len(calls), 3, "two deletions tried, and the first one put back")
        self.assertEqual((self.folder / "first.bin").read_bytes(), b"first")
        self.assertEqual((self.folder / "second.bin").read_bytes(), b"second")
        self.assertEqual((self.folder / "old.bin").read_bytes(), b"old")
        self.assertEqual(self.all_files(), ["first.bin", "old.bin", "second.bin"])


class PlaneConversionTests(unittest.TestCase):
    def test_channels_move_by_identity_and_the_rest_are_neutral_or_dropped(self):
        old = va.PBR_PROFILES[LABPBR]
        planes = numpy.zeros((2, 2, 8), dtype=numpy.uint8)
        planes[:] = [10, 20, 30, 40, 50, 60, 70, 80]
        new_planes, dropped = va.convert_planes(old, SPLIT, planes)
        self.assertTrue((new_planes == [10, 40, 0, 0, 80, 50, 0, 0]).all())
        self.assertEqual(dropped, ["reflectance", "porosity_scattering", "normal_y", "ambient_occlusion"])

    def test_a_channel_with_no_data_is_not_reported_as_dropped(self):
        old = va.PBR_PROFILES[LABPBR]
        _, dropped = va.convert_planes(old, SPLIT, va.neutral_planes(old, 2))
        self.assertEqual(dropped, [])

    def test_converting_there_and_back_gives_the_same_planes_when_nothing_was_dropped(self):
        planes = numpy.random.default_rng(5).integers(0, 256, size=(3, 3, 8), dtype=numpy.uint8)
        there, dropped = va.convert_planes(va.PBR_PROFILES[LABPBR], SWAPPED, planes)
        self.assertEqual(dropped, [])
        back, _ = va.convert_planes(SWAPPED, va.PBR_PROFILES[LABPBR], there)
        self.assertTrue(numpy.array_equal(back, planes))


class ScanTwoFormatsTests(SwitchFixture):
    def test_a_picture_with_files_in_the_format_of_the_setting_and_in_another_is_an_error_tile_naming_both(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_s.png": specular_picture(4, 1, 2, 3, 4), "dirt_spec.png": specular_picture(4, 5, 6, 7, 8)},
            profile=SPLIT.name,
        )
        error = project.tiles[(0, 0)].error
        self.assertIn("dirt_s.png", error)
        self.assertIn("dirt_spec.png", error)
        self.assertIn("two formats", error)

    def test_files_of_only_another_format_are_not_an_error_and_are_not_read(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_s.png": specular_picture(4, 1, 2, 3, 4)},
            profile=SPLIT.name,
        )
        record = project.tiles[(0, 0)]
        self.assertIsNone(record.error)
        self.assertTrue(numpy.array_equal(record.planes, va.neutral_planes(SPLIT, 4)))
        self.assertEqual([tile.name for tile in project.unplaced], ["dirt_s.png"])

    def test_under_none_nothing_is_a_second_format(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_s.png": specular_picture(4, 1, 2, 3, 4), "dirt_spec.png": specular_picture(4, 5, 6, 7, 8)},
            profile="none",
        )
        self.assertIsNone(project.tiles[(0, 0)].error)


class SwitchFromNoneTests(SwitchFixture):
    def test_tiles_named_as_companions_leave_the_atlas_with_a_warning_and_their_files_stay(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("dirt_n.png", normal_picture(4, 50, 60, 70, 80))},
            profile="none",
        )
        result = project.switch_pbr_profile(LABPBR)
        self.assertEqual(project.pbr_profile, LABPBR)
        self.assertEqual(self.read_setting(), LABPBR)
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "dirt.png"})
        self.assertEqual(result.ejected_names, ["dirt_n.png"])
        self.assertTrue(any("dirt_n.png" in warning for warning in result.warnings))
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png"])
        record = project.tiles[(0, 0)]
        self.assertTrue((record.planes[:, :, 4:] == [50, 60, 70, 80]).all())
        self.assertFalse(record.is_modified)
        self.assertEqual(project.unplaced, [])

    def test_a_companion_with_no_picture_becomes_an_unplaced_tile_with_blank_pixels(self):
        project = self.make_pbr_project(
            {(0, 0): ("ghost_n.png", normal_picture(4, 50, 60, 70, 80))}, profile="none"
        )
        project.switch_pbr_profile(LABPBR)
        self.assertEqual(project.tiles, {})
        self.assertEqual([(tile.name, tile.has_file) for tile in project.unplaced], [("ghost.png", False)])

    def test_nothing_is_renamed_or_rewritten_on_disk(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_s.png": specular_picture(4, 1, 2, 3, 4)},
            profile="none",
        )
        before = {name: (self.read(name).tobytes(), self.modified_time(name)) for name in self.file_names()}
        project.switch_pbr_profile(LABPBR)
        self.assertEqual(before, {name: (self.read(name).tobytes(), self.modified_time(name)) for name in self.file_names()})

    def test_the_selection_of_an_ejected_tile_is_dropped(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("dirt_n.png", normal_picture(4, 1, 2, 3, 4))},
            profile="none",
        )
        project.selection = {(0, 0), (1, 0)}
        project.switch_pbr_profile(LABPBR)
        self.assertEqual(project.selection, {(0, 0)})


class SwitchToNoneTests(SwitchFixture):
    def test_the_companion_files_stay_and_become_ordinary_unplaced_tiles(self):
        project = self.labpbr_dirt_and_lamp()
        project.switch_pbr_profile("none")
        self.assertEqual(project.pbr_profile, "none")
        self.assertEqual(self.read_setting(), "none")
        self.assertNotIn("pbr", json.loads((self.folder / "atlas.json").read_text(encoding="utf-8")))
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png", "lamp.png"])
        self.assertIsNone(project.tiles[(0, 0)].planes)
        self.assertEqual([tile.name for tile in project.unplaced], ["dirt_n.png", "dirt_s.png"])

    def test_switching_there_and_back_gives_the_same_tiles(self):
        project = self.labpbr_dirt_and_lamp()
        before = project.tiles[(0, 0)].planes.copy()
        project.switch_pbr_profile("none")
        project.switch_pbr_profile(LABPBR)
        self.assertTrue(numpy.array_equal(project.tiles[(0, 0)].planes, before))
        self.assertEqual(project.unplaced, [])


class SwitchRefusalTests(SwitchFixture):
    def test_nothing_is_switched_while_a_tile_has_unsaved_changes(self):
        project = self.labpbr_dirt_and_lamp()
        project.tiles[(0, 0)].planes[0, 0, 0] = 5
        plan = project.plan_pbr_switch("none")
        self.assertIn("unsaved", plan.problem)
        with self.assertRaises(ValueError):
            project.switch_pbr_profile("none")
        self.assertEqual(project.pbr_profile, LABPBR)
        self.assertEqual(self.read_setting(), LABPBR)

    def test_an_unknown_profile_is_refused(self):
        project = self.labpbr_dirt_and_lamp()
        with self.assertRaises(ValueError):
            project.switch_pbr_profile("labpbr")

    def test_the_setting_it_already_has_changes_nothing_and_says_so(self):
        project = self.labpbr_dirt_and_lamp()
        self.assertIsNone(project.switch_pbr_profile(LABPBR))

    def test_a_failure_to_write_atlas_json_changes_nothing(self):
        project = self.labpbr_dirt_and_lamp()
        with mock.patch.object(project, "save_atlas_file", side_effect=OSError("read only")):
            with self.assertRaises(OSError):
                project.switch_pbr_profile("none")
        self.assertEqual(project.pbr_profile, LABPBR)
        self.assertIsNotNone(project.tiles[(0, 0)].planes)
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png", "lamp.png"])


class SwitchBetweenFormatsTests(SwitchFixture):
    def test_the_files_of_a_tile_are_rewritten_in_the_new_format_and_the_old_ones_are_removed(self):
        project = self.labpbr_dirt_and_lamp()
        project.switch_pbr_profile(SPLIT.name, confirm_dropped=True)
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_bump.png", "dirt_spec.png", "lamp.png"])
        self.assertTrue((self.read("dirt_spec.png") == [10, 40, 0, 0]).all())
        self.assertTrue((self.read("dirt_bump.png") == [80, 50, 0, 0]).all())
        record = project.tiles[(0, 0)]
        self.assertFalse(record.is_modified)
        self.assertEqual(record.disk_suffixes, frozenset({"_spec", "_bump"}))
        self.assertEqual(project.existing_names, {"dirt.png", "dirt_bump.png", "dirt_spec.png", "lamp.png"})
        self.assertEqual(self.read_setting(), SPLIT.name)
        self.assertEqual(project.unplaced, [])

    def test_a_tile_with_no_companion_files_gets_none(self):
        project = self.labpbr_dirt_and_lamp()
        project.switch_pbr_profile(SPLIT.name, confirm_dropped=True)
        self.assertNotIn("lamp_spec.png", self.file_names())

    def test_the_plan_names_the_channels_that_would_be_lost_and_nothing_happens_without_a_confirm(self):
        project = self.labpbr_dirt_and_lamp()
        plan = project.plan_pbr_switch(SPLIT.name)
        self.assertIsNone(plan.problem)
        self.assertEqual(
            plan.dropped, {(0, 0): ["reflectance", "porosity_scattering", "normal_y", "ambient_occlusion"]}
        )
        before = {name: self.read(name).tobytes() for name in self.file_names()}
        with self.assertRaises(ValueError) as context:
            project.switch_pbr_profile(SPLIT.name)
        self.assertIn("reflectance", str(context.exception))
        self.assertEqual(project.pbr_profile, LABPBR)
        self.assertEqual(before, {name: self.read(name).tobytes() for name in self.file_names()})
        self.assertEqual(self.read_setting(), LABPBR)

    def test_a_tile_with_files_in_both_formats_is_an_error_tile_that_a_switch_does_not_touch(self):
        project = self.labpbr_dirt_and_lamp()
        va.write_png_atomically(self.folder / "dirt_spec.png", specular_picture(4, 5, 6, 7, 8))
        project = va.AtlasProject.open(self.folder)
        self.assertIn("two formats", project.tiles[(0, 0)].error)
        plan = project.plan_pbr_switch(SPLIT.name)
        self.assertEqual(plan.conversions, {})
        self.assertEqual(plan.dropped, {})
        before = {name: self.read(name).tobytes() for name in self.file_names()}
        result = project.switch_pbr_profile(SPLIT.name)
        self.assertEqual(self.read_setting(), SPLIT.name)
        self.assertIn("dirt_s.png", project.tiles[(0, 0)].error)
        self.assertIn("dirt_spec.png", project.tiles[(0, 0)].error)
        self.assertEqual(before, {name: self.read(name).tobytes() for name in self.file_names()})
        self.assertIsNone(project.tiles[(1, 0)].error)
        self.assertEqual(result.warnings, [])

    def test_that_error_tile_stays_one_when_the_setting_is_changed_again_until_the_files_are_sorted_out(self):
        project = self.labpbr_dirt_and_lamp()
        va.write_png_atomically(self.folder / "dirt_spec.png", specular_picture(4, 5, 6, 7, 8))
        project = va.AtlasProject.open(self.folder)
        project.switch_pbr_profile(SPLIT.name, confirm_dropped=True)
        project.switch_pbr_profile(LABPBR, confirm_dropped=True)
        self.assertIn("two formats", project.tiles[(0, 0)].error)
        os.unlink(self.folder / "dirt_spec.png")
        self.assertTrue(project.reload())
        self.assertIsNone(project.tiles[(0, 0)].error)
        self.assertTrue((project.tiles[(0, 0)].planes[:, :, :4] == [10, 20, 30, 40]).all())

    def test_a_tile_whose_files_cannot_all_be_changed_becomes_an_error_tile_with_its_files_as_they_were(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("lamp.png", solid_tile(RED))},
            companions={
                "dirt_s.png": specular_picture(4, 10, 20, 30, 40),
                "lamp_s.png": specular_picture(4, 1, 2, 3, 4),
            },
        )
        real_replace = os.replace
        failing = []

        def fail_for_dirt(source, target):
            if "dirt" in Path(target).name:
                failing.append(target)
                raise OSError("locked")
            return real_replace(source, target)

        with mock.patch.object(va.os, "replace", fail_for_dirt):
            result = project.switch_pbr_profile(SPLIT.name, confirm_dropped=True)
        self.assertTrue(failing)
        self.assertIn("locked", project.tiles[(0, 0)].error)
        self.assertEqual(len(result.warnings), 1)
        self.assertIn("dirt.png", result.warnings[0])
        self.assertIn("locked", result.warnings[0])
        self.assertIn("dirt_s.png", self.file_names())
        self.assertNotIn("dirt_spec.png", self.file_names())
        self.assertIsNone(project.tiles[(1, 0)].error)
        self.assertIn("lamp_spec.png", self.file_names())
        self.assertNotIn("lamp_s.png", self.file_names())

    def test_files_with_the_same_name_in_both_formats_are_rewritten_in_place_and_only_when_they_change(self):
        project = self.labpbr_dirt_and_lamp()
        time_of_normals = self.modified_time("dirt_n.png")
        project.switch_pbr_profile(SWAPPED.name)
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png", "lamp.png"])
        self.assertTrue((self.read("dirt_s.png") == [20, 10, 30, 40]).all())
        self.assertEqual(self.modified_time("dirt_n.png"), time_of_normals)
        planes = project.tiles[(0, 0)].planes
        self.assertTrue((planes[:, :, :2] == [10, 20]).all())
        self.assertFalse(project.tiles[(0, 0)].is_modified)

    def test_a_tile_with_only_new_format_files_is_just_loaded(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_spec.png": specular_picture(4, 10, 40, 0, 0)},
            profile=LABPBR,
        )
        self.assertEqual(project.tiles[(0, 0)].disk_suffixes, frozenset())
        before = self.modified_time("dirt_spec.png")
        project.switch_pbr_profile(SPLIT.name)
        self.assertEqual(self.modified_time("dirt_spec.png"), before)
        self.assertTrue((project.tiles[(0, 0)].planes[:, :, :2] == [10, 40]).all())


class ReloadTests(SwitchFixture):
    def test_reload_reads_the_folder_again(self):
        project = self.labpbr_dirt_and_lamp()
        va.write_png_atomically(self.folder / "lamp_n.png", normal_picture(4, 1, 2, 3, 4))
        va.write_png_atomically(self.folder / "stone.png", solid_tile(RED))
        self.assertTrue(project.reload())
        self.assertTrue((project.tiles[(1, 0)].planes[:, :, 4:] == [1, 2, 3, 4]).all())
        self.assertEqual([tile.name for tile in project.unplaced], ["stone.png"])
        self.assertIn("lamp_n.png", project.existing_names)

    def test_reload_reads_atlas_json_again_including_the_setting(self):
        project = self.labpbr_dirt_and_lamp()
        va.write_atlas_file(
            self.folder / "atlas.json", va.AtlasFile(4, {(0, 0): "dirt.png", (3, 3): "lamp.png"}, {}, {}, "none")
        )
        self.assertTrue(project.reload())
        self.assertEqual(project.pbr_profile, "none")
        self.assertEqual(set(project.tiles), {(0, 0), (3, 3)})
        self.assertIsNone(project.tiles[(0, 0)].planes)

    def test_reload_is_refused_while_a_tile_has_unsaved_changes(self):
        project = self.labpbr_dirt_and_lamp()
        project.tiles[(0, 0)].planes[0, 0, 0] = 9
        self.assertFalse(project.reload())
        self.assertEqual(project.tiles[(0, 0)].planes[0, 0, 0], 9)

    def test_reload_forgets_the_edit_history_of_the_tiles_it_replaces(self):
        project = self.labpbr_dirt_and_lamp()
        record = project.tiles[(0, 0)]
        project.history.record({record: (record.pixels, solid_tile((1, 2, 3, 255)))})
        self.assertTrue(project.history.can_undo([record]))
        project.reload()
        self.assertFalse(project.history.can_undo([record]), "the replaced tile's steps are not kept")
        self.assertIsNot(project.tiles[(0, 0)], record)


if __name__ == "__main__":
    unittest.main()
