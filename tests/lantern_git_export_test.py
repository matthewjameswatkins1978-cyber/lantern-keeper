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

    def test_index_is_split_into_bounded_deterministic_shards(self):
        original_limit = lantern_git_export.MAX_INDEX_SHARD_BYTES
        try:
            lantern_git_export.MAX_INDEX_SHARD_BYTES = 220
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                rows = [{"id": f"id-{number}", "preview": "x" * 35} for number in range(5)]
                first = lantern_git_export.write_index_shards(root, "active", rows)
                self.assertGreater(len(first), 1)
                self.assertEqual(first, [f"index/active-{n:05d}.jsonl" for n in range(len(first))])
                for path in first:
                    self.assertLessEqual((root / path).stat().st_size, 220)
        finally:
            lantern_git_export.MAX_INDEX_SHARD_BYTES = original_limit

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
            entry = {
                "path": path,
                "bytes": len(data),
                "sha256": lantern_git_export.hashlib.sha256(data).hexdigest(),
                "git_blob_sha": lantern_git_export.hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest(),
            }
            target.with_suffix(".integrity.json").write_text(
                json.dumps({"record_id": record_id, **entry}), encoding="utf-8"
            )
            lantern_git_export.validate_staged(root, [entry], "pi-crossclient-20261001-094804", record_id)
            stored = json.loads(target.read_text().split("```json\n", 1)[1].rsplit("\n```", 1)[0])
            self.assertEqual(stored["id"], record_id)
            self.assertEqual(stored["originator_actor_id"], "matthew")
            self.assertEqual(stored["holder_actor_id"], "matthew")

    def test_status_json_metadata_and_manifest_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = {
                "format": "lantern-git-mirror",
                "export_schema_version": 1,
                "record_count": 42,
                "counts_by_type": {"memory-item": 42, "claim": 0, "belief": 0},
                "snapshot_consistency": "test-consistency",
            }
            manifest_text = json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n"
            manifest_digest = f"sha256:{lantern_git_export.hashlib.sha256(manifest_text.encode('utf-8')).hexdigest()}"
            status = {
                "format": "lantern-git-mirror-status",
                "status_schema_version": 1,
                "generated_at": "2026-10-02T05:00:00Z",
                "read_started_at": "2026-10-02T04:59:50Z",
                "source_software_revision": "abcdef123456",
                "export_schema_version": 1,
                "record_count": 42,
                "counts_by_type": manifest["counts_by_type"],
                "active_count": 30,
                "archived_count": 12,
                "manifest_digest": manifest_digest,
                "snapshot_consistency": manifest["snapshot_consistency"],
            }
            status_path = root / "status.json"
            status_path.write_text(json.dumps(status, sort_keys=True, indent=2) + "\n", encoding="utf-8")
            loaded = json.loads(status_path.read_text(encoding="utf-8"))
            self.assertEqual(loaded["format"], "lantern-git-mirror-status")
            self.assertEqual(loaded["record_count"], 42)
            self.assertTrue(loaded["manifest_digest"].startswith("sha256:"))
            self.assertEqual(loaded["source_software_revision"], "abcdef123456")


if __name__ == "__main__":
    unittest.main()

