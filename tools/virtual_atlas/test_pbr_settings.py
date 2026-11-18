"""Tests for the settings window of VirtualAtlas (the PBR setting and reading the folder again), the gear button that opens
it, and the warning for names that the game reads as companion pictures.

Run from the repository root:  python -m unittest discover -s tools/virtual_atlas -v
"""

import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_pbr_files import (  # noqa: E402
    LABPBR,
    SMOOTHNESS,
    PbrProjectFixture,
    normal_picture,
    solid_tile,
)
from test_virtual_atlas import BLUE, EditorDriver, RED, va  # noqa: E402

SMALL = va.PbrProfile(
    "test_small",
    (
        va.PbrChannel("smoothness", "_s", 0, 0),
        va.PbrChannel("reflectance", "_s", 1, 0),
        va.PbrChannel("porosity_scattering", "_s", 2, 0),
        va.PbrChannel("emission", "_s", 3, 255),
    ),
)


class SettingsDialogTests(PbrProjectFixture):
    def project_with(self, placed=None, profile=LABPBR, **keywords):
        return self.make_pbr_project(placed, profile=profile, **keywords)

    def test_the_dialog_starts_on_the_current_setting_and_has_nothing_to_apply(self):
        project = self.project_with()
        dialog = va.SettingsDialog(project)
        self.assertEqual(dialog.choice, LABPBR)
        self.assertFalse(dialog.can_apply)
        self.assertEqual(dialog.profile_names, list(va.PBR_PROFILES))

    def test_choosing_another_setting_can_be_applied_and_choosing_the_current_again_cannot(self):
        dialog = va.SettingsDialog(self.project_with())
        dialog.choose("none")
        self.assertTrue(dialog.can_apply)
        dialog.choose(LABPBR)
        self.assertFalse(dialog.can_apply)
        with self.assertRaises(ValueError):
            dialog.choose("labpbr_9")

    def test_applying_changes_the_setting_and_says_so(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))}, profile="none")
        dialog = va.SettingsDialog(project)
        dialog.choose(LABPBR)
        self.assertEqual(dialog.apply(), "done")
        self.assertEqual(project.pbr_profile, LABPBR)
        self.assertEqual(self.saved().pbr_profile, LABPBR)
        self.assertIn(LABPBR, dialog.done_message)
        self.assertIn("cannot be undone", dialog.note)

    def test_unsaved_tiles_stop_the_change_and_the_dialog_says_why(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))})
        project.tiles[(0, 0)].planes[0, 0, SMOOTHNESS] = 9
        dialog = va.SettingsDialog(project)
        dialog.choose("none")
        self.assertEqual(dialog.apply(), "ask")
        self.assertIn("unsaved", dialog.error)
        self.assertEqual(project.pbr_profile, LABPBR)

    def test_channels_with_data_that_the_new_setting_cannot_hold_need_a_confirm(self):
        with mock.patch.dict(va.PBR_PROFILES, {"test_small": SMALL}):
            project = self.project_with(
                {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
            )
            dialog = va.SettingsDialog(project)
            dialog.choose("test_small")
            self.assertEqual(dialog.apply(), "ask")
            self.assertEqual(dialog.stage, "confirm")
            self.assertIn("dirt.png", dialog.lost_text)
            self.assertIn("normal_x", dialog.lost_text)
            self.assertEqual(project.pbr_profile, LABPBR, "nothing has changed yet")
            self.assertEqual(dialog.apply(), "done")
            self.assertEqual(project.pbr_profile, "test_small")

    def test_back_from_the_confirm_returns_to_the_choice_and_keeps_it(self):
        with mock.patch.dict(va.PBR_PROFILES, {"test_small": SMALL}):
            project = self.project_with(
                {(0, 0): ("dirt.png", solid_tile(RED))}, companions={"dirt_n.png": normal_picture(4, 50, 60, 70, 80)}
            )
            dialog = va.SettingsDialog(project)
            dialog.choose("test_small")
            dialog.apply()
            dialog.back()
            self.assertEqual((dialog.stage, dialog.choice), ("choose", "test_small"))
            self.assertEqual(project.pbr_profile, LABPBR)

    def test_a_file_that_cannot_be_written_leaves_the_dialog_open_with_the_reason(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))}, profile="none")
        dialog = va.SettingsDialog(project)
        dialog.choose(LABPBR)
        with mock.patch.object(project, "switch_pbr_profile", side_effect=OSError("disk full")):
            self.assertEqual(dialog.apply(), "ask")
        self.assertIn("disk full", dialog.error)

    def test_what_left_the_atlas_is_told_after_the_change(self):
        project = self.project_with(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("dirt_s.png", solid_tile(BLUE))}, profile="none"
        )
        dialog = va.SettingsDialog(project)
        dialog.choose(LABPBR)
        self.assertEqual(dialog.apply(), "done")
        self.assertIn("dirt_s.png", dialog.done_message)

    def test_reading_the_folder_again_picks_up_a_file_that_came_after(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))})
        va.write_png_atomically(self.folder / "stone.png", solid_tile(BLUE))
        dialog = va.SettingsDialog(project)
        self.assertEqual(dialog.reload(), "done")
        self.assertEqual([tile.name for tile in project.unplaced], ["stone.png"])
        self.assertIn("again", dialog.done_message)

    def test_reading_the_folder_again_is_refused_while_a_tile_is_unsaved(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))})
        project.tiles[(0, 0)].planes[0, 0, SMOOTHNESS] = 9
        dialog = va.SettingsDialog(project)
        self.assertEqual(dialog.reload(), "ask")
        self.assertIn("unsaved", dialog.error)
        self.assertEqual(int(project.tiles[(0, 0)].planes[0, 0, SMOOTHNESS]), 9)

    def test_an_atlas_json_that_cannot_be_read_now_is_reported(self):
        project = self.project_with({(0, 0): ("dirt.png", solid_tile(RED))})
        (self.folder / "atlas.json").write_text("{ not json", encoding="utf-8")
        dialog = va.SettingsDialog(project)
        self.assertEqual(dialog.reload(), "ask")
        self.assertIn("atlas.json", dialog.error)


class ReservedNameTests(PbrProjectFixture):
    def test_names_that_end_as_companions_do_are_listed_under_any_setting(self):
        project = self.make_pbr_project(
            {(0, 0): ("dirt.png", solid_tile(RED)), (1, 0): ("dirt_s.png", solid_tile(RED)), (2, 0): ("x_n.png", solid_tile(RED))},
            profile="none",
        )
        warnings = project.reserved_name_warnings()
        self.assertEqual([name for name, _ in warnings], ["dirt_s.png", "x_n.png"])
        self.assertIn("companion", warnings[0][1])

    def test_a_project_with_no_such_names_has_no_warnings(self):
        project = self.make_pbr_project({(0, 0): ("dirt.png", solid_tile(RED))}, profile="none")
        self.assertEqual(project.reserved_name_warnings(), [])


class DescribeReservedNamesTests(unittest.TestCase):
    def test_nothing_to_say_when_there_are_none(self):
        self.assertEqual(va.describe_reserved_names([]), "")

    def test_the_first_three_are_named_and_the_rest_are_counted(self):
        warnings = [(f"x{index}_n.png", "w") for index in range(5)]
        text = va.describe_reserved_names(warnings)
        self.assertIn("5 tile names", text)
        self.assertIn("x0_n.png, x1_n.png, x2_n.png, ...", text)
        self.assertNotIn("x3_n.png", text)
        self.assertIn("1 tile name ", va.describe_reserved_names(warnings[:1]))


class WindowSettingsTests(EditorDriver):
    def setUp(self):
        super().setUp()
        self.application.draw()

    def click(self, position):
        self.send("down", position)
        self.send("up", position)

    def open_dialog(self):
        self.click(self.application.gear_area().center)
        self.assertIsNotNone(self.application.settings_dialog)

    def test_the_gear_is_left_of_the_atlas_title_and_inside_the_window(self):
        gear = self.application.gear_area()
        title = self.application.layout.atlas_title
        self.assertTrue(title.contains(gear))
        self.assertEqual(gear.left, title.left)
        self.assertGreater(self.application.atlas_title_left(), gear.right)

    def test_pressing_the_gear_opens_the_dialog_and_escape_closes_it(self):
        self.open_dialog()
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.application.settings_dialog)

    def test_the_window_behind_the_dialog_does_nothing(self):
        self.open_dialog()
        self.select()
        self.click(self.application.atlas_panel.rect.center)
        self.assertEqual(self.project.selection, set())
        self.assertIsNotNone(self.application.settings_dialog)

    def test_choosing_a_setting_and_pressing_apply_changes_it_and_the_rows_follow(self):
        self.open_dialog()
        areas = self.application.settings_areas()
        self.click(areas["choice"].center)
        self.click(self.application.settings_areas()["options"][LABPBR].center)
        self.assertEqual(self.application.settings_dialog.choice, LABPBR)
        self.click(self.application.settings_areas()["apply"].center)
        self.assertIsNone(self.application.settings_dialog)
        self.assertEqual(self.project.pbr_profile, LABPBR)
        self.editor.sync()
        self.assertEqual(len(self.editor.controls.rows), 8)
        self.assertIn(LABPBR, self.application.message)

    def test_escape_closes_the_open_list_first_and_the_dialog_after(self):
        self.open_dialog()
        self.click(self.application.settings_areas()["choice"].center)
        self.assertTrue(self.application.settings_dialog.list_open)
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNotNone(self.application.settings_dialog)
        self.assertFalse(self.application.settings_dialog.list_open)
        self.key(va.pygame.K_ESCAPE)
        self.assertIsNone(self.application.settings_dialog)

    def test_the_window_starts_with_a_warning_for_a_name_the_game_reads_as_a_companion(self):
        va.write_png_atomically(self.folder / "b_s.png", solid_tile(BLUE, 8))
        atlas_file = va.read_atlas_file(self.folder / "atlas.json")
        atlas_file.coordinates_to_names[(5, 5)] = "b_s.png"
        va.write_atlas_file(self.folder / "atlas.json", atlas_file)
        application = va.Application(va.AtlasProject.open(self.folder), va.pygame.Surface((1280, 800)))
        self.assertIn("b_s.png", application.message)
        self.assertIn("companion", application.message)

    def test_cancel_closes_without_changing_anything(self):
        self.open_dialog()
        self.click(self.application.settings_areas()["choice"].center)
        self.click(self.application.settings_areas()["options"][LABPBR].center)
        self.click(self.application.settings_areas()["cancel"].center)
        self.assertIsNone(self.application.settings_dialog)
        self.assertEqual(self.project.pbr_profile, "none")

    def test_apply_is_greyed_while_nothing_is_chosen(self):
        self.open_dialog()
        self.click(self.application.settings_areas()["apply"].center)
        self.assertIsNotNone(self.application.settings_dialog)

    def test_an_unsaved_tile_keeps_the_dialog_open_with_the_reason_on_it(self):
        self.project.tiles[(0, 0)].pixels[0, 0] = (1, 2, 3, 4)
        self.open_dialog()
        self.click(self.application.settings_areas()["choice"].center)
        self.click(self.application.settings_areas()["options"][LABPBR].center)
        self.click(self.application.settings_areas()["apply"].center)
        self.assertIn("unsaved", self.application.settings_dialog.error)
        self.assertEqual(self.project.pbr_profile, "none")

    def test_the_reload_button_reads_the_folder_again(self):
        va.write_png_atomically(self.folder / "extra.png", solid_tile(BLUE, 8))
        self.open_dialog()
        self.click(self.application.settings_areas()["reload"].center)
        self.assertEqual([tile.name for tile in self.project.unplaced], ["extra.png"])
        self.assertIsNone(self.application.settings_dialog)

    def test_the_window_draws_with_the_dialog_open_in_every_stage(self):
        self.open_dialog()
        self.application.draw()
        self.click(self.application.settings_areas()["choice"].center)
        self.application.draw()
        self.application.settings_dialog.error = "something"
        self.application.draw()
        self.application.settings_dialog.stage = "confirm"
        self.application.settings_dialog.lost_text = "dirt.png: normal_x"
        self.application.draw()

    def test_renaming_a_tile_to_a_companion_name_says_what_the_game_will_do(self):
        self.select((0, 0))
        self.editor.sync()
        self.click(self.editor.areas.header.center)
        self.editor.name_field.set_text("a_s")
        self.key(va.pygame.K_RETURN)
        self.assertIn("companion", self.application.message)
        self.assertIn("Renamed", self.application.message)


if __name__ == "__main__":
    unittest.main()
