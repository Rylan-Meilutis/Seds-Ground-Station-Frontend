"""Gradle download retry and cache contracts; no Gradle, network or build use."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import tempfile
import unittest
from unittest.mock import Mock, patch

import yaml

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ci = load("ci_gradle_driver", "scripts/ci/frontend.py")
build = load("ci_gradle_build", "build.py")

POM = "https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-reflect/2.2.0/kotlin-reflect-2.2.0.pom"
# Shape of the dry run that Maven Central rate limited (hosted run 45524586611,
# android-armv7): dx indents Gradle's output and wraps it in its own error.
RATE_LIMITED = f"""350.71s  INFO Compiled [549/549]: groundstation_frontend
350.75s  INFO Bundling app...
\x1b[31mERROR\x1b[0m \x1b[33mdx bundle\x1b[0m: Failed to assemble build

Caused by:
    Failed to assemble apk:
    FAILURE: Build failed with an exception.

    * What went wrong:
    A problem occurred configuring root project 'app'.
    > Could not resolve all artifacts for configuration 'classpath'.
       > Could not resolve org.jetbrains.kotlin:kotlin-reflect:2.2.0.
         Required by:
             buildscript of root project 'app' > com.android.tools.build:gradle:8.7.0 > com.android.tools:sdk-common:31.7.0
          > Could not resolve org.jetbrains.kotlin:kotlin-reflect:2.2.0.
             > Could not get resource '{POM}'.
                > Could not GET '{POM}'. Received status code 429 from server: Too Many Requests
       > Could not resolve com.squareup:javawriter:2.5.0.
          > Skipped due to earlier error
    > There are 7 more failures with identical causes.

    * Try:
    > Run with --stacktrace option to get the stack trace.
    > Get more help at https://help.gradle.org.

    BUILD FAILED in 1m 27s

Error: Frontend build failed.
Hint: `dx` launched successfully; the failure came from a downstream build step.
FAILED android-armv7: 1
"""
GET_429 = f"Could not GET '{POM}'. Received status code 429 from server: Too Many Requests"


def gradle(*leaves, task="Execution failed for task ':app:checkReleaseAarMetadata'."):
    tree = "\n".join(f"   > Could not resolve a:b:1.\n      > Could not get resource '{POM}'.\n         > {leaf}" for leaf in leaves)
    return (f"FAILURE: Build failed with an exception.\n\n* What went wrong:\n{task}\n"
            f"> Could not resolve all files for configuration ':app:releaseRuntimeClasspath'.\n{tree}\n\n* Try:\n> Run with --info\n\nBUILD FAILED in 3s\n")


class Classification(unittest.TestCase):
    def test_hosted_429_failure_is_a_transient_download(self):
        self.assertEqual(ci.transient_dependency_failures(RATE_LIMITED), [GET_429])

    def test_429_5xx_and_timeouts_only(self):
        for leaf in [GET_429, *(GET_429.replace("429", str(code)) for code in (500, 502, 503, 504)),
                     GET_429.replace("GET", "HEAD"), "Read timed out", "Connect timed out",
                     "Connect to dl.google.com:443 [dl.google.com/142.250.1.1] failed: Connection timed out (Connection timed out)"]:
            with self.subTest(leaf=leaf):
                self.assertEqual(ci.transient_dependency_failures(gradle(leaf)), [leaf])
        for leaf in [GET_429.replace("429", "404"), GET_429.replace("429", "403"), GET_429.replace("429", "401"),
                     "Could not find a:b:1.", "PKIX path building failed: unable to find valid certification path",
                     "Compilation error. See log for more details", "Skipped due to earlier error",
                     GET_429.replace("https:", "http:")]:
            with self.subTest(leaf=leaf):
                self.assertEqual(ci.transient_dependency_failures(gradle(leaf)), [])

    def test_one_permanent_cause_makes_the_failure_final(self):
        self.assertEqual(ci.transient_dependency_failures(gradle(GET_429, "Could not find a:b:1.")), [])
        self.assertEqual(ci.transient_dependency_failures(RATE_LIMITED + gradle("Could not find a:b:1.")), [])
        self.assertEqual(ci.transient_dependency_failures(RATE_LIMITED + gradle(GET_429)), [GET_429] * 2)

    def test_timeouts_count_only_under_a_download(self):
        text = ("FAILURE: Build failed with an exception.\n\n* What went wrong:\nExecution failed for task ':app:test'.\n"
                "> Read timed out\n\n* Try:\n")
        self.assertEqual(ci.transient_dependency_failures(text), [])

    def test_compiler_errors_and_text_without_a_gradle_failure_are_final(self):
        for text in ["", "Received status code 429 from server\n", GET_429 + "\n",
                     "error[E0308]: mismatched types\n" + RATE_LIMITED,
                     "error: could not compile `groundstation_frontend`\n" + RATE_LIMITED,
                     "e: file:///w/app/src/main/Main.kt:1:1 Unresolved reference\n" + RATE_LIMITED,
                     "FAILURE: Build failed with an exception.\n" + RATE_LIMITED,
                     RATE_LIMITED.replace("* What went wrong:", "* Something new:")]:
            with self.subTest(text=text[:60]):
                self.assertEqual(ci.transient_dependency_failures(text), [])

    def test_wrapper_distribution_download(self):
        url = "https://services.gradle.org/distributions/gradle-8.14.3-bin.zip"
        prefix = 'Exception in thread "main" java.io.IOException: '
        for line in [f"Server returned HTTP response code: 503 for URL: {url}", f"Server returned HTTP response code: 429 for URL: {url}",
                     f"Downloading from {url} failed: timeout (10000ms)"]:
            self.assertEqual(ci.transient_dependency_failures(f"Downloading {url}\n{prefix}{line}\n\tat java.base\n"), [prefix + line])
        for line in [prefix + f"Server returned HTTP response code: 404 for URL: {url}",
                     prefix + "Server returned HTTP response code: 503 for URL: https://example.invalid/gradle.zip",
                     'Exception in thread "main" java.lang.OutOfMemoryError']:
            self.assertEqual(ci.transient_dependency_failures(line + "\n" + RATE_LIMITED), [], line)


class Retry(unittest.TestCase):
    def build(self, rows, *, fails=False, **options):
        children = [Mock(stdout=io.StringIO(text), wait=Mock(return_value=code)) for text, code in rows]
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci.shutil, "which", return_value="/prepared/python"), \
             patch.object(ci.subprocess, "Popen", side_effect=children) as spawn, \
             patch.object(ci.time, "sleep") as sleep, patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": directory + "/summary"}), \
             contextlib.redirect_stdout(io.StringIO()):
            if fails:
                with self.assertRaises(ci.CommandFailure):
                    ci.checked_gradle_build(["python"], os.environ.copy(), "release-android", **options)
            else:
                ci.checked_gradle_build(["python"], os.environ.copy(), "release-android", **options)
            logs = Path(directory) / "ci-results"
            receipts = {p.stem: json.loads(p.read_text()) for p in logs.glob("*.json")}
            raw = {p.stem: p.read_text() for p in logs.glob("*.log")}
            summary = Path(directory, "summary").read_text() if Path(directory, "summary").exists() else ""
            return spawn.call_count, [c.args[0] for c in sleep.call_args_list], receipts, raw, summary

    def test_every_gradle_build_reads_maven_central_from_the_mirror(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
             patch.object(ci, "checked") as build:
            ci.checked_gradle_build(["python"], os.environ.copy(), "release-android")
            installed = Path(directory, ".gradle-user-home", "init.d", "maven-central-mirror.gradle")
            self.assertTrue(installed.is_file())
            build.assert_called_once()
        script = ci.MAVEN_CENTRAL_MIRROR_SCRIPT.read_text()
        self.assertEqual(installed.name, ci.MAVEN_CENTRAL_MIRROR_SCRIPT.name)
        self.assertIn("'https://maven-central.storage-download.googleapis.com/maven2/'", script)
        # Plugin, settings, buildscript and project repositories alike.
        for scope in ["settings.pluginManagement.repositories", "settings.buildscript.repositories",
                      "settings.dependencyResolutionManagement.repositories",
                      "redirect(buildscript.repositories)", "redirect(repositories)"]:
            self.assertIn(scope, script)
        central = re.search(r"def central = ~/(.*)/$", script, re.MULTILINE).group(1).replace("\\/", "/")
        for url in ["https://repo.maven.apache.org/maven2/", "https://repo.maven.apache.org/maven2",
                    "https://repo1.maven.org/maven2/"]:
            self.assertRegex(url, f"^(?:{central})$")
        for url in ["https://dl.google.com/dl/android/maven2/", "https://plugins.gradle.org/m2/",
                    "https://repo.maven.apache.org/maven2/extra/"]:
            self.assertNotRegex(url, f"^(?:{central})$")

    def test_clean_build_records_no_retry(self):
        count, delays, receipts, _, summary = self.build([("ok\n", 0)])
        self.assertEqual((count, delays, summary), (1, [], ""))
        self.assertEqual(list(receipts), ["release-android"])
        self.assertNotIn("dependency_retry_attempt", receipts["release-android"])

    def test_transient_failure_is_retried_and_recorded(self):
        count, delays, receipts, raw, summary = self.build([(RATE_LIMITED, 1), ("ok\n", 0)])
        self.assertEqual((count, delays), (2, [60]))
        # The first attempt's log and receipt stay exactly as they were.
        self.assertEqual(raw["release-android"], RATE_LIMITED)
        self.assertEqual(receipts["release-android"]["exit"], 1)
        retry = receipts["release-android-dependency-retry-1"]
        self.assertEqual((retry["exit"], retry["dependency_retry_attempt"]), (0, 1))
        expected = [{"attempt": "release-android", "exit": 1, "transient_causes": 1, "first_causes": [GET_429], "retry_after_s": 60}]
        self.assertEqual(retry["after_transient_dependency_failures"], expected)
        self.assertEqual(receipts["release-android-dependency-retries"], expected)
        self.assertIn("dependency download retry 1", summary)

    def test_retries_are_bounded_and_the_last_failure_is_fatal(self):
        count, delays, receipts, _, _ = self.build([(RATE_LIMITED, 1)] * 5, fails=True)
        self.assertEqual((count, delays), (5, [60, 240, 600, 900]))
        self.assertEqual(len(receipts["release-android-dependency-retries"]), 4)

    def test_any_other_failure_and_any_warning_stay_fatal_at_once(self):
        for text, code in [("error: real build failure\n", 1), (gradle("Could not find a:b:1."), 1),
                           ("warning: unused import\n" + RATE_LIMITED, 1), ("w: file:///x.kt deprecated\n" + RATE_LIMITED, 1),
                           ("warning: unused import\n", 0), (RATE_LIMITED + "WARNING: something new\n", 1), ("", 1)]:
            with self.subTest(text=text[:40], code=code):
                count, delays, receipts, _, _ = self.build([(text, code)], fails=True)
                self.assertEqual((count, delays, list(receipts)), (1, [], ["release-android"]))

    def test_a_warning_or_real_failure_in_the_retry_is_fatal(self):
        for text, code in [("warning: unused import\n", 0), ("error: real build failure\n", 1)]:
            with self.subTest(text=text):
                self.assertEqual(self.build([(RATE_LIMITED, 1), (text, code)], fails=True)[0], 2)

    def test_options_reach_every_attempt(self):
        progress = "warning: Waiting for cargo-metadata...\n"
        count, _, receipts, _, _ = self.build([(progress + RATE_LIMITED, 1), (progress, 0)], dioxus_progress=True)
        self.assertEqual(count, 2)
        self.assertEqual(receipts["release-android-dependency-retry-1"]["external_metadata_progress_warnings"], [progress.strip()])

    def test_only_gradle_builds_use_the_retry(self):
        for name, gradle_target in [("android", True), ("wearos", True), ("web", False), ("linux", False)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
                 patch.object(ci, "validate_host"), patch.object(ci, "verify_bundletool"), \
                 patch.object(ci.shutil, "which", return_value="/tool"), patch.object(ci, "prepare_android_development_signing"), \
                 patch.object(ci, "checked") as plain, patch.object(ci, "checked_gradle_build") as retried, \
                 self.assertRaisesRegex(RuntimeError, "no staged artifacts"):
                ci.run(name, "release")
            self.assertEqual((plain.called, retried.called), (not gradle_target, gradle_target))
        for name, retry in [("android", True), ("macos", False), ("windows", False)]:
            with self.subTest(release=name), tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
                 patch.object(ci, "validate_host"), patch.object(ci, "verify_bundletool"), patch.object(ci, "release_env", return_value={"PATH": ""}), \
                 patch.object(ci.shutil, "which", return_value="/tool"), \
                 patch.object(ci, "checked") as plain, patch.object(ci, "checked_gradle_build") as retried:
                ci.release_build(name)
                self.assertEqual((plain.called, retried.called), (not retry, retry))

    def test_download_outage_never_enters_the_android_packaging_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "app/src/main").mkdir(parents=True)
            outage = build.CapturedCommandFailure(1, ["dx"], gradle(GET_429), False)
            packaging = build.CapturedCommandFailure(1, ["dx"], gradle("Could not find a:b:1."), False)
            self.assertFalse(build._android_gradle_fallback_allowed(outage, Path(directory)))
            self.assertTrue(build._android_gradle_fallback_allowed(packaging, Path(directory)))


class CacheContract(unittest.TestCase):
    """Nothing but Gradle's own downloads may be cached, and never for a signed job."""
    PATHS = [".gradle-user-home/caches/modules-2", ".gradle-user-home/wrapper/dists",
             "!.gradle-user-home/caches/modules-2/modules-2.lock", "!.gradle-user-home/caches/modules-2/gc.properties"]

    @staticmethod
    def steps(workflow):
        for job in workflow["jobs"].values():
            yield from job.get("steps", [])

    def test_cache_paths_are_exactly_the_download_directories(self):
        steps = [s for s in self.steps(yaml.safe_load((ROOT / ".github/workflows/platforms.yml").read_text()))
                 if "cache" in s.get("uses", "") and not s["uses"].startswith("Swatinem/rust-cache@")]
        # The other cache is the dx CLI (test_ci_platforms.DxCache).
        self.assertEqual([s["with"]["path"] for s in steps if "gradle" not in s["with"]["path"]], [".ci-tools/dx", ".ci-tools/dx"])
        steps = [s for s in steps if "gradle" in s["with"]["path"]]
        self.assertEqual([s["uses"] for s in steps], ["actions/cache/restore@v5", "actions/cache/save@v5"])
        for step in steps:
            self.assertEqual(step["with"]["path"].split(), self.PATHS)
            self.assertFalse(set(step["with"]) - {"path", "key", "restore-keys"})

    def test_cache_paths_exclude_every_place_signing_material_is_written(self):
        cached = [PurePosixPath(p) for p in self.PATHS if not p.startswith("!")]
        generated = ["target/dx/groundstation_frontend/release/android/app/app/build.gradle.kts", "watch/wear/app/build",
                     ".ci-tools/android-development-signing/development.p12", ".ci-tools/android-development-signing/password.txt",
                     ".gradle-user-home/gradle.properties", ".gradle-user-home/init.gradle", "release-assets", "artifacts", "dist"]
        for path in map(PurePosixPath, generated):
            for root in cached:
                self.assertFalse(path == root or root in path.parents or path in root.parents, (str(path), str(root)))
        for root in cached:
            self.assertFalse(root.is_absolute() or ".." in root.parts or "*" in str(root) or "$" in str(root))
        # Development signing and release signing stay where this test says.
        with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), patch.object(ci, "checked"):
            (Path(directory) / ".ci-tools").mkdir()
            with self.assertRaisesRegex(RuntimeError, "no regular keystore"):
                ci.prepare_android_development_signing({})
            written = [PurePosixPath(p.relative_to(directory).as_posix()) for p in Path(directory).rglob("*") if p.is_file()]
            self.assertEqual(written, [PurePosixPath(".ci-tools/android-development-signing/password.txt")])
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"RUNNER_TEMP": directory}):
            self.assertNotIn(ROOT, ci.release_signing_dir().parents)

    def test_only_pushes_save_and_pull_requests_never_do(self):
        steps = list(self.steps(yaml.safe_load((ROOT / ".github/workflows/platforms.yml").read_text())))
        save = next(s for s in steps if s.get("uses", "").startswith("actions/cache/save@") and "gradle" in s["with"]["path"])
        self.assertTrue(save["if"].startswith("github.event_name == 'push' && "))
        self.assertNotIn("||", re.sub(r"\([^)]*\)", "", save["if"]))
        self.assertNotIn("always()", save["if"])
        # After the build: a failed build saves nothing.
        self.assertLess([s.get("run") for s in steps].index("python scripts/ci/frontend.py build"), steps.index(save))
        self.assertNotIn("pull_request_target", (ROOT / ".github/workflows/platforms.yml").read_text())

    def test_release_and_promotion_jobs_use_no_cache(self):
        workflows = sorted((ROOT / ".github/workflows").glob("*.yml"))
        self.assertEqual([p.name for p in workflows], ["platforms.yml", "promote.yml", "release.yml"])
        for path in workflows:
            workflow = yaml.safe_load(path.read_text())
            for step in self.steps(workflow):
                uses = step.get("uses", "")
                if uses.startswith("Swatinem/rust-cache@"):
                    # Compiled dependencies: platform CI saves on push, nothing else saves.
                    self.assertEqual(step["with"]["save-if"],
                                     "${{ github.event_name == 'push' && github.ref == 'refs/heads/dev' }}"
                                     if path.name == "platforms.yml" else False)
                    self.assertIs(step["with"]["cache-bin"], False)
                    continue
                # setup-java/-node/-python (and gradle's own action) can restore caches too.
                self.assertFalse({k for k in step.get("with", {}) if k.startswith("cache")}, (path.name, uses))
                self.assertNotIn("setup-gradle", uses)
                if path.name != "platforms.yml" and "cache" in uses:
                    # Only the dx CLI and Gradle's downloads, restored and never saved.
                    self.assertEqual((path.name, uses), ("release.yml", "actions/cache/restore@v5"))
                    if step["with"]["path"] != ".ci-tools/dx":
                        self.assertEqual(step["with"]["path"].split(), self.PATHS)
                        self.assertEqual(step["with"]["restore-keys"], "gradle-downloads-android-")
        self.assertNotIn("secrets.", (ROOT / ".github/workflows/platforms.yml").read_text())

    def test_release_android_jobs_restore_the_platform_gradle_key(self):
        platforms = yaml.safe_load((ROOT / ".github/workflows/platforms.yml").read_text())
        restore = next(s for s in platforms["jobs"]["build"]["steps"] if s.get("name") == "Cached Gradle downloads")
        release = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())
        expected = restore["with"]["key"].replace("${{ startsWith(matrix.target, 'wearos') && 'wearos' || 'android' }}", "android")
        for name in ("android", "android-launch", "play"):
            steps = release["jobs"][name]["steps"]
            names = [s.get("name") for s in steps]
            step = steps[names.index("Cached Gradle downloads")]
            self.assertEqual(step["with"]["key"], expected, name)
            self.assertLess(names.index("Cached Gradle downloads"), names.index("Pinned frontend tools, warnings fatal"), name)
        for name, job in release["jobs"].items():
            if name not in ("android", "android-launch", "play"):
                self.assertNotIn("Cached Gradle downloads", [s.get("name") for s in job.get("steps", [])], name)

    def test_gradle_home_is_the_cached_one_for_every_gradle_target(self):
        for name in ("android", "wearos"):
            with tempfile.TemporaryDirectory() as directory, patch.object(ci, "ROOT", Path(directory)), \
                 patch.object(ci, "validate_host"), patch.object(ci, "verify_bundletool"), \
                 patch.object(ci.shutil, "which", return_value="/tool"), patch.object(ci, "prepare_android_development_signing"), \
                 patch.object(ci, "checked_gradle_build") as retried, self.assertRaisesRegex(RuntimeError, "no staged artifacts"):
                ci.run(name, "debug")
            self.assertEqual(retried.call_args.args[1]["GRADLE_USER_HOME"], str(Path(directory) / ".gradle-user-home"))


if __name__ == "__main__":
    unittest.main()
