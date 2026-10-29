import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).parents[1]

def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

build = load('release_build', ROOT / 'build.py')
targets = load('release_targets', ROOT / 'scripts/build_targets.py')

class FinalReleaseTests(unittest.TestCase):
    def test_final_rejects_unsigned_and_debug(self):
        for option in ['--no-sign', '--debug']:
            with self.subTest(option=option), self.assertRaises(SystemExit):
                targets.parse_args(['--final', option])

    def test_final_workflows_run_before_artifact_export(self):
        for name in ['macos', 'ios', 'android']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp, patch.object(targets.platform, 'system', return_value='Darwin'), patch.object(targets.subprocess, 'run'):
                root = Path(temp)
                events = []
                def finalize(root, platform, package_type, *, prepare):
                    events.append('prepare' if prepare else 'finalize')
                    if not prepare:
                        (root / 'dist').mkdir()
                        (root / 'dist' / 'signed').write_text('verified')
                builder = Mock(side_effect=lambda *a, **kw: events.append('build'))
                package = Mock(side_effect=lambda *a, **kw: events.append('ipa'))
                targets.handle_target_args(['--target', name, '--final'], root, builder, package, finalize)
                expected = ['prepare', 'build', 'ipa', 'finalize'] if name == 'ios' else ['prepare', 'build', 'finalize']
                self.assertEqual(events, expected)
                self.assertEqual((root / 'artifacts' / name / 'dist/signed').read_text(), 'verified')
                if name == 'ios': package.assert_called_once_with(root, sign_kind='distribution', debug_mode=False)

    def test_final_docker_signs_on_host_without_passing_credentials(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(targets.subprocess, 'run') as run:
            finalizer = Mock()
            targets.handle_target_args(['--target', 'android', '--docker', '--final'], Path(temp), Mock(), Mock(), finalizer)
            self.assertEqual([c.kwargs['prepare'] for c in finalizer.call_args_list], [True, False])
            command = run.call_args_list[-1].args[0]
            self.assertNotIn('--final', command)
            self.assertFalse(any('PASSWORD' in arg or 'KEYSTORE' in arg for arg in command))

    def test_final_skips_simulator_signing(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(targets.platform, 'system', return_value='Darwin'), patch.object(targets.subprocess, 'run'):
            finalizer, package = Mock(), Mock()
            targets.handle_target_args(['--target', 'ios_sim', '--final'], Path(temp), Mock(), package, finalizer)
            finalizer.assert_not_called()
            package.assert_not_called()

    def test_final_legacy_platform_spelling(self):
        with patch.object(targets.platform, 'system', return_value='Darwin'):
            finalizer = Mock()
            self.assertTrue(targets.handle_target_args(['macos', '--final', '--dry-run'], ROOT, Mock(), Mock(), finalizer))
            finalizer.assert_not_called()

    def test_missing_final_credentials_prevent_build(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(targets.platform, 'system', return_value='Darwin'), patch.object(targets.subprocess, 'run') as run:
            builder = Mock()
            with self.assertRaises(SystemExit):
                targets.handle_target_args(['--target', 'macos', '--final'], Path(temp), builder, Mock(), Mock(side_effect=RuntimeError('missing credentials')))
            builder.assert_not_called()
            run.assert_not_called()

    def test_notary_explicit_profile_wins_without_enumeration(self):
        with patch.dict(os.environ, {'NOTARY_PROFILE': 'chosen'}, clear=True), patch.object(build.subprocess, 'run') as run:
            self.assertEqual(build._notarization_auth(ROOT), ['--keychain-profile', 'chosen'])
            run.assert_not_called()

    def test_notary_sole_profile_is_detected_and_ambiguity_rejected(self):
        for profiles, success in [(['one'], True), ([], False), (['one', 'two'], False)]:
            with self.subTest(profiles=profiles), patch.dict(os.environ, {}, clear=True), patch.object(build.subprocess, 'run', return_value=Mock(stdout=json.dumps(profiles))):
                if success: self.assertEqual(build._notarization_auth(ROOT), ['--keychain-profile', 'one'])
                else:
                    with self.assertRaises(RuntimeError): build._notarization_auth(ROOT)

    def test_notary_complete_account_credentials_and_partial_failure(self):
        with patch.dict(os.environ, {'NOTARY_APPLE_ID': 'account', 'NOTARY_TEAM_ID': 'team', 'NOTARY_PASSWORD': 'secret'}, clear=True), patch.object(build.subprocess, 'run') as run:
            self.assertEqual(build._notarization_auth(ROOT)[::2], ['--apple-id', '--team-id', '--password'])
            run.assert_not_called()
        with patch.dict(os.environ, {'NOTARY_APPLE_ID': 'account'}, clear=True):
            with self.assertRaises(RuntimeError): build._notarization_auth(ROOT)

    def test_notary_password_is_redacted_from_command_logs(self):
        command = ["xcrun", "notarytool", "submit", "artifact.dmg", "--password", "private-secret"]
        self.assertNotIn("private-secret", build._cmd_to_str(command))
        self.assertIn("<redacted>", build._cmd_to_str(command))

    def test_macos_finalizer_runs_notarization(self):
        with patch.object(build, "notarize_macos") as notarize:
            build.finalize_release(ROOT, "macos", "apk", prepare=False)
            notarize.assert_called_once_with(ROOT)

    def test_android_final_requires_release_keystore(self):
        with patch.object(build, '_android_signing_settings', return_value={'ANDROID_KEYSTORE_PATH': '/nonexistent/release.jks'}):
            with self.assertRaises(RuntimeError): build._require_android_release_settings(ROOT)

    def test_android_final_signs_verifies_then_replaces_without_password_arguments(self):
        for kind in ['apk', 'aab']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                artifact = root / f'{build.ANDROID_APP_NAME}.{kind}'
                artifact.write_text('original')
                settings = dict(ANDROID_KEYSTORE_PATH='/keys/release.jks', ANDROID_KEY_ALIAS='upload', ANDROID_KEYSTORE_TYPE='JKS', ANDROID_KEYSTORE_PASSWORD='store-secret', ANDROID_KEY_PASSWORD='key-secret')
                commands = []
                def run(command, **kwargs):
                    commands.append(command)
                    flag = '--out' if kind == 'apk' else '-signedjar'
                    if flag in command: Path(command[command.index(flag)+1]).write_text('signed')
                    self.assertEqual(kwargs['env']['GS_ANDROID_STORE_PASSWORD'], 'store-secret')
                with patch.object(build, '_require_android_release_settings', return_value=settings), patch.object(build, 'dist_dir', return_value=root), patch.object(build, '_detect_android_sdk_root', return_value=None), patch.object(build.shutil, 'which', return_value='/tools/apksigner'), patch.object(build, 'run', side_effect=run), patch.object(build, 'run_capture', return_value='jar verified.'):
                    build._finalize_android_release(root, kind)
                self.assertEqual(artifact.read_text(), 'signed')
                self.assertNotIn('store-secret', repr(commands))
                self.assertNotIn('key-secret', repr(commands))
                if kind == 'apk': self.assertEqual(commands[-1][1], 'verify')

    def test_android_failed_verification_preserves_original(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifact = root / f'{build.ANDROID_APP_NAME}.aab'
            artifact.write_text('original')
            settings = dict(ANDROID_KEYSTORE_PATH='/keys/release.jks', ANDROID_KEY_ALIAS='upload', ANDROID_KEYSTORE_TYPE='JKS', ANDROID_KEYSTORE_PASSWORD='secret', ANDROID_KEY_PASSWORD='secret')
            def sign(command, **kwargs): Path(command[command.index('-signedjar')+1]).write_text('bad-signature')
            with patch.object(build, '_require_android_release_settings', return_value=settings), patch.object(build, 'dist_dir', return_value=root), patch.object(build, 'run', side_effect=sign), patch.object(build, 'run_capture', return_value='jar is unsigned.'):
                with self.assertRaises(RuntimeError): build._finalize_android_release(root, 'aab')
            self.assertEqual(artifact.read_text(), 'original')

if __name__ == '__main__': unittest.main()
