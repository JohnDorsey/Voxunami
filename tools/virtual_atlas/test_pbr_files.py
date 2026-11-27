"""Tests for the PBR channels of VirtualAtlas: the profile table, the `pbr` setting in atlas.json, and how the companion
files (dirt_n.png, dirt_s.png) of a tile are read, kept, saved, renamed, purged and copied with it. No window.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import numpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_virtual_atlas import ProjectFixture, random_tile, solid_tile, va  # noqa: E402

LABPBR = "labpbr_1_3"
VECTORS = json.loads((Path(__file__).resolve().parent / "labpbr_channels.json").read_text(encoding="utf-8"))
RED = (255, 0, 0, 255)

# The order of the planes of a tile under LabPBR 1.3 (the order of the table in labpbr_channels.json).
SMOOTHNESS, REFLECTANCE, POROSITY, EMISSION, NORMAL_X, NORMAL_Y, OCCLUSION, HEIGHT = range(8)


def specular_picture(size, smoothness, reflectance, porosity, emission):
    return solid_tile((smoothness, reflectance, porosity, emission), size)


def normal_picture(size, normal_x, normal_y, occlusion, height):
    return solid_tile((normal_x, normal_y, occlusion, height), size)


def neutral_planes(size=4):
    """The planes of a tile that has no companion files, written out here and not taken from the program."""
    planes = numpy.zeros((size, size, 8), dtype=numpy.uint8)
    planes[:, :, EMISSION] = 255
    planes[:, :, NORMAL_X] = 128
    planes[:, :, NORMAL_Y] = 128
    planes[:, :, OCCLUSION] = 255
    planes[:, :, HEIGHT] = 255
    return planes


class ProfileTableTests(unittest.TestCase):
    def test_none_has_no_channels_and_no_companion_files(self):
        profile = va.PBR_PROFILES["none"]
        self.assertEqual(profile.channels, ())
        self.assertEqual(profile.suffixes, ())

    def test_labpbr_has_the_eight_channels_in_order_in_two_files(self):
        profile = va.PBR_PROFILES[LABPBR]
        self.assertEqual(
            [channel.identity for channel in profile.channels],
            ["smoothness", "reflectance", "porosity_scattering", "emission",
             "normal_x", "normal_y", "ambient_occlusion", "height"],
        )
        self.assertEqual(profile.suffixes, ("_s", "_n"))

    def test_the_table_equals_the_vector_file_the_game_is_also_checked_against(self):
        profile = va.PBR_PROFILES[LABPBR]
        self.assertEqual(VECTORS["profile"], LABPBR)
        table = [(channel.identity, channel.suffix, channel.byte, channel.neutral) for channel in profile.channels]
        vectors = [(row["identity"], row["file"], row["byte"], row["neutral"]) for row in VECTORS["channels"]]
        self.assertEqual(table, vectors)

    def test_the_pixel_of_a_picture_with_no_companion_is_the_one_in_the_vector_file(self):
        profile = va.PBR_PROFILES[LABPBR]
        pictures = va.companions_from_planes(profile, va.neutral_planes(profile, 3))
        self.assertEqual(set(pictures), set(VECTORS["neutral_pixels"]))
        for suffix, expected in VECTORS["neutral_pixels"].items():
            self.assertEqual(pictures[suffix].shape, (3, 3, 4))
            self.assertTrue((pictures[suffix] == numpy.array(expected, dtype=numpy.uint8)).all(), suffix)

    def test_every_byte_of_a_companion_file_belongs_to_exactly_one_channel(self):
        profile = va.PBR_PROFILES[LABPBR]
        owners = [(channel.suffix, channel.byte) for channel in profile.channels]
        self.assertEqual(len(owners), len(set(owners)))
        self.assertEqual(set(owners), {(suffix, byte) for suffix in profile.suffixes for byte in range(4)})

    def test_neutral_planes_have_the_neutral_of_each_channel(self):
        planes = va.neutral_planes(va.PBR_PROFILES[LABPBR], 4)
        self.assertEqual(planes.shape, (4, 4, 8))
        self.assertEqual(planes.dtype, numpy.uint8)
        self.assertTrue(numpy.array_equal(planes, neutral_planes(4)))

    def test_none_has_planes_of_no_channels(self):
        self.assertEqual(va.neutral_planes(va.PBR_PROFILES["none"], 4).shape, (4, 4, 0))

    def test_a_picture_sets_the_planes_of_its_bytes_and_no_others(self):
        profile = va.PBR_PROFILES[LABPBR]
        planes = va.planes_from_companions(profile, {"_s": specular_picture(4, 10, 20, 30, 40)})
        self.assertTrue((planes[:, :, [SMOOTHNESS, REFLECTANCE, POROSITY, EMISSION]] == [10, 20, 30, 40]).all())
        self.assertTrue(numpy.array_equal(planes[:, :, 4:], neutral_planes(4)[:, :, 4:]))
        planes = va.planes_from_companions(profile, {"_n": normal_picture(4, 50, 60, 70, 80)})
        self.assertTrue((planes[:, :, [NORMAL_X, NORMAL_Y, OCCLUSION, HEIGHT]] == [50, 60, 70, 80]).all())
        self.assertTrue(numpy.array_equal(planes[:, :, :4], neutral_planes(4)[:, :, :4]))

    def test_planes_become_pictures_and_back_without_loss(self):
        profile = va.PBR_PROFILES[LABPBR]
        planes = numpy.random.default_rng(3).integers(0, 256, size=(5, 5, 8), dtype=numpy.uint8)
        pictures = va.companions_from_planes(profile, planes)
        self.assertEqual(pictures["_s"][2, 3, 3], planes[2, 3, EMISSION])
        self.assertEqual(pictures["_n"][4, 1, 0], planes[4, 1, NORMAL_X])
        self.assertTrue(numpy.array_equal(va.planes_from_companions(profile, pictures), planes))


class CompanionNameTests(unittest.TestCase):
    def test_a_companion_is_named_by_adding_its_ending_before_the_extension(self):
        self.assertEqual(va.companion_name("dirt.png", "_n"), "dirt_n.png")
        self.assertEqual(va.companion_name("Dirt.PNG", "_s"), "Dirt_s.png")
        self.assertEqual(va.companion_name("dirt (copy).png", "_n"), "dirt (copy)_n.png")

    def test_under_labpbr_names_ending_in_s_or_n_are_companions_of_the_rest(self):
        profile = va.PBR_PROFILES[LABPBR]
        self.assertEqual(va.split_companion_name("dirt_n.png", profile), ("dirt.png", "_n"))
        self.assertEqual(va.split_companion_name("lamp_s.png", profile), ("lamp.png", "_s"))
        self.assertEqual(va.split_companion_name("Lamp_S.PNG", profile), ("Lamp.png", "_s"))
        self.assertEqual(va.split_companion_name("a_n_n.png", profile), ("a_n.png", "_n"))

    def test_other_names_are_not_companions(self):
        profile = va.PBR_PROFILES[LABPBR]
        for name in ["dirt.png", "sand.png", "_n.png", "_s.png", "dirt_ns.png", "dirt_n.jpg", "dirt_x.png", "ns.png"]:
            self.assertIsNone(va.split_companion_name(name, profile), name)

    def test_under_none_nothing_is_a_companion(self):
        self.assertIsNone(va.split_companion_name("dirt_n.png", va.PBR_PROFILES["none"]))

    def test_the_reserved_endings_get_a_warning_under_any_setting(self):
        for name in ["dirt_n.png", "lamp_s.png", "Lamp_S.PNG"]:
            warning = va.reserved_ending_warning(name)
            self.assertIsNotNone(warning, name)
            self.assertIn("the game", warning)
        for name in ["dirt.png", "sand.png", "ns.png", "_n.png"]:
            self.assertIsNone(va.reserved_ending_warning(name), name)


class AtlasFilePbrTests(unittest.TestCase):
    def test_a_file_without_the_setting_is_none(self):
        self.assertEqual(va.parse_atlas_file("{}").pbr_profile, "none")
        self.assertEqual(va.AtlasFile(4).pbr_profile, "none")

    def test_the_setting_is_read_and_is_not_an_unknown_setting(self):
        atlas_file = va.parse_atlas_file('{"pbr": "labpbr_1_3", "other": 1}')
        self.assertEqual(atlas_file.pbr_profile, LABPBR)
        self.assertEqual(atlas_file.other_settings, {"other": 1})

    def test_none_is_not_written_so_a_file_without_the_setting_is_rewritten_as_it_was(self):
        text = va.serialize_atlas_file(va.parse_atlas_file('{"tile_size": 4}'))
        self.assertNotIn("pbr", text)

    def test_a_setting_other_than_none_is_written_and_read_back(self):
        atlas_file = va.AtlasFile(4, {(0, 0): "a.png"}, {}, {"keep": [1]}, LABPBR)
        text = va.serialize_atlas_file(atlas_file)
        self.assertEqual(json.loads(text)["pbr"], LABPBR)
        again = va.parse_atlas_file(text)
        self.assertEqual((again.pbr_profile, again.other_settings), (LABPBR, {"keep": [1]}))

    def test_an_unknown_or_wrongly_typed_setting_is_refused_naming_the_choices(self):
        for text in ['{"pbr": "labpbr"}', '{"pbr": 3}', '{"pbr": null}', '{"pbr": ""}']:
            with self.assertRaises(va.AtlasFileError, msg=text) as context:
                va.parse_atlas_file(text)
            self.assertTrue(any("pbr" in problem and LABPBR in problem for problem in context.exception.problems))


class ScanCompanionTests(unittest.TestCase):
    def setUp(self):
        self.temporary_folder = tempfile.TemporaryDirectory()
        self.folder = Path(self.temporary_folder.name)

    def tearDown(self):
        self.temporary_folder.cleanup()

    def put(self, name, pixels):
        va.write_png_atomically(self.folder / name, pixels)

    def scan(self, names=None, data=None, profile=LABPBR, tile_size=4):
        return va.scan_tile_folder(self.folder, va.AtlasFile(tile_size, names or {}, data or {}, {}, profile), tile_size)

    def test_a_tile_with_no_companion_files_has_neutral_planes_and_is_clean(self):
        self.put("dirt.png", solid_tile(RED))
        record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        self.assertTrue(numpy.array_equal(record.planes, neutral_planes()))
        self.assertTrue(numpy.array_equal(record.disk_planes, neutral_planes()))
        self.assertEqual(record.disk_suffixes, frozenset())
        self.assertFalse(record.is_modified)
        self.assertTrue(record.can_return_to_unplaced)

    def test_the_companion_files_of_a_tile_give_its_planes(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_s.png", specular_picture(4, 10, 20, 30, 40))
        self.put("dirt_n.png", normal_picture(4, 50, 60, 70, 80))
        scan = self.scan({(0, 0): "dirt.png"})
        record = scan.tiles[(0, 0)]
        self.assertIsNone(record.error)
        self.assertTrue((record.planes == [10, 20, 30, 40, 50, 60, 70, 80]).all())
        self.assertTrue(numpy.array_equal(record.planes, record.disk_planes))
        self.assertEqual(record.disk_suffixes, frozenset({"_s", "_n"}))
        self.assertFalse(record.is_modified)
        self.assertEqual(scan.unplaced, [])
        self.assertEqual(scan.existing_names, {"dirt.png", "dirt_s.png", "dirt_n.png"})

    def test_one_companion_file_leaves_the_channels_of_the_other_neutral(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_n.png", normal_picture(4, 50, 60, 70, 80))
        record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        self.assertTrue(numpy.array_equal(record.planes[:, :, :4], neutral_planes()[:, :, :4]))
        self.assertTrue((record.planes[:, :, 4:] == [50, 60, 70, 80]).all())
        self.assertEqual(record.disk_suffixes, frozenset({"_n"}))

    def test_companion_files_are_found_ignoring_letter_case(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("DIRT_N.PNG", normal_picture(4, 50, 60, 70, 80))
        record = self.scan({(0, 0): "Dirt.png"}).tiles[(0, 0)]
        self.assertTrue((record.planes[:, :, 4:] == [50, 60, 70, 80]).all())

    def test_a_changed_plane_makes_a_tile_modified_and_changing_it_back_does_not(self):
        self.put("dirt.png", solid_tile(RED))
        record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        record.planes[1, 2, HEIGHT] = 7
        self.assertTrue(record.is_modified)
        self.assertFalse(record.can_return_to_unplaced)
        record.planes[1, 2, HEIGHT] = 255
        self.assertFalse(record.is_modified)

    def test_with_no_file_a_neutral_plane_is_the_same_as_the_file(self):
        self.put("dirt.png", solid_tile(RED))
        record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        record.planes[:] = neutral_planes()
        self.assertFalse(record.is_modified)

    def test_under_none_companion_names_are_ordinary_tiles_and_planes_do_not_exist(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_n.png", normal_picture(4, 50, 60, 70, 80))
        scan = self.scan({(0, 0): "dirt.png"}, profile="none")
        self.assertIsNone(scan.tiles[(0, 0)].planes)
        self.assertEqual([tile.name for tile in scan.unplaced], ["dirt_n.png"])
        self.assertFalse(scan.tiles[(0, 0)].is_modified)

    def test_a_companion_of_the_wrong_size_makes_its_tile_an_error_naming_the_file(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_n.png", normal_picture(8, 50, 60, 70, 80))
        with self.assertLogs("VirtualAtlas", level="ERROR") as logs:
            record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        self.assertIn("dirt_n.png", record.error)
        self.assertIn("8x8", record.error)
        self.assertTrue(any("dirt_n.png" in line for line in logs.output))
        self.assertFalse(record.is_modified)

    def test_a_companion_that_cannot_be_read_makes_its_tile_an_error_naming_the_file(self):
        self.put("dirt.png", solid_tile(RED))
        (self.folder / "dirt_s.png").write_bytes(b"junk")
        record = self.scan({(0, 0): "dirt.png"}).tiles[(0, 0)]
        self.assertIn("dirt_s.png", record.error)
        self.assertIn("cannot be read", record.error)

    def test_a_listed_tile_that_is_itself_a_companion_name_is_an_error_tile_and_both_files_stay(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_n.png", normal_picture(4, 50, 60, 70, 80))
        scan = self.scan({(0, 0): "dirt.png", (1, 0): "dirt_n.png"})
        self.assertIn("companion of dirt.png", scan.tiles[(1, 0)].error)
        self.assertIsNone(scan.tiles[(0, 0)].error)
        self.assertTrue((scan.tiles[(0, 0)].planes[:, :, 4:] == [50, 60, 70, 80]).all())

    def test_an_unlisted_picture_with_companions_is_unplaced_with_its_planes_and_the_companions_are_not_unplaced(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_s.png", specular_picture(4, 10, 20, 30, 40))
        scan = self.scan()
        self.assertEqual([tile.name for tile in scan.unplaced], ["dirt.png"])
        tile = scan.unplaced[0]
        self.assertTrue(tile.has_file)
        self.assertIsNone(tile.companion_error)
        self.assertTrue((tile.planes[:, :, :4] == [10, 20, 30, 40]).all())
        self.assertEqual(tile.disk_suffixes, frozenset({"_s"}))

    def test_a_companion_with_no_picture_is_an_unplaced_tile_with_blank_pixels_and_no_file(self):
        self.put("ghost_n.png", normal_picture(4, 50, 60, 70, 80))
        scan = self.scan()
        self.assertEqual([tile.name for tile in scan.unplaced], ["ghost.png"])
        tile = scan.unplaced[0]
        self.assertFalse(tile.has_file)
        self.assertTrue((tile.pixels == 0).all())
        self.assertTrue((tile.planes[:, :, 4:] == [50, 60, 70, 80]).all())
        self.assertEqual(scan.existing_names, {"ghost_n.png"})

    def test_a_listed_picture_with_no_file_but_companions_is_a_new_tile_with_blank_pixels(self):
        self.put("ghost_s.png", specular_picture(4, 10, 20, 30, 40))
        record = self.scan({(0, 0): "ghost.png"}).tiles[(0, 0)]
        self.assertIsNone(record.error)
        self.assertTrue(record.is_new)
        self.assertTrue(record.is_modified)
        self.assertTrue((record.pixels == 0).all())
        self.assertTrue((record.planes[:, :, :4] == [10, 20, 30, 40]).all())
        self.assertTrue(numpy.array_equal(record.planes, record.disk_planes))
        self.assertEqual(record.disk_suffixes, frozenset({"_s"}))

    def test_a_listed_picture_with_stored_data_and_companions_keeps_both(self):
        pixels = random_tile(3)
        self.put("ghost_s.png", specular_picture(4, 10, 20, 30, 40))
        record = self.scan({(0, 0): "ghost.png"}, {(0, 0): va.encode_tile_data(pixels)}).tiles[(0, 0)]
        self.assertTrue(numpy.array_equal(record.pixels, pixels))
        self.assertTrue((record.planes[:, :, :4] == [10, 20, 30, 40]).all())

    def test_an_unplaced_picture_with_a_bad_companion_is_still_listed_and_says_why_it_cannot_be_placed(self):
        self.put("dirt.png", solid_tile(RED))
        self.put("dirt_n.png", normal_picture(8, 50, 60, 70, 80))
        tile = self.scan().unplaced[0]
        self.assertEqual(tile.name, "dirt.png")
        self.assertIn("dirt_n.png", tile.companion_error)

    def test_a_companion_of_the_wrong_size_is_ignored_when_nothing_lists_or_shows_its_picture(self):
        self.put("ghost_n.png", normal_picture(8, 50, 60, 70, 80))
        with self.assertNoLogs("VirtualAtlas", level="ERROR"):
            scan = self.scan()
        self.assertEqual(scan.unplaced, [])


class PbrProjectFixture(ProjectFixture):
    def make_pbr_project(self, placed=None, unplaced=(), companions=None, profile=LABPBR, data=None):
        """placed: {coordinate: (name, pixels or None)}; companions: {file name: pixels}, written beside them."""
        names = {}
        for coordinate, (name, pixels) in (placed or {}).items():
            names[coordinate] = name
            if pixels is not None:
                va.write_png_atomically(self.folder / name, pixels)
        for name, pixels in unplaced:
            va.write_png_atomically(self.folder / name, pixels)
        for name, pixels in (companions or {}).items():
            va.write_png_atomically(self.folder / name, pixels)
        va.write_atlas_file(
            self.folder / "atlas.json", va.AtlasFile(self.tile_size, names, dict(data or {}), {}, profile)
        )
        return va.AtlasProject.open(self.folder)

    def read(self, name):
        return va.read_image_pixels(self.folder / name)

    def file_names(self):
        return sorted(entry.name for entry in os.scandir(self.folder) if entry.name.endswith(".png"))

    def modified_time(self, name):
        return os.stat(self.folder / name).st_mtime_ns


class ProjectProfileTests(PbrProjectFixture):
    def test_the_project_knows_its_profile_and_writes_it_back_with_every_change(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        self.assertEqual(project.pbr_profile, LABPBR)
        project.create_tile((1, 0))
        self.assertEqual(self.saved().pbr_profile, LABPBR)

    def test_a_project_without_the_setting_is_none_and_stays_without_it(self):
        project = self.make_project({(0, 0): ("dirt.png", solid_tile(RED))})
        self.assertEqual(project.pbr_profile, "none")
        project.create_tile((1, 0))
        self.assertNotIn("pbr", json.loads((self.folder / "atlas.json").read_text(encoding="utf-8")))

    def test_new_and_imported_tiles_start_with_neutral_planes(self):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        self.assertTrue(numpy.array_equal(project.tiles[(0, 0)].planes, neutral_planes()))
        self.assertTrue(numpy.array_equal(project.tiles[(0, 0)].disk_planes, neutral_planes()))
        other = self.make_pbr_project()
        other.import_tiles({(0, 0): solid_tile(RED)})
        self.assertTrue(numpy.array_equal(other.tiles[(0, 0)].planes, neutral_planes()))

    def test_under_none_new_tiles_have_no_planes(self):
        project = self.make_pbr_project(profile="none")
        project.create_tile((0, 0))
        self.assertIsNone(project.tiles[(0, 0)].planes)

    def test_a_tile_with_changed_planes_is_listed_as_unsaved(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        self.assertEqual(project.unsaved_tiles(), [])
        project.tiles[(0, 0)].planes[0, 0, SMOOTHNESS] = 9
        self.assertEqual(project.unsaved_tiles(), [(0, 0)])


class PlaceAndReturnTests(PbrProjectFixture):
    def test_placing_an_unplaced_picture_brings_its_planes_and_returning_it_takes_them_back(self):
        project = self.make_pbr_project(
            unplaced=[("dirt.png", solid_tile(RED))], companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        self.assertTrue(project.place_unplaced_tile(0, (0, 0)))
        record = project.tiles[(0, 0)]
        self.assertTrue((record.planes[:, :, 4:] == [50, 60, 70, 80]).all())
        self.assertEqual(record.disk_suffixes, frozenset({"_n"}))
        self.assertFalse(record.is_modified)
        self.assertTrue(project.return_to_unplaced([(0, 0)]))
        tile = project.unplaced[0]
        self.assertTrue((tile.planes[:, :, 4:] == [50, 60, 70, 80]).all())
        self.assertEqual(tile.disk_suffixes, frozenset({"_n"}))

    def test_placing_a_companion_with_no_picture_makes_a_new_tile_kept_in_atlas_json_until_saved(self):
        project = self.make_pbr_project(companions={"ghost_n.png": normal_picture(4, 50, 60, 70, 80)})
        self.assertTrue(project.place_unplaced_tile(0, (0, 0)))
        record = project.tiles[(0, 0)]
        self.assertEqual(record.name, "ghost.png")
        self.assertTrue(record.is_new)
        self.assertEqual(self.saved().coordinates_to_data.keys(), {(0, 0)})
        project.save_tile((0, 0))
        self.assertTrue((self.read("ghost.png") == 0).all())
        self.assertFalse(project.tiles[(0, 0)].is_modified)
        self.assertEqual(self.saved().coordinates_to_data, {})

    def test_placing_a_picture_whose_companion_is_bad_gives_an_error_tile_that_changes_no_file(self):
        project = self.make_pbr_project(
            unplaced=[("dirt.png", solid_tile(RED))], companions={"dirt_n.png": normal_picture(8, 1, 2, 3, 4)}
        )
        before = {name: self.read(name).tobytes() for name in self.file_names()}
        self.assertTrue(project.place_unplaced_tile(0, (0, 0)))
        self.assertIn("dirt_n.png", project.tiles[(0, 0)].error)
        self.assertEqual(before, {name: self.read(name).tobytes() for name in self.file_names()})


class SaveCompanionTests(PbrProjectFixture):
    def test_saving_creates_a_companion_file_only_for_a_file_with_a_channel_that_is_not_neutral(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        project.tiles[(0, 0)].planes[:, :, NORMAL_X] = 200
        project.save_tile((0, 0))
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png"])
        self.assertTrue((self.read("dirt_n.png") == [200, 128, 255, 255]).all())
        record = project.tiles[(0, 0)]
        self.assertFalse(record.is_modified)
        self.assertEqual(record.disk_suffixes, frozenset({"_n"}))

    def test_a_tile_whose_channels_are_all_neutral_creates_no_companion_file(self):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        project.save_tile((0, 0))
        self.assertEqual(self.file_names(), ["new_tile.png"])

    def test_a_tile_painted_on_both_files_writes_both_with_neutral_bytes_where_nothing_was_painted(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        project.tiles[(0, 0)].planes[:, :, EMISSION] = 100
        project.tiles[(0, 0)].planes[:, :, HEIGHT] = 3
        project.save_tile((0, 0))
        self.assertTrue((self.read("dirt_s.png") == [0, 0, 0, 100]).all())
        self.assertTrue((self.read("dirt_n.png") == [128, 128, 255, 3]).all())

    def test_a_file_that_exists_is_rewritten_even_when_the_change_makes_it_neutral(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        project.tiles[(0, 0)].planes[:, :, 4:] = neutral_planes()[:, :, 4:]
        project.save_tile((0, 0))
        self.assertTrue((self.read("dirt_n.png") == [128, 128, 255, 255]).all())

    def test_only_the_files_that_changed_are_rewritten(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80), "dirt_s.png": specular_picture(4, 1, 2, 3, 4)},
        )
        times = {name: self.modified_time(name) for name in self.file_names()}
        project.tiles[(0, 0)].planes[:, :, REFLECTANCE] = 99
        project.save_tile((0, 0))
        self.assertEqual(self.modified_time("dirt.png"), times["dirt.png"])
        self.assertEqual(self.modified_time("dirt_n.png"), times["dirt_n.png"])
        self.assertTrue((self.read("dirt_s.png") == [1, 99, 3, 4]).all())

    def test_a_change_to_the_pixels_alone_leaves_the_companion_files_alone(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        time = self.modified_time("dirt_n.png")
        project._replace_pixels({project.tiles[(0, 0)]: solid_tile((1, 2, 3, 255))})
        project.save_tile((0, 0))
        self.assertEqual(self.modified_time("dirt_n.png"), time)
        self.assertTrue((self.read("dirt.png") == [1, 2, 3, 255]).all())

    def test_a_new_tile_is_saved_with_its_companions_and_leaves_atlas_json(self):
        project = self.make_pbr_project()
        project.create_tile((0, 0))
        project.tiles[(0, 0)].planes[:, :, SMOOTHNESS] = 77
        project.save_tile((0, 0))
        self.assertEqual(self.file_names(), ["new_tile.png", "new_tile_s.png"])
        self.assertTrue((self.read("new_tile_s.png") == [77, 0, 0, 255]).all())
        self.assertEqual(self.saved().coordinates_to_data, {})

    def test_if_one_file_cannot_be_written_none_of_the_files_of_the_tile_change(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80), "dirt_s.png": specular_picture(4, 1, 2, 3, 4)},
        )
        before = {name: self.read(name).tobytes() for name in self.file_names()}
        record = project.tiles[(0, 0)]
        project._replace_pixels({record: solid_tile((9, 9, 9, 255))})
        record.planes[:, :, NORMAL_X] = 1
        record.planes[:, :, SMOOTHNESS] = 2
        real_replace = os.replace
        calls = []

        def replace_but_fail_the_third(source, target):
            calls.append(target)
            if len(calls) == 3:
                raise OSError("disk full")
            return real_replace(source, target)

        with mock.patch.object(va.os, "replace", replace_but_fail_the_third):
            with self.assertRaises(OSError):
                project.save_tile((0, 0))
        self.assertGreaterEqual(len(calls), 3)  # the third write failed; putting the files back may replace more
        self.assertEqual(before,{name: self.read(name).tobytes() for name in self.file_names()})
        self.assertEqual([name for name in os.listdir(self.folder) if name.startswith(va.TEMPORARY_FILE_PREFIX)], [])
        self.assertTrue(record.is_modified)
        self.assertTrue((record.disk_pixels == RED).all())
        self.assertFalse((record.disk_planes[:, :, NORMAL_X] == 1).any())

    def test_a_new_file_that_was_written_before_a_failure_is_removed_again(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        record = project.tiles[(0, 0)]
        record.planes[:, :, NORMAL_X] = 1
        record.planes[:, :, SMOOTHNESS] = 2
        real_replace = os.replace
        calls = []

        def replace_but_fail_the_second(source, target):
            calls.append(target)
            if len(calls) == 2:
                raise OSError("disk full")
            return real_replace(source, target)

        with mock.patch.object(va.os, "replace", replace_but_fail_the_second):
            with self.assertRaises(OSError):
                project.save_tile((0, 0))
        self.assertEqual(self.file_names(), ["dirt.png"])


class WriteTogetherTests(unittest.TestCase):
    def test_all_files_are_written_or_none(self):
        with tempfile.TemporaryDirectory() as name:
            folder = Path(name)
            (folder / "old.bin").write_bytes(b"old")
            real_replace = os.replace
            calls = []

            def fail_the_second(source, target):
                calls.append(target)
                if len(calls) == 2:
                    raise OSError("no")
                return real_replace(source, target)

            writes = [(folder / "old.bin", b"new"), (folder / "fresh.bin", b"fresh")]
            with mock.patch.object(va.os, "replace", fail_the_second):
                with self.assertRaises(OSError):
                    va.write_files_all_or_none(writes)
            self.assertEqual((folder / "old.bin").read_bytes(), b"old")
            self.assertFalse((folder / "fresh.bin").exists())
            self.assertEqual(sorted(os.listdir(folder)), ["old.bin"])
            va.write_files_all_or_none(writes)
            self.assertEqual((folder / "old.bin").read_bytes(), b"new")
            self.assertEqual((folder / "fresh.bin").read_bytes(), b"fresh")


class RenameCompanionTests(PbrProjectFixture):
    def make(self):
        return self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))},
            companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80), "dirt_s.png": specular_picture(4, 1, 2, 3, 4)},
        )

    def test_renaming_a_tile_renames_all_its_files(self):
        project = self.make()
        self.assertTrue(project.rename_tile((0, 0), "mud.png"))
        self.assertEqual(self.file_names(), ["mud.png", "mud_n.png", "mud_s.png"])
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "mud.png"})
        self.assertTrue((self.read("mud_n.png") == [50, 60, 70, 80]).all())
        self.assertFalse(project.tiles[(0, 0)].is_modified)
        self.assertEqual(project.existing_names, {"mud.png", "mud_n.png", "mud_s.png"})

    def test_a_companion_name_that_is_taken_refuses_the_rename_and_changes_nothing(self):
        project = self.make()
        va.write_png_atomically(self.folder / "mud_n.png", normal_picture(4, 1, 1, 1, 1))
        project = va.AtlasProject.open(self.folder)
        problem = project.rename_problem((0, 0), "mud.png")
        self.assertIn("mud_n.png", problem)
        with self.assertRaises(ValueError):
            project.rename_tile((0, 0), "mud.png")
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png", "mud_n.png"])

    def test_a_rename_that_fails_part_way_gives_every_file_its_name_back(self):
        project = self.make()
        real_rename = os.rename
        calls = []

        def fail_the_third(source, target):
            calls.append(target)
            if len(calls) == 3:
                raise OSError("locked")
            return real_rename(source, target)

        with mock.patch.object(va.os, "rename", fail_the_third):
            with self.assertRaises(OSError):
                project.rename_tile((0, 0), "mud.png")
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png"])
        self.assertEqual(project.tiles[(0, 0)].name, "dirt.png")
        self.assertEqual(self.saved().coordinates_to_names, {(0, 0): "dirt.png"})

    def test_a_rename_whose_atlas_json_cannot_be_written_gives_every_file_its_name_back(self):
        project = self.make()
        with mock.patch.object(project, "save_atlas_file", side_effect=OSError("read only")):
            with self.assertRaises(OSError):
                project.rename_tile((0, 0), "mud.png")
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png", "dirt_s.png"])

    def test_a_tile_with_companions_but_no_picture_renames_its_companions(self):
        project = self.make_pbr_project(
            {(0, 0): ("ghost.png", None)}, companions={"ghost_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        project.rename_tile((0, 0), "spirit.png")
        self.assertEqual(self.file_names(), ["spirit_n.png"])

    def test_a_name_that_another_tiles_companion_has_is_taken(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("lamp.png", solid_tile(RED))},
            companions={"stone_n.png": normal_picture(4, 1, 2, 3, 4)},
        )
        self.assertIn("stone", project.rename_problem((0, 0), "stone.png"))


class PurgeCompanionTests(PbrProjectFixture):
    def test_purging_a_tile_deletes_its_companion_files_too(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("lamp.png", solid_tile(RED))},
            companions={
                "dirt_n.png": normal_picture(4, 50, 60, 70, 80),
                "dirt_s.png": specular_picture(4, 1, 2, 3, 4),
                "lamp_s.png": specular_picture(4, 5, 6, 7, 8),
            },
        )
        project.purge_tiles([(0, 0)])
        self.assertEqual(self.file_names(), ["lamp.png", "lamp_s.png"])
        self.assertEqual(project.existing_names, {"lamp.png", "lamp_s.png"})

    def test_a_purge_that_fails_gives_the_companion_files_back(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        with mock.patch.object(project, "save_atlas_file", side_effect=OSError("read only")):
            with self.assertRaises(OSError):
                project.purge_tiles([(0, 0)])
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png"])
        self.assertTrue((self.read("dirt_n.png") == [50, 60, 70, 80]).all())

    def test_purging_a_tile_with_no_picture_deletes_its_companions(self):
        project = self.make_pbr_project(
            {(0, 0): ("ghost.png", None)}, companions={"ghost_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        project.purge_tiles([(0, 0)])
        self.assertEqual(self.file_names(), [])


class DuplicateCompanionTests(PbrProjectFixture):
    def test_a_copy_gets_copies_of_the_companion_files_with_the_channels_the_tile_shows(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        project.select_only((0, 0))
        project.tiles[(0, 0)].planes[:, :, SMOOTHNESS] = 33
        self.assertTrue(project.duplicate_selection((0, 0), (1, 0)))
        self.assertEqual(self.file_names(), ["dirt (copy).png", "dirt (copy)_n.png", "dirt (copy)_s.png", "dirt.png", "dirt_n.png"])
        self.assertTrue((self.read("dirt (copy)_n.png") == [50, 60, 70, 80]).all())
        self.assertTrue((self.read("dirt (copy)_s.png") == [33, 0, 0, 255]).all())
        copy = project.tiles[(1, 0)]
        self.assertFalse(copy.is_modified)
        self.assertEqual(copy.disk_suffixes, frozenset({"_n", "_s"}))
        self.assertTrue(project.tiles[(0, 0)].is_modified)

    def test_a_tile_with_neutral_channels_is_copied_without_companion_files(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))})
        project.select_only((0, 0))
        project.duplicate_selection((0, 0), (1, 0))
        self.assertEqual(self.file_names(), ["dirt (copy).png", "dirt.png"])

    def test_a_copy_does_not_take_a_name_whose_companion_file_exists(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt (copy)_n.png": normal_picture(4, 1, 2, 3, 4)}
        )
        project.select_only((0, 0))
        project.duplicate_selection((0, 0), (1, 0))
        self.assertEqual(project.tiles[(1, 0)].name, "dirt (copy 2).png")

    def test_a_copy_that_fails_while_writing_companions_leaves_no_file_behind(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
        )
        project.select_only((0, 0))
        with mock.patch.object(project, "save_atlas_file", side_effect=OSError("read only")):
            with self.assertRaises(OSError):
                project.duplicate_selection((0, 0), (1, 0))
        self.assertEqual(self.file_names(), ["dirt.png", "dirt_n.png"])
        self.assertNotIn((1, 0), project.tiles)


class NewTileNameTests(PbrProjectFixture):
    def test_a_new_tile_does_not_take_a_name_whose_companion_file_exists(self):
        project = self.make_pbr_project(companions={"new_tile_n.png": normal_picture(4, 1, 2, 3, 4)})
        self.assertEqual(project.create_tile((0, 0)), "new_tile_2.png")


if __name__ == "__main__":
    unittest.main()
