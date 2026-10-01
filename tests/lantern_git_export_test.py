import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "lantern_git_export.py"
SPEC = importlib.util.spec_from_file_location("lantern_git_export", MODULE_PATH)
assert SPEC and SPEC.loader
lantern_git_export = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(lantern_git_export)


class LanternGitExportTests(unittest.TestCase):
    def test_uuid_mapping_is_stable_and_validated(self):
        record_id = "ed7e2de6-2d4d-464a-903d-f41df1b990f3"
        self.assertEqual(
            lantern_git_export.uuid_path("memory-item", False, record_id),
            "records/memory-item/active/ed/7e/ed7e2de6-2d4d-464a-903d-f41df1b990f3.md",
        )
        with self.assertRaises(ValueError):
            lantern_git_export.uuid_path("memory-item", False, "not-a-uuid")

    def test_source_revision_is_resolved_from_exporter_checkout(self):
        original = Path.cwd()
        with tempfile.TemporaryDirectory() as temp:
            try:
                os.chdir(temp)
                self.assertIsNotNone(lantern_git_export.git_revision())
            finally:
                os.chdir(original)

    def test_credential_shaped_text_fails_closed(self):
        with self.assertRaisesRegex(RuntimeError, "resembles a credential"):
            lantern_git_export.assert_safe_record({"id": "r1", "content": "api_key=sk-proj-12345678901234567890"})

    def test_marker_round_trip_checks_provenance_and_hash(self):
        record_id = "ed7e2de6-2d4d-464a-903d-f41df1b990f3"
        record = {
            "record_type": "memory-item", "id": record_id, "kind": "reference",
            "content": "pi-crossclient-20261001-094804", "originator_actor_id": "matthew",
            "holder_actor_id": "matthew", "source_id": None, "episode_id": None,
        }
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            path = lantern_git_export.uuid_path("memory-item", False, record_id)
            data = lantern_git_export.json_markdown(record)
            target = root / path
            target.parent.mkdir(parents=True)
            target.write_bytes(data)
            manifest = {"records": [{"path": path, "bytes": len(data), "sha256": lantern_git_export.hashlib.sha256(data).hexdigest()}]}
            lantern_git_export.validate_staged(root, manifest, "pi-crossclient-20261001-094804", record_id)
            stored = json.loads(target.read_text().split("```json\n", 1)[1].rsplit("\n```", 1)[0])
            self.assertEqual(stored["id"], record_id)
            self.assertEqual(stored["originator_actor_id"], "matthew")
            self.assertEqual(stored["holder_actor_id"], "matthew")


if __name__ == "__main__":
    unittest.main()
