"""Black-box training-plan tests; only the documented CLI and reports are inspected."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BIN = None


def run(argv, **kwargs):
    return subprocess.run([str(x) for x in argv], stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=180, **kwargs)


class Training(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="pgo-plan-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.project = self.root / "project"
        (self.project / "src").mkdir(parents=True)
        (self.project / "Cargo.toml").write_text(
            '[package]\nname="probe"\nversion="0.1.0"\nedition="2024"\n', encoding="utf-8")
        self.source = (ROOT / "tests/fixtures/training_probe.rs").read_text(encoding="utf-8")
        (self.project / "src/main.rs").write_text(self.source, encoding="utf-8")
        self.plan_dir = self.root / "plan folder"
        self.plan_dir.mkdir()
        self.plan = self.plan_dir / "plan.json"
        self.report = self.root / "report.json"
        self.profiles = self.root / "profiles"
        self.records = self.root / "records"
        self.env = os.environ.copy()
        for key in ["CARGO_TARGET_DIR", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "LLVM_PROFILE_FILE",
                    "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "TRAIN_VALUE", "TRAIN_REMOVE"]:
            self.env.pop(key, None)
        self.env.update(CARGO_NET_OFFLINE="true", CARGO_TERM_COLOR="never", RECORD_DIR=str(self.records),
                        TRAIN_PARENT="inherited")

    def command(self, extra=(), profiles=True, report=None):
        args = [BIN, "pgo", "train", "--plan", self.plan, "--report", report or self.report]
        if profiles:
            args += ["--profiles-dir", self.profiles]
        return args + ["--", *extra]

    def train(self, workloads, extra=(), success=True, profiles=True, report=None):
        self.plan.write_text(json.dumps({"workloads": workloads}), encoding="utf-8")
        output = run(self.command(extra, profiles, report), cwd=self.project, env=self.env)
        self.assertEqual(output.returncode == 0, success,
                         f"code={output.returncode}\n{output.stdout.decode(errors='replace')}\n"
                         f"{output.stderr.decode(errors='replace')}")
        return output

    def results(self):
        result = json.loads(self.report.read_text(encoding="utf-8"))
        rows = result["attempts"]
        self.assertEqual(result["succeeded"], sum(row["success"] for row in rows))
        self.assertEqual(result["failed"], sum(not row["success"] for row in rows))
        self.assertEqual(result["profiles"], sum(row["profiles"] for row in rows))
        for row in rows:
            if row["success"]:
                self.assertIsNone(row["error"])
                self.assertGreater(row["profiles"], 0)
            else:
                self.assertEqual(row["profiles"], 0)
                self.assertIsInstance(row["error"], str)
                self.assertTrue(row["error"])
        return result

    def empty_report(self):
        result = self.results()
        self.assertEqual(result["attempts"], [])
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (0, 0, 0))

    def observations(self):
        rows = []
        for path in sorted(self.records.glob("*")):
            row = {"args": []}
            for line in path.read_text(encoding="utf-8").splitlines():
                key, value = line.split("=", 1)
                if key == "arg":
                    row["args"].append(bytes.fromhex(value).decode())
                else:
                    row[key] = value
            rows.append(row)
        return rows

    def payloads(self):
        return sorted(path.read_bytes() for path in self.profiles.rglob("*.profraw") if path.is_file())

    def expected_payloads(self):
        profiles = []
        for row in self.observations():
            mode = row["args"][0] if row["args"] else "ok"
            if "profile" in row and mode != "fail":
                count = 5 if mode == "links" else 4 if mode == "multi" else 1
                profiles.extend([bytes.fromhex(row["profile"])] * count)
        return sorted(profiles)

    def test_defaults_and_forwarded_streams(self):
        output = self.train([{}])
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (1, 0, 1))
        row = self.observations()[0]
        self.assertEqual(row["args"], [])
        self.assertEqual(Path(row["cwd"]).resolve(), self.plan_dir.resolve())
        self.assertIn(b"probe stdout", output.stdout)
        self.assertIn(b"probe stderr", output.stderr)
        self.train([{"args": [], "env": {}, "repeat": 1}])
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (1, 0, 1))
        self.assertEqual(self.observations()[-1]["args"], [])

    def test_literal_arguments(self):
        args = ["ok", "", "two words", 'a"b', "*", "$HOME", "x;y", "--release", "café"]
        self.train([{"args": args}])
        self.assertEqual(self.observations()[0]["args"], args)
        self.results()

    def test_environment_isolation(self):
        self.env.update(TRAIN_VALUE="parent", TRAIN_REMOVE="remove me")
        self.train([{"env": {"TRAIN_VALUE": "child", "TRAIN_REMOVE": None}}, {}])
        first, second = self.observations()
        self.assertEqual(first["TRAIN_VALUE"], b"child".hex())
        self.assertEqual(first["TRAIN_REMOVE"], "NONE")
        self.assertEqual(second["TRAIN_VALUE"], b"parent".hex())
        self.assertEqual(second["TRAIN_REMOVE"], b"remove me".hex())
        self.assertTrue(all(row["TRAIN_PARENT"] == b"inherited".hex() for row in (first, second)))
        if os.name == "posix":
            self.train([{"env": {"llvm_profile_file": "ordinary variable"}}])
            self.assertEqual(self.observations()[-1]["llvm_profile_file"], b"ordinary variable".hex())

    def test_plan_relative_paths(self):
        (self.plan_dir / "work").mkdir()
        (self.plan_dir / "data").write_bytes(b"plan input\x00\xff")
        (self.plan_dir / "work/data").write_bytes(b"wrong cwd input")
        self.train([{"cwd": "work", "stdin": "data"}])
        row = self.observations()[0]
        self.assertEqual(Path(row["cwd"]).resolve(), (self.plan_dir / "work").resolve())
        self.assertEqual(row["stdin"], b"plan input\x00\xff".hex())
        if os.name == "posix" and sys.platform != "darwin":
            byte_directory = self.root / os.fsdecode(b"plan-\xff")
            self.plan_dir.rename(byte_directory)
            self.plan_dir = byte_directory
            self.plan = self.plan_dir / "plan.json"
            self.train([{"stdin": "data"}, {"cwd": "work", "stdin": "data"}])
            first, second = self.observations()[-2:]
            self.assertEqual(first["cwd_bytes"], os.fsencode(self.plan_dir.resolve()).hex())
            self.assertEqual(second["cwd_bytes"], os.fsencode((self.plan_dir / "work").resolve()).hex())
            self.assertEqual([first["stdin"], second["stdin"]], [b"plan input\x00\xff".hex()] * 2)
            self.assertEqual(self.results()["succeeded"], 2)

    def test_stdin_replay_and_eof(self):
        data = bytes(range(256)) * 1024
        (self.plan_dir / "data").write_bytes(data)
        (self.plan_dir / "empty").write_bytes(b"")
        self.train([{"stdin": "data", "repeat": 3}, {"stdin": "empty"}, {}])
        self.assertEqual([x["stdin"] for x in self.observations()], [data.hex()] * 3 + ["", ""])
        self.results()

    def test_successful_workload_may_leave_stdin_unread(self):
        (self.plan_dir / "large-input").write_bytes(b"x" * (1024 * 1024))
        self.train([{"args": ["ignore-stdin"], "stdin": "large-input", "repeat": 2}, {}])
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (3, 0, 3))
        self.assertEqual([row["stdin"] for row in self.observations()], ["", "", ""])
        self.assertEqual(self.payloads(), self.expected_payloads())

    def test_finished_workload_does_not_wait_for_descendant_stdin(self):
        (self.plan_dir / "large-input").write_bytes(b"x" * (16 * 1024 * 1024))
        ready, release, done = [self.root / name for name in ("ready", "release", "done")]
        helper = self.root / "holder.py"
        helper.write_text('''from pathlib import Path
import sys
import time
ready, release, done = map(Path, sys.argv[1:])
ready.write_text("ready")
sys.stdout.write("R")
sys.stdout.flush()
while not release.exists():
    time.sleep(0.01)
sys.stdin.close()
done.write_text("done")
''', encoding="utf-8")
        self.env.update(HOLDER_PYTHON=sys.executable, HOLDER_SCRIPT=str(helper),
                        HOLDER_READY=str(ready), HOLDER_RELEASE=str(release), HOLDER_DONE=str(done))
        try:
            self.train([{"args": ["hold-stdin"], "stdin": "large-input"}, {}])
            self.assertTrue(ready.exists(), "workload never started its stdin-inheriting descendant")
            result = self.results()
            self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (2, 0, 2))
            self.assertEqual(self.payloads(), self.expected_payloads())
        finally:
            # Release the fixture only after completion (or the ordinary hang watchdog).
            release.touch()
            if ready.exists():
                deadline = time.monotonic() + 180
                while not done.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(done.exists(), "stdin-holder fixture did not acknowledge cleanup")

    def test_omitted_stdin_ignores_parent_stream(self):
        self.plan.write_text(json.dumps({"workloads": [{}]}))
        output = run(self.command(), cwd=self.project, env=self.env, input=b"not workload input")
        self.assertEqual(output.returncode, 0, output.stderr.decode(errors="replace"))
        self.assertEqual(self.observations()[0]["stdin"], "")
        self.results()

    def test_repetitions_order_and_duplicate_entries(self):
        self.train([{"args": ["ok", "first"], "repeat": 2},
                    {"args": ["ok", "second"]}, {"args": ["ok", "second"]}])
        result = self.results()
        self.assertEqual([(r["workload"], r["repetition"]) for r in result["attempts"]],
                         [(0, 0), (0, 1), (1, 0), (2, 0)])
        self.assertEqual([r["args"][-1] for r in self.observations()], ["first", "first", "second", "second"])
        self.assertEqual(result["profiles"], 4)
        self.assertEqual(len(self.payloads()), 4)

    def test_complete_validation_before_build(self):
        if os.name == "posix" and os.geteuid() == 0:
            import pwd
            account = pwd.getpwnam("nobody")
            with tempfile.TemporaryDirectory(prefix="pgo-validation-") as directory:
                os.chown(directory, account.pw_uid, account.pw_gid)
                env = dict(os.environ, TMPDIR=directory, CARGO_HOME=str(Path(directory) / "cargo"))
                output = run([sys.executable, __file__, "--binary", BIN,
                              "--output_path", Path(directory) / "result.xml",
                              "--case", self._testMethodName], env=env,
                             user=account.pw_uid, group=account.pw_gid, extra_groups=[])
                self.assertEqual(output.returncode, 0,
                                 (output.stdout + output.stderr).decode(errors="replace"))
            return

        marker = self.root / "built"
        (self.project / "build.rs").write_text(
            'fn main() { std::fs::write(std::env::var_os("BUILD_MARKER").unwrap(), b"built").unwrap(); }')
        self.env["BUILD_MARKER"] = str(marker)
        (self.plan_dir / "not-a-directory").write_bytes(b"regular file")
        (self.plan_dir / "42").mkdir()
        (self.plan_dir / "7").write_bytes(b"numeric-looking filename")
        bad_entries = [{"repeat": 0}, {"repeat": -1}, {"repeat": 1.5}, {"repeat": True},
                       {"args": None}, {"env": None}, {"repeat": None},
                       {"env": {"TRAIN_VALUE": 4}}, {"env": {"TRAIN_VALUE": True}},
                       {"cwd": 42}, {"stdin": 7},
                       {"unknown": 1}, {"args": [4]}, {"cwd": "absent"}, {"stdin": "absent"},
                       {"stdin": "."}, {"cwd": None}, {"stdin": None},
                       {"cwd": "not-a-directory"}, {"args": ["bad\x00argument"]},
                       {"env": {"BAD\x00KEY": "value"}}, {"env": {"KEY": "bad\x00value"}},
                       {"env": {"LLVM_PROFILE_FILE": None}},
                       {"env": {"LLVM_PROFILE_FILE": "elsewhere"}}, {"env": {"A=B": "bad"}}]
        if os.name == "nt":
            bad_entries.append({"env": {"LlVm_PrOfIlE_fIlE": "elsewhere"}})
        if os.name == "posix":
            unreadable = self.plan_dir / "unreadable-input"
            unreadable.write_bytes(b"regular but unreadable input")
            unreadable.chmod(0o000)
            self.addCleanup(unreadable.chmod, 0o600)
            self.assertTrue(unreadable.is_file())
            with self.assertRaises(PermissionError):
                with unreadable.open("rb"):
                    pass
            bad_entries.append({"stdin": unreadable.name})
        for bad in bad_entries:
            with self.subTest(bad=bad):
                self.report.unlink(missing_ok=True)
                self.train([{}, bad], success=False)
                self.assertFalse(marker.exists(), "invalid plan started a build")
                self.assertEqual(self.observations(), [])
                self.empty_report()
        self.train([{"repeat": 1, "cwd": "42", "stdin": "7"}])
        self.assertTrue(marker.exists())
        self.assertEqual(Path(self.observations()[0]["cwd"]).resolve(), (self.plan_dir / "42").resolve())
        self.assertEqual(self.observations()[0]["stdin"], b"numeric-looking filename".hex())
        self.results()

    def test_empty_and_unknown_plan_rejected(self):
        (self.project / "build.rs").write_text(
            'fn main() { println!("cargo:rerun-if-env-changed=BUILD_MARKER"); '
            'std::fs::write(std::env::var_os("BUILD_MARKER").unwrap(), b"built").unwrap(); }')
        invalid_plans = [
            '{"workloads": [',
            *[json.dumps(value) for value in (None, [], [[{}]], "plan", 1, True, {})],
            *[json.dumps({"workloads": value}) for value in (None, {}, "work", 1, True, [])],
            *[json.dumps({"workloads": [{}, value]}) for value in (None, [], "work", 1, True)],
        ]
        for index, source in enumerate(invalid_plans):
            with self.subTest(plan=source):
                marker = self.root / f"built-{index}"
                self.env["BUILD_MARKER"] = str(marker)
                previous = self.observations()
                self.plan.write_text(source, encoding="utf-8")
                self.report.unlink(missing_ok=True)
                output = run(self.command(), cwd=self.project, env=self.env)
                self.assertNotEqual(output.returncode, 0,
                                    (output.stdout + output.stderr).decode(errors="replace"))
                self.assertFalse(marker.exists(), "invalid plan started a build")
                self.assertEqual(self.observations(), previous)
                self.empty_report()
        self.plan.unlink()
        self.report.unlink(missing_ok=True)
        missing_marker = self.root / "built-missing-plan"
        self.env["BUILD_MARKER"] = str(missing_marker)
        previous = self.observations()
        output = run(self.command(), cwd=self.project, env=self.env)
        self.assertNotEqual(output.returncode, 0, output.stderr.decode(errors="replace"))
        self.assertFalse(missing_marker.exists())
        self.assertEqual(self.observations(), previous)
        self.empty_report()
        marker = self.root / "built-valid"
        self.env["BUILD_MARKER"] = str(marker)
        previous = self.observations()
        self.train([{}])
        self.assertTrue(marker.exists())
        self.assertEqual(len(self.observations()), len(previous) + 1)
        self.results()

    def test_failed_build_never_launches(self):
        (self.project / "src/main.rs").write_text('fn main() { invalid rust code }')
        self.train([{}], success=False)
        self.assertEqual(self.results()["attempts"], [])
        self.assertEqual(self.observations(), [])
        (self.project / "src/main.rs").write_text(self.source)
        self.train([{}])
        previous = self.observations()
        self.report.unlink()
        no_manifest = self.root / "without-manifest"
        no_manifest.mkdir()
        output = run(self.command(), cwd=no_manifest, env=self.env)
        self.assertNotEqual(output.returncode, 0, output.stderr.decode(errors="replace"))
        result = self.results()
        self.assertEqual(result["attempts"], [])
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (0, 0, 0))
        self.assertEqual(self.observations(), previous)

    def test_single_build_and_late_failure_barrier(self):
        tools = self.root / "tools"
        tools.mkdir()
        source = tools / "cargo.rs"
        source.write_text(r'''
use std::io::Write;
use std::process::{Command, exit};
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let build = args.first().is_some_and(|x| x == "build");
    if build {
        let mut log = std::fs::OpenOptions::new().append(true).create(true)
            .open(std::env::var_os("BUILD_LOG").unwrap()).unwrap();
        log.write_all(b"build\n").unwrap();
    }
    let output = Command::new(std::env::var_os("REAL_CARGO").unwrap()).args(args).output().unwrap();
    if build && output.status.success() {
        if let Some(path) = std::env::var_os("INVALIDATE_EXECUTABLE") {
            std::fs::remove_file(&path).unwrap();
            if std::env::var_os("REPLACE_WITH_DIRECTORY").is_some() {
                std::fs::create_dir(path).unwrap();
            }
        }
    }
    std::io::stdout().write_all(&output.stdout).unwrap();
    std::io::stderr().write_all(&output.stderr).unwrap();
    if build && std::env::var_os("LATE_FAILURE").is_some() {
        println!("{{\"reason\":\"build-finished\",\"success\":false}}");
        exit(1);
    }
    exit(output.status.code().unwrap_or(1));
}
''', encoding="utf-8")
        cargo = tools / ("cargo.exe" if os.name == "nt" else "cargo")
        compiled = run(["rustc", "--edition=2024", source, "-o", cargo], env=self.env)
        self.assertEqual(compiled.returncode, 0, compiled.stderr.decode(errors="replace"))
        self.env["REAL_CARGO"] = shutil.which("cargo")
        self.env["PATH"] = str(tools) + os.pathsep + self.env["PATH"]
        self.env["BUILD_LOG"] = str(self.root / "build-log")
        self.train([{"repeat": 2}, {}])
        self.assertEqual((self.root / "build-log").read_text().splitlines(), ["build"])
        self.assertEqual(len(self.observations()), 3)
        self.env["LATE_FAILURE"] = "1"
        self.train([{}], success=False)
        self.assertEqual(self.results()["attempts"], [])
        self.assertEqual(len(self.observations()), 3, "ran executable before final Cargo status")
        self.env.pop("LATE_FAILURE")
        executable = Path(self.observations()[0]["executable"])
        backup = self.root / "saved-executable"
        shutil.copy2(executable, backup)
        # Invalidate the actual cached output after Cargo finishes, before training starts.
        for directory in (False, True):
            with self.subTest(replaced_by_directory=directory):
                self.env["INVALIDATE_EXECUTABLE"] = str(executable)
                if directory:
                    self.env["REPLACE_WITH_DIRECTORY"] = "1"
                try:
                    self.train([{"repeat": 2}, {}], success=False)
                    self.empty_report()
                    self.assertEqual(len(self.observations()), 3)
                finally:
                    self.env.pop("INVALIDATE_EXECUTABLE")
                    self.env.pop("REPLACE_WITH_DIRECTORY", None)
                    if executable.is_dir():
                        executable.rmdir()
                    shutil.copy2(backup, executable)
        self.train([{}])
        self.assertEqual(self.results()["succeeded"], 1)
        self.assertEqual(len(self.observations()), 4)

    def test_ambiguous_and_missing_executable(self):
        (self.project / "src/bin").mkdir()
        (self.project / "src/bin/second.rs").write_text(self.source)
        self.train([{}], success=False)
        self.assertEqual(self.results()["attempts"], [])
        self.assertEqual(self.observations(), [])
        (self.project / "src/lib.rs").write_text('pub fn library() {}')
        self.train([{}], extra=["--lib"], success=False)
        self.assertEqual(self.results()["attempts"], [])
        self.train([{}], extra=["--bin", "second"])
        self.assertEqual(self.observations()[0]["crate"], "second")

        (self.project / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["a", "b"]\nresolver = "2"\n')
        for package in ("a", "b"):
            directory = self.project / package
            (directory / "src").mkdir(parents=True)
            (directory / "Cargo.toml").write_text(
                f'[package]\nname = "package_{package}"\nversion = "0.1.0"\nedition = "2024"\n'
                '[[bin]]\nname = "runner"\npath = "src/main.rs"\n')
            (directory / "src/main.rs").write_text(
                self.source.replace('env!("CARGO_CRATE_NAME")', 'env!("CARGO_PKG_NAME")'))
        for cached in (False, True):
            with self.subTest(shared_output_path=True, cached=cached):
                previous = self.observations()
                before = self.payloads()
                self.report.unlink()
                self.train([{}], extra=["--workspace"], success=False)
                self.empty_report()
                self.assertEqual(self.observations(), previous)
                self.assertEqual(self.payloads(), before)
                for package in ("a", "b"):
                    self.train([{}], extra=["--package", f"package_{package}", "--bin", "runner",
                                            "--target-dir", str(self.root / f"selected-{package}")])
                    result = self.results()
                    self.assertEqual((result["succeeded"], result["failed"], result["profiles"]),
                                     (1, 0, 1))
                    self.assertEqual(self.observations()[-1]["crate"], f"package_{package}")
        self.assertEqual(self.payloads(), self.expected_payloads())

    def test_example_custom_profile_and_cached_artifact(self):
        (self.project / "examples").mkdir()
        (self.project / "examples/runner.rs").write_text(self.source)
        extra = ["--example", "runner", "--profile", "dev", "--target-dir", str(self.root / "custom target")]
        self.train([{}], extra=extra)
        self.train([{}], extra=extra)
        self.assertEqual([r["crate"] for r in self.observations()], ["runner", "runner"])
        self.assertEqual(self.results()["succeeded"], 1)
        self.assertEqual(len(self.payloads()), 2)

    def test_binary_selection_ignores_test_harnesses(self):
        for extra in (["--bin", "probe", "--tests"], ["--all-targets"], ["--all-targets"]):
            with self.subTest(cargo_args=extra):
                previous = len(self.observations())
                self.train([{}], extra=extra)
                self.assertEqual(len(self.observations()), previous + 1)
                self.assertEqual(self.observations()[-1]["crate"], "probe")
                self.assertEqual(self.results()["succeeded"], 1)
        self.assertEqual(self.payloads(), self.expected_payloads())

    @unittest.skipUnless(os.name == "posix", "Unix directory permissions")
    def test_publication_accounting_with_readonly_profile_source(self):
        if os.geteuid() == 0:
            import pwd
            account = pwd.getpwnam("nobody")
            with tempfile.TemporaryDirectory(prefix="pgo-permissions-") as directory:
                os.chown(directory, account.pw_uid, account.pw_gid)
                env = dict(os.environ, TMPDIR=directory, CARGO_HOME=str(Path(directory) / "cargo"))
                output = run([sys.executable, __file__, "--binary", BIN,
                              "--output_path", Path(directory) / "result.xml",
                              "--case", self._testMethodName], env=env,
                             user=account.pw_uid, group=account.pw_gid, extra_groups=[])
                self.assertEqual(output.returncode, 0,
                                 (output.stdout + output.stderr).decode(errors="replace"))
            return
        scratch = self.root / "scratch"
        scratch.mkdir()
        self.env["TMPDIR"] = str(scratch)
        self.profiles.mkdir()
        (self.profiles / "old.profraw").write_bytes(b"old")
        self.plan.write_text(json.dumps({"workloads": [{"args": ["sealed"]}, {}]}))
        try:
            output = run(self.command(), cwd=self.project, env=self.env)
            result = self.results()
            self.assertEqual(len(result["attempts"]), 2)
            self.assertEqual(output.returncode == 0, result["failed"] == 0,
                             output.stderr.decode(errors="replace"))
            self.assertTrue(result["attempts"][1]["success"], "did not continue to next workload")
            expected = [b"old"]
            observations = self.observations()
            self.assertEqual(len(observations), 2)
            for attempt, observation in zip(result["attempts"], observations):
                if attempt["success"]:
                    self.assertEqual(attempt["profiles"], 1)
                    expected.append(bytes.fromhex(observation["profile"]))
            actual = self.payloads()
            self.assertEqual(len(actual), len(expected),
                             f"published files disagree with successful-attempt accounting: {result}")
            self.assertTrue(actual == sorted(expected),
                            "published profile contents disagree with successful attempts")
            for unreadable in ("a.profraw", "c.profraw"):
                # Both ends of the name range exercise copy-first and preflight strategies.
                # No assertion depends on which source is visited first.
                before = self.payloads()
                start = len(self.observations())
                self.plan.write_text(json.dumps({"workloads": [
                    {"args": ["copy-error", unreadable]}, {}]}))
                output = run(self.command(), cwd=self.project, env=self.env)
                result = self.results()
                self.assertEqual(len(result["attempts"]), 2)
                self.assertEqual(output.returncode == 0, result["failed"] == 0,
                                 output.stderr.decode(errors="replace"))
                first, second = result["attempts"]
                self.assertEqual(first["profiles"], 3 if first["success"] else 0)
                self.assertTrue(second["success"], "copy failure stopped the later workload")
                self.assertEqual(second["profiles"], 1)
                # A move-based publisher may legitimately accept the unreadable file.
                # Restore fixture readability only after the command, to inspect its bytes.
                for directory in (self.profiles, scratch):
                    for path in directory.rglob("*.profraw"):
                        if not path.is_symlink() and path.is_file():
                            path.chmod(path.stat().st_mode | 0o600)
                rows = self.observations()[start:]
                self.assertEqual(len(rows), 2)
                expected = before + [bytes.fromhex(rows[1]["profile"])]
                if first["success"]:
                    expected += [bytes.fromhex(rows[0]["profile"])] * 3
                actual = self.payloads()
                self.assertEqual(len(actual), len(expected),
                                 f"partial publication disagrees with report: {result}")
                self.assertTrue(actual == sorted(expected),
                                "partial publication changed accepted profile contents")
                start = len(self.observations())
                self.train([{"args": ["copy-retry"]}, {}])
                retried = self.results()
                self.assertEqual((retried["succeeded"], retried["failed"], retried["profiles"]),
                                 (2, 0, 4))
                rows = self.observations()[start:]
                expected += [bytes.fromhex(rows[0]["profile"])] * 3
                expected += [bytes.fromhex(rows[1]["profile"])]
                actual = self.payloads()
                self.assertEqual(len(actual), len(expected),
                                 f"retry lost or duplicated contributions: {retried}")
                self.assertTrue(actual == sorted(expected),
                                "retry changed accepted profile contents")
        finally:
            for path in scratch.rglob("*"):
                if path.is_dir() and not path.is_symlink():
                    path.chmod(0o755)

    def test_failed_contributions_and_continuation(self):
        self.train([{"env": {"PAYLOAD": "first"}},
                    {"args": ["fail"], "env": {"PAYLOAD": "reject"}, "repeat": 2},
                    {"env": {"PAYLOAD": "last"}}], success=False)
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (2, 2, 2))
        self.assertEqual([r["success"] for r in result["attempts"]], [True, False, False, True])
        self.assertEqual([(r["workload"], r["repetition"]) for r in result["attempts"]],
                         [(0, 0), (1, 0), (1, 1), (2, 0)])
        self.assertEqual(self.payloads(), self.expected_payloads())
        self.assertEqual(len(self.observations()), 4)
        accepted = self.payloads()
        self.train([{"args": ["fail"], "repeat": 2}], success=False)
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (0, 2, 0))
        self.assertEqual([(r["workload"], r["repetition"]) for r in result["attempts"]],
                         [(0, 0), (0, 1)])
        self.assertEqual(self.payloads(), accepted)
        self.assertEqual(len(self.observations()), 6)
        if os.name == "posix":
            source = self.source.replace(
                'stop(if mode == "fail" { 7 } else { 0 });',
                'if mode == "fail" { std::process::abort(); } stop(0);')
            (self.project / "src/main.rs").write_text(source, encoding="utf-8")
            self.train([{"args": ["fail"]}, {}], success=False)
            result = self.results()
            self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (1, 1, 1))
            self.assertEqual(self.payloads(), self.expected_payloads())

    def test_empty_profiles_are_failed_attempts(self):
        self.train([{"args": ["none"]}, {"args": ["empty"]}, {}], success=False)
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (1, 2, 1))
        self.assertEqual(self.payloads(), self.expected_payloads())

    def test_recursive_profile_accounting(self):
        self.train([{"args": ["multi"], "repeat": 2}, {}])
        result = self.results()
        self.assertEqual(result["profiles"], 9)
        self.assertEqual([r["profiles"] for r in result["attempts"]], [4, 4, 1])
        self.assertEqual(self.payloads(), self.expected_payloads())

    @unittest.skipUnless(os.name == "posix", "Unix symbolic links")
    def test_profile_symlinks_not_collected(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.env["OUTSIDE"] = str(outside)
        self.train([{"args": ["links"]}])
        self.assertEqual(self.results()["profiles"], 5)
        self.assertEqual(self.payloads(), self.expected_payloads())

    def test_inherited_profile_destination_is_overridden(self):
        outside = self.root / "outside"
        self.env["LLVM_PROFILE_FILE"] = str(outside / "elsewhere.profraw")
        self.train([{}])
        self.assertFalse(outside.exists())
        self.assertEqual(self.payloads(), self.expected_payloads())
        self.results()

    def test_existing_data_and_reruns_are_additive(self):
        self.profiles.mkdir()
        (self.profiles / "old.profraw").write_bytes(b"old")
        self.train([{}, {}])
        self.train([{}, {}])
        self.assertEqual(self.results()["profiles"], 2)
        self.assertEqual(self.payloads(), sorted(self.expected_payloads() + [b"old"]))

    def test_launch_failure_does_not_stop_later_attempts(self):
        vanished = self.plan_dir / "vanished"
        vanished.mkdir()
        self.env["REMOVE_DIR"] = str(vanished)
        self.train([{"args": ["remove-cwd"]}, {"cwd": "vanished", "repeat": 2}, {}], success=False)
        result = self.results()
        self.assertEqual([r["success"] for r in result["attempts"]], [True, False, False, True])
        self.assertEqual([(r["workload"], r["repetition"]) for r in result["attempts"]],
                         [(0, 0), (1, 0), (1, 1), (2, 0)])
        self.assertEqual(len(self.observations()), 2)
        self.assertEqual(len(self.payloads()), 2)

    def test_report_write_failure_is_an_error(self):
        self.train([{}], report=self.root, success=False)
        self.train([{}])
        self.results()

    def test_real_profiles_feed_existing_optimizer(self):
        # The optimizer rebuild must not depend on instrumentation-only runtime APIs.
        (self.project / "src/main.rs").write_text('''fn main() {
    if std::env::args().any(|arg| arg == "realfail") { std::process::exit(9); }
    println!("training completed");
}
''')
        self.train([{"args": ["real"]}, {"args": ["realfail"]}, {"args": ["real"]}], success=False)
        result = self.results()
        self.assertEqual((result["succeeded"], result["failed"]), (2, 1))
        self.assertEqual(len(self.payloads()), result["profiles"])
        output = run([BIN, "pgo", "optimize", "--profiles-dir", self.profiles],
                     cwd=self.project, env=self.env)
        self.assertEqual(output.returncode, 0,
                         (output.stdout + output.stderr).decode(errors="replace"))

    def test_default_profile_directory(self):
        self.train([{}], profiles=False)
        self.results()
        files = list((self.project / "target/pgo-profiles").rglob("*.profraw"))
        self.assertEqual(sorted(p.read_bytes() for p in files), self.expected_payloads())
        original = {path: path.read_bytes() for path in files}
        target = self.root / "custom target"
        self.train([{}], profiles=False, extra=["--target-dir", str(target)])
        self.assertEqual(self.results()["profiles"], 1)
        new_files = list((target / "pgo-profiles").rglob("*.profraw"))
        latest = bytes.fromhex(self.observations()[-1]["profile"])
        self.assertEqual([path.read_bytes() for path in new_files], [latest])
        self.assertEqual({path: path.read_bytes() for path in
                          (self.project / "target/pgo-profiles").rglob("*.profraw")}, original)

        no_manifest = self.root / "without-manifest"
        no_manifest.mkdir()
        unrelated = self.root / "unrelated-project"
        (unrelated / "src").mkdir(parents=True)
        (unrelated / "Cargo.toml").write_text(
            '[package]\nname = "unrelated"\nversion = "0.1.0"\nedition = "2024"\n')
        (unrelated / "src/main.rs").write_text('fn main() {}\n')
        sentinel = unrelated / "target/pgo-profiles/old.profraw"
        sentinel.parent.mkdir(parents=True)
        sentinel.write_bytes(b"unrelated profiles")
        expected = sorted(original.values())
        selections = [
            (no_manifest, ["--manifest-path", str(self.project / "Cargo.toml")]),
            (unrelated, ["--manifest-path=" + os.path.relpath(self.project / "Cargo.toml", unrelated)]),
        ]
        for caller, selection in selections:
            with self.subTest(caller=caller.name, selection=selection[0]):
                previous = len(self.observations())
                self.report.unlink()
                output = run(self.command(extra=[*selection, "--bin", "probe"], profiles=False),
                             cwd=caller, env=self.env)
                self.assertEqual(output.returncode, 0,
                                 (output.stdout + output.stderr).decode(errors="replace"))
                self.assertEqual(self.results()["succeeded"], 1)
                self.assertEqual(len(self.observations()), previous + 1)
                self.assertEqual(self.observations()[-1]["crate"], "probe")
                expected.append(bytes.fromhex(self.observations()[-1]["profile"]))
                actual = [path.read_bytes() for path in
                          (self.project / "target/pgo-profiles").rglob("*.profraw")]
                self.assertEqual(len(actual), len(expected))
                self.assertTrue(sorted(actual) == sorted(expected), "wrong selected-project profiles")
                self.assertFalse((no_manifest / "target/pgo-profiles").exists())
                self.assertEqual(list(sentinel.parent.rglob("*.profraw")), [sentinel])
                self.assertEqual(sentinel.read_bytes(), b"unrelated profiles")

    def test_concurrent_invocations_both_survive(self):
        self.train([{}])  # warm the shared Cargo artifact before launching both trainers
        self.plan.write_text(json.dumps({"workloads": [{}]}))
        children = []
        for label in ("a", "b"):
            env = dict(self.env, PAYLOAD=label)
            child = subprocess.Popen([str(x) for x in self.command(report=self.root / f"{label}.json")],
                                     cwd=self.project, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            children.append(child)
        try:
            for child in children:
                _, err = child.communicate(timeout=180)
                self.assertEqual(child.returncode, 0, err.decode(errors="replace"))
            self.assertEqual(self.payloads(), self.expected_payloads())
            self.assertEqual(len(self.payloads()), 3)
            self.assertEqual(len(self.observations()), 3)
            for label in ("a", "b"):
                result = json.loads((self.root / f"{label}.json").read_text())
                self.assertEqual((result["succeeded"], result["failed"], result["profiles"]), (1, 0, 1))
        finally:
            for child in children:
                if child.poll() is None:
                    child.kill()
                child.communicate()


class Result(unittest.TextTestResult):
    def __init__(self, *args):
        super().__init__(*args)
        self.records = []

    def startTest(self, test):
        super().startTest(test)
        self.subfailures = []

    def stopTest(self, test):
        self.records.append((test.id(), "\n\n".join(self.subfailures) or None))
        super().stopTest(test)

    def addSuccess(self, test):
        super().addSuccess(test)

    def addFailure(self, test, err):
        super().addFailure(test, err)
        self.subfailures.append(self._exc_info_to_string(err, test))

    def addError(self, test, err):
        super().addError(test, err)
        self.subfailures.append(self._exc_info_to_string(err, test))

    def addSubTest(self, test, subtest, err):
        super().addSubTest(test, subtest, err)
        if err:
            self.subfailures.append(subtest.id() + "\n" + self._exc_info_to_string(err, test))


def main():
    global BIN
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output_path")
    parser.add_argument("--case", action="append")
    args = parser.parse_args()
    BIN = str(Path(args.binary).resolve())
    names = args.case or unittest.defaultTestLoader.getTestCaseNames(Training)
    if os.name != "posix":
        names = [name for name in names if name not in (
            "test_profile_symlinks_not_collected", "test_publication_accounting_with_readonly_profile_source")]
    suite = unittest.TestSuite(Training(name) for name in names)
    result = unittest.TextTestRunner(verbosity=2, resultclass=Result).run(suite)
    xml = ET.Element("testsuite", name="training-plan", tests=str(len(result.records)),
                     failures=str(sum(error is not None for _, error in result.records)))
    for name, error in result.records:
        case = ET.SubElement(xml, "testcase", name=name, classname="cargo_pgo.training")
        if error:
            ET.SubElement(case, "failure").text = error
    if args.output_path:
        path = Path(args.output_path)
        path.parent.mkdir(parents=True, exist_ok=True)
        ET.ElementTree(xml).write(path, encoding="utf-8", xml_declaration=True)
    return 0 if result.wasSuccessful() and not result.skipped and result.records else 1


if __name__ == "__main__":
    sys.exit(main())
