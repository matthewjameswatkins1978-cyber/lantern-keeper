import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class PluginSourceTests(unittest.TestCase):
    def test_metadata_versions_and_descriptions_match(self):
        root = json.loads((ROOT / "plugin.json").read_text(encoding="utf-8"))
        codex = json.loads((ROOT / ".codex-plugin" / "plugin.json").read_text(encoding="utf-8"))
        self.assertEqual(root["version"], "0.2.0")
        self.assertEqual(root["version"], codex["version"])
        self.assertEqual(root["description"], codex["description"])
        self.assertEqual(root["extensions"]["com.openai"]["interface"], codex["interface"])

    def test_fixture_covers_required_routes(self):
        cases = json.loads((ROOT / "tests" / "routing-cases.json").read_text(encoding="utf-8"))
        ids = {case["id"] for case in cases}
        routes = {case["route"] for case in cases}
        self.assertGreaterEqual(len(cases), 16)
        self.assertTrue({"consult", "skip", "write", "no-write", "write-if-clear", "clarify-if-ambiguous"} <= routes)
        self.assertTrue(all(case.get("prompt") and case.get("reason") for case in cases))
        self.assertTrue(all(case.get("context") for case in cases if case["route"] in {"write-if-clear", "clarify-if-ambiguous"}))
        self.assertIn("explicit-lantern-nudge", ids)
        self.assertIn("project-concurrency", ids)

    def test_policy_covers_routing_and_authority_boundaries(self):
        skill = (ROOT / "skills" / "lantern-memory" / "SKILL.md").read_text(encoding="utf-8")
        for phrase in (
            "materially depend",
            "`🏮` signal means consult Lantern",
            "Do not use Lantern as a preflight",
            "read-only transport snapshot",
            "GitHub fallback is never a write path",
            "Report “saved”",
            "authority grant",
            "`APPLIED`",
            "`DENIED`",
            "`REQUIRES_APPROVAL`",
            "`CONFLICT`",
            "`EXPIRED`",
            "`FAILED`",
            "`INDETERMINATE`",
            "`SUSPECT`",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, skill)

    def test_mcp_templates_contain_no_machine_paths_or_credentials(self):
        for name in ("mcp.json.template", ".mcp.json.template"):
            data = json.loads((ROOT / name).read_text(encoding="utf-8"))
            serialized = json.dumps(data)
            self.assertIn("__LANTERN_KEEPER_ROOT__", serialized)
            self.assertNotIn("D:\\Projects", serialized)
            self.assertNotIn("token", serialized.lower())


if __name__ == "__main__":
    unittest.main()
