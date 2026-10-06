"""Explicit real-provider runner: one ignored Sonnet/low test with native /compact.

Usage: python3 -B <this file> <built integration-test executable> <sanitized evidence.jsonl>
The private bundle is removed after the child exits; no global config is written.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

if len(sys.argv) != 3:
    sys.exit("usage: run_compact_sdk_fixture.py <integration binary> <sanitized evidence.jsonl>")

here = Path(__file__).resolve().parent
root = here.parents[3]
runtime = root / "provider-runtime"
sdk_relative = Path("node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs")
with tempfile.TemporaryDirectory(prefix="aa-compact-sdk-") as directory:
    stage = Path(directory)
    for name in ["claude-agent-sdk-bridge.mjs", "claude-native-delivery.mjs", "claude-owner-requests.mjs"]:
        shutil.copyfile(runtime / name, stage / name)
    sdk = stage / sdk_relative
    sdk.parent.mkdir(parents=True)
    sdk.write_text((here / "compact_sdk_fixture.mjs").read_text().replace(
        "__NATIVE_SDK_URL__", (runtime / sdk_relative).as_uri()).replace("__EVIDENCE_PATH__", str(stage / "evidence.jsonl")))
    env = {**os.environ, "AGENTSASSEMBLE_PROVIDER_RUNTIME": str(stage),
           "AA_VERIFY_PROVIDER": "claude", "AA_VERIFY_COMPACT": "1"}
    env.pop("AA_VERIFY_MODEL", None)
    completed = subprocess.run([str(Path(sys.argv[1]).resolve()), "--ignored", "--exact",
        "agent_session_boundary::real_instruction_persistence::real_managed_instruction_persistence",
        "--nocapture"], env=env, check=False)
    if (stage / "evidence.jsonl").exists():
        shutil.copyfile(stage / "evidence.jsonl", sys.argv[2])
    sys.exit(completed.returncode)
