"""Portable metadata fixtures; no Apple executable is run or signed."""
import importlib.util
import os
from pathlib import Path
import plistlib
import struct
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).parents[1]
spec=importlib.util.spec_from_file_location('ios_platform_build',ROOT/'build.py')
build=importlib.util.module_from_spec(spec);sys.modules[spec.name]=build;spec.loader.exec_module(build)

def macho(platform=2,cpu=0x0100000c):
    # Header + one LC_BUILD_VERSION; parser fixture, not a runnable executable.
    return struct.pack('<8I',0xfeedfacf,cpu,0,2,1,24,0,0)+struct.pack('<6I',0x32,24,platform,14<<16,17<<16,0)

class IOSBundlePlatformTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);(self.root/'Cargo.toml').write_text('[package]\nname="groundstation_frontend"\n')
        self.environment=patch.dict(os.environ,{'CARGO_TARGET_DIR':str(self.root/'target')});self.environment.start();self.addCleanup(self.environment.stop)
    def bundle(self,platform=2,profile='release',name=None,cpu=0x0100000c):
        app=self.root/'target/dx/groundstation_frontend'/profile/'ios'/(name or build.APP_BUNDLE_NAME)
        app.mkdir(parents=True)
        (app/'Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable':'groundstation_frontend','CFBundleIdentifier':'com.UBSEDS.GS26','CFBundleSupportedPlatforms':['iPhoneOS' if platform==2 else 'iPhoneSimulator']}))
        (app/'groundstation_frontend').write_bytes(macho(platform,cpu));return app
    def stage(self,**kwargs):
        return build._stage_app_bundle_from_dx(self.root,platform_name='ios',preferred_bundle_name=build.APP_BUNDLE_NAME,**kwargs)
    def test_device_staging_accepts_real_device(self):
        original=self.bundle();staged=self.stage();self.assertEqual((staged/'groundstation_frontend').read_bytes(),(original/'groundstation_frontend').read_bytes())
    def test_device_staging_rejects_cached_arm64_simulator(self):
        self.bundle(platform=7)
        with self.assertRaises(RuntimeError):self.stage()
        self.assertFalse((self.root/'dist'/build.APP_BUNDLE_NAME).exists(),'must reject before copying or mutating metadata')
    def test_device_staging_does_not_fall_back_opposite_profile(self):
        self.bundle(profile='debug');self.assertIsNone(self.stage(debug_mode=False))
    def test_device_staging_rejects_ambiguous_same_profile(self):
        self.bundle();self.bundle(name=build.LEGACY_APP_BUNDLE_NAME)
        with self.assertRaises(RuntimeError):self.stage()

    def test_device_rejects_wrong_cpu_even_with_device_platform(self):
        self.bundle(cpu=0x01000007)
        with self.assertRaises(RuntimeError):self.stage()
    def test_plist_device_label_cannot_hide_simulator_binary(self):
        app=self.bundle(platform=7)
        info=plistlib.loads((app/'Info.plist').read_bytes());info['CFBundleSupportedPlatforms']=['iPhoneOS'];(app/'Info.plist').write_bytes(plistlib.dumps(info))
        with self.assertRaises(RuntimeError):self.stage()
    def test_non_native_resource_with_java_fat_magic_is_not_treated_as_code(self):
        app=self.bundle();(app/'Resource.class').write_bytes(bytes.fromhex('cafebabe0000003d')+b'java-resource')
        self.assertIsNotNone(self.stage())
    def test_embedded_root_dylib_platform_is_checked(self):
        app=self.bundle();(app/'app.debug.dylib').write_bytes(macho(7))
        with self.assertRaisesRegex(RuntimeError,'app.debug.dylib'):self.stage()
    def test_embedded_framework_platform_is_checked(self):
        app=self.bundle();framework=app/'Frameworks/Test.framework';framework.mkdir(parents=True)
        (framework/'Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable':'Test'}));(framework/'Test').write_bytes(macho(7))
        with self.assertRaisesRegex(RuntimeError,'Test.framework'):self.stage()
    def test_fat_device_payload_cannot_hide_simulator_slice(self):
        app=self.bundle();first=macho(2);second=macho(7)
        data=struct.pack('>2I',0xcafebabe,2)+struct.pack('>5I',0x0100000c,0,64,len(first),4)+struct.pack('>5I',0x0100000c,0,128,len(second),4)
        data=data.ljust(64,b'\0')+first;data=data.ljust(128,b'\0')+second;(app/'groundstation_frontend').write_bytes(data)
        with self.assertRaises(RuntimeError):self.stage()
    def test_arm64_simulator_is_valid_only_for_arm64_simulator_target(self):
        self.bundle(platform=7)
        self.assertIsNotNone(self.stage(ios_target='aarch64-apple-ios-sim'))
        with self.assertRaises(RuntimeError):self.stage()
    def test_simulator_target_requires_matching_platform_and_arch(self):
        self.bundle(platform=7,cpu=0x01000007)
        self.assertIsNotNone(self.stage(ios_target='x86_64-apple-ios'))
        with self.assertRaises(RuntimeError):self.stage(ios_target='aarch64-apple-ios-sim')
    def test_device_packaging_rejects_before_plist_or_signing(self):
        self.bundle(platform=7)
        with patch.object(build.platform,'system',return_value='Darwin'),patch.object(build,'patch_plist') as metadata,patch.object(build,'run') as signer:
            with self.assertRaises(RuntimeError):build.package_ios_ipa_with_script(self.root,sign_kind='development')
            metadata.assert_not_called();signer.assert_not_called()
    def test_correct_device_reaches_existing_signer_only_after_preflight(self):
        self.bundle();scripts=self.root/'scripts';scripts.mkdir();(scripts/'ios_package_sign.sh').write_text('# test-only signer hook')
        profile=self.root/'test.mobileprovision';profile.write_bytes(b'test-only, never decoded')
        info={'Entitlements':{'application-identifier':'TEST.com.UBSEDS.GS26','get-task-allow':True},'ProvisionedDevices':['test-only-device']}
        events=[]
        def metadata(*_args,**_kwargs):events.append('metadata')
        def signer(command,**_kwargs):events.append('signer');Path(command[-1]).write_bytes(b'test-only IPA hook')
        with patch.object(build.platform,'system',return_value='Darwin'),patch.object(build,'patch_plist',side_effect=metadata),patch.object(build,'ios_development_mobileprovision_path',return_value=profile),patch.object(build,'_decode_mobileprovision',return_value=info),patch.object(build,'run',side_effect=signer):
            result=build.package_ios_ipa_with_script(self.root,sign_kind='development')
        self.assertTrue(result.exists());self.assertEqual(events,['metadata','signer'])
    def test_dist_only_artifact_is_not_a_requested_profile(self):
        app=self.root/'dist'/build.APP_BUNDLE_NAME;app.mkdir(parents=True)
        (app/'Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable':'groundstation_frontend'}));(app/'groundstation_frontend').write_bytes(macho())
        with patch.object(build.platform,'system',return_value='Darwin'),patch.object(build,'patch_plist') as metadata:
            with self.assertRaises(FileNotFoundError):build.package_ios_ipa_with_script(self.root,sign_kind='development')
            metadata.assert_not_called()
    def test_fresh_cache_clear_preserves_other_profile(self):
        self.bundle();other=self.bundle(profile='debug');build.clear_ios_dx_bundle_cache(self.root,False)
        self.assertTrue(other.exists());self.assertIsNone(self.stage())
    def test_debug_simulator_capture_uses_debug_profile(self):
        self.bundle(platform=7,profile='debug',cpu=0x01000007)
        with patch.object(build.platform,'system',return_value='Darwin'),patch.object(build.platform,'machine',return_value='x86_64'),patch.object(build,'patch_plist'),patch.object(build,'ios_sim_deploy',return_value=('test-device','test.app')) as deploy,patch.object(build,'run'):
            build.capture_ios_sim_screenshot(self.root,debug_mode=True,output_dir=self.root,delay_seconds=0)
            deploy.assert_called_once_with(self.root,debug_mode=True)
    def test_macos_profile_fallback_is_unchanged(self):
        app=self.root/'target/dx/groundstation_frontend/debug/macos'/build.APP_BUNDLE_NAME;app.mkdir(parents=True);(app/'marker').write_text('macOS only')
        result=build._stage_app_bundle_from_dx(self.root,platform_name='macos',preferred_bundle_name=build.APP_BUNDLE_NAME)
        self.assertEqual((result/'marker').read_text(),'macOS only')

class IOSMetadataReaderTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.tools=build._ios_bundle_tools()
    def image(self,data):
        path=self.root/'payload';path.write_bytes(data);return path
    def test_exact_platform_not_just_arm64(self):
        self.assertEqual(self.tools.inspect_macho(self.image(macho(2)))[0]['platform'],2)
        self.assertEqual(self.tools.inspect_macho(self.image(macho(7)))[0]['platform'],7)
    def test_truncated_and_unidentified_platforms_are_rejected(self):
        for data in [macho()[:20],macho()[:40],macho()[:32]+struct.pack('<2I',0x32,0),struct.pack('<8I',0xfeedfacf,0x0100000c,0,2,0,0,0,0)]:
            with self.subTest(length=len(data)),self.assertRaises(RuntimeError):self.tools.inspect_macho(self.image(data))
    def test_fat_metadata_checks_all_slices(self):
        first=macho(2);second=macho(7)
        table=struct.pack('>2I',0xcafebabe,2)+struct.pack('>5I',0x0100000c,0,64,len(first),4)+struct.pack('>5I',0x0100000c,0,128,len(second),4)
        data=table.ljust(64,b'\0')+first;data=data.ljust(128,b'\0')+second
        self.assertEqual([r['platform'] for r in self.tools.inspect_macho(self.image(data))],[2,7])
    def test_fat_bounds_and_descriptor_mismatch_are_rejected(self):
        for offset,cpu in [(8,0x0100000c),(32,0x01000007)]:
            data=struct.pack('>2I',0xcafebabe,1)+struct.pack('>5I',cpu,0,offset,len(macho()),4)
            data=data.ljust(32,b'\0')+macho()
            with self.subTest(offset=offset,cpu=cpu),self.assertRaises(RuntimeError):self.tools.inspect_macho(self.image(data))
    def test_python_target_flags_cover_real_intel_simulator(self):
        for target,sdk,flag in [('aarch64-apple-ios','iphoneos','arm64-apple-ios13.0'),('aarch64-apple-ios-sim','iphonesimulator','arm64-apple-ios13.0-simulator'),('x86_64-apple-ios','iphonesimulator','x86_64-apple-ios13.0-simulator')]:
            with self.subTest(target=target):
                env=build._ensure_ios_deployment_env(self.root,{'RUSTFLAGS':'-C opt-level=2'},target)
                self.assertIn('link-arg='+flag,env['RUSTFLAGS']);self.assertIn('opt-level=2',env['RUSTFLAGS']);self.assertEqual(self.tools.target_spec(target)[2],sdk)

# Test-only PlistBuddy adapter: execute the actual shell logic, substituting only
# its unavailable Apple tool path. This does not validate Apple's tool or signing.
BUDDY_ADAPTER = r'''#!/usr/bin/env python3
import plistlib,shlex,sys
from pathlib import Path
try:
 command=shlex.split(sys.argv[sys.argv.index('-c')+1]);path=Path(sys.argv[-1]);data=plistlib.loads(path.read_bytes());parts=command[1].lstrip(':').split(':');parent=data
 for part in parts[:-1]:parent=parent[int(part)] if isinstance(parent,list) else parent[part]
 key=int(parts[-1]) if isinstance(parent,list) else parts[-1];operation=command[0]
 if operation=='Print':
  value=parent[key];print('true' if value is True else 'false' if value is False else value);sys.exit(0)
 if operation=='Delete':del parent[key]
 elif operation=='Add':
  kind=command[2];raw=' '.join(command[3:]);value={} if kind=='dict' else [] if kind=='array' else raw=='true' if kind=='bool' else int(raw) if kind=='integer' else raw
  if isinstance(parent,list):
   if key!=len(parent):raise ValueError('non-append array Add')
   parent.append(value)
  else:
   if key in parent:raise ValueError('duplicate Add')
   parent[key]=value
 elif operation=='Set':
  old=parent[key];raw=' '.join(command[2:]);parent[key]=raw=='true' if isinstance(old,bool) else int(raw) if isinstance(old,int) else raw
 else:raise ValueError('unsupported command')
 path.write_bytes(plistlib.dumps(data))
except Exception as error:
 print(type(error).__name__,file=sys.stderr);sys.exit(1)
'''

class IOSPlistMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
        (self.root/'Cargo.toml').write_text('[package]\nname="groundstation_frontend"\nversion="0.3.1"\n')
        (self.root/'Dioxus.toml').write_text('[application]\nbuild="36"\n')
        (self.root/'assets').mkdir();(self.root/'assets/Assets.car').write_bytes(b'test-only asset placeholder')
        (self.root/'scripts').mkdir();self.buddy=self.root/'plist-buddy-adapter';self.buddy.write_text(BUDDY_ADAPTER);self.buddy.chmod(0o755)
    def app(self,platform):
        app=self.root/'dist'/build.APP_BUNDLE_NAME;app.mkdir(parents=True,exist_ok=True)
        info={'CFBundleExecutable':'groundstation_frontend','CFBundleIdentifier':'com.UBSEDS.GS26','DTPlatformName':'iphoneos','DTSDKName':'iphoneos17.0'}
        (app/'Info.plist').write_bytes(plistlib.dumps(info));(app/'groundstation_frontend').write_bytes(macho(platform));return app
    def script(self,source):
        needle='PB="/usr/libexec/PlistBuddy"';self.assertEqual(source.count(needle),1)
        script=self.root/'scripts/patch_plist.sh';script.write_text(source.replace(needle,'PB='+shlex.quote(str(self.buddy))))
        return script
    def test_simulator_binary_gets_simulator_labels_after_validation(self):
        app=self.app(7);self.script((ROOT/'scripts/patch_plist.sh').read_text())
        build.patch_plist(self.root,app,ios_target='aarch64-apple-ios-sim')
        info=plistlib.loads((app/'Info.plist').read_bytes());self.assertEqual(info['CFBundleSupportedPlatforms'],['iPhoneSimulator']);self.assertEqual(info['DTPlatformName'],'iphonesimulator');self.assertEqual(info['DTSDKName'],'iphonesimulator17.0')
        self.assertEqual(build._ios_bundle_tools().inspect_macho(app/'groundstation_frontend')[0]['platform'],7,'metadata patch never rewrites binary platform')
    def test_legacy_manual_script_defaults_to_device_labels(self):
        app=self.app(2);script=self.script((ROOT/'scripts/patch_plist.sh').read_text());env=os.environ.copy();env.pop('GS26_IOS_BUNDLE_PLATFORM',None);env['APP_DIR']=str(app)
        subprocess.run(['bash',str(script),'0.3.1','36'],cwd=self.root,env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        info=plistlib.loads((app/'Info.plist').read_bytes());self.assertEqual(info['CFBundleSupportedPlatforms'],['iPhoneOS']);self.assertEqual(info['DTPlatformName'],'iphoneos')
    def test_scene_manifest_uses_registered_window_delegate_and_is_idempotent(self):
        app=self.app(2);self.script((ROOT/'scripts/patch_plist.sh').read_text())
        for _ in range(2):
            build.patch_plist(self.root,app,ios_target='aarch64-apple-ios')
        info=plistlib.loads((app/'Info.plist').read_bytes())
        manifest=info['UIApplicationSceneManifest']
        for key in ('NSCameraUsageDescription','NSMicrophoneUsageDescription','NSMotionUsageDescription'):
            self.assertTrue(info[key].strip())
        self.assertIs(manifest['UIApplicationSupportsMultipleScenes'],False)
        self.assertEqual(manifest['UISceneConfigurations']['UIWindowSceneSessionRoleApplication'],[{
            'UISceneConfigurationName':'Default Configuration',
            'UISceneClassName':'UIWindowScene',
            'UISceneDelegateClassName':'TaoSceneDelegate',
        }])
        self.assertEqual(build._ios_bundle_tools().inspect_macho(app/'groundstation_frontend')[0]['platform'],2)

    def test_wrong_binary_target_is_rejected_before_shell_patcher(self):
        app=self.app(7)
        with patch.object(build,'run_script') as shell:
            with self.assertRaises(RuntimeError):build.patch_plist(self.root,app,ios_target='aarch64-apple-ios')
            shell.assert_not_called()

if __name__=='__main__':unittest.main()
