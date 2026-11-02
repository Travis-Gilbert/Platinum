"""Exercise the verifier against real Mach-O files and resource signatures."""
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
loader = importlib.machinery.SourceFileLoader("package_macos", str(ROOT / "script/package-macos"))
spec = importlib.util.spec_from_loader(loader.name, loader)
pack = importlib.util.module_from_spec(spec)
loader.exec_module(pack)


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="platinum-package-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.app = self.root / "Platinum.app"
        self.macos = self.app / "Contents/MacOS"
        self.frameworks = self.app / "Contents/Frameworks"
        self.resources = self.app / "Contents/Resources"
        for path in (self.macos, self.frameworks, self.resources):
            path.mkdir(parents=True)
        (self.root / "lib.c").write_text("int answer(void) { return 42; }\n")
        (self.root / "main.c").write_text("int answer(void); int main(void) { return answer() != 42; }\n")
        self.library = self.frameworks / "libanswer.dylib"
        self.executable = self.macos / "frame"
        subprocess.run(["xcrun", "clang", "-dynamiclib", str(self.root / "lib.c"), "-o", str(self.library),
                        "-Wl,-install_name,/tmp/external/libanswer.dylib", "-Wl,-headerpad_max_install_names"], check=True)
        subprocess.run(["xcrun", "clang", str(self.root / "main.c"), str(self.library), "-o", str(self.executable),
                        "-Wl,-headerpad_max_install_names"], check=True)
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleExecutable": "frame", "CFBundleIdentifier": "dev.platinum.packaging-test",
            "CFBundlePackageType": "APPL", "LSMinimumSystemVersion": "99.0.0"}))
        (self.resources / "platinum-bundle.json").write_text(json.dumps({"plugins": []}))

    def portable(self):
        pack.run("install_name_tool", "-change", "/tmp/external/libanswer.dylib",
                 "@executable_path/../Frameworks/libanswer.dylib", self.executable, stderr=subprocess.STDOUT)
        pack.run("install_name_tool", "-id", "@executable_path/../Frameworks/libanswer.dylib",
                 self.library, stderr=subprocess.STDOUT)

    def test_external_dependency_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "External dependency"):
            pack.verify(self.app)

    def test_missing_library_is_rejected(self):
        self.portable()
        self.library.unlink()
        with self.assertRaisesRegex(ValueError, "Unresolved dependency"):
            pack.verify(self.app)

    def test_missing_dynamically_loaded_module_is_rejected(self):
        self.portable()
        (self.resources / "platinum-bundle.json").write_text(json.dumps({
            "plugins": [], "gio_modules": ["libgiognutls.so"]}))
        with self.assertRaisesRegex(ValueError, "Missing GIO module"):
            pack.verify(self.app)

    def test_external_rpath_is_rejected(self):
        self.portable()
        pack.run("install_name_tool", "-add_rpath", "/opt/homebrew/lib", self.executable, stderr=subprocess.STDOUT)
        with self.assertRaisesRegex(ValueError, "External/unsupported rpath"):
            pack.verify(self.app)

    def test_external_symlink_is_rejected(self):
        self.portable()
        (self.resources / "outside").symlink_to(self.root / "main.c")
        with self.assertRaisesRegex(ValueError, "Broken or external symlink"):
            pack.verify(self.app)

    def test_signed_bundle_runs_and_resource_tampering_fails(self):
        self.portable()
        # Match the actual plugin layout: code directories cannot contain
        # arbitrary subdirectories that codesign mistakes for nested bundles.
        (self.macos / "lib").symlink_to("../Frameworks")
        plugins = self.app / "Contents/PlugIns"
        plugins.mkdir()
        (plugins / "libmodule.so").write_bytes(self.library.read_bytes())
        result = pack.sign(self.app, "-")
        self.assertTrue(result["signatures_verified"])
        subprocess.run([str(self.executable)], check=True, cwd="/")
        (self.resources / "platinum-bundle.json").write_text('{"plugins": [], "tampered": true}')
        with self.assertRaises(subprocess.CalledProcessError):
            pack.verify(self.app, signatures=True)

    def test_adhoc_is_not_accepted_as_developer_id(self):
        self.portable()
        pack.sign(self.app, "-")
        with self.assertRaisesRegex(ValueError, "Developer ID"):
            pack.verify(self.app, developer_id=True)

    def test_missing_identity_does_not_mutate_bundle(self):
        self.portable()
        before = pack.digest(self.executable)
        with self.assertRaisesRegex(ValueError, "valid Developer ID"):
            pack.sign(self.app, "PLATINUM-NONEXISTENT-TEST-IDENTITY")
        self.assertEqual(before, pack.digest(self.executable))


if __name__ == "__main__":
    unittest.main()
