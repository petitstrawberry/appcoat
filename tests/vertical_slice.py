#!/usr/bin/env python3
"""Real host processes and a non-Cargo build, with all temporary artifacts scoped here."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

PROJECT = Path(__file__).resolve().parents[1]
CLI = PROJECT / "target/debug/appcoat"
HOST = "macos" if sys.platform == "darwin" else "linux"


class VerticalSlice(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="appcoat-e2e-", dir=PROJECT / "target")
        self.root = Path(self.temp.name)
        self.foreign_cwd = self.root / "unrelated cwd"
        self.foreign_cwd.mkdir()
        self.env = os.environ.copy()
        self.env.pop("APPCOAT_RESOURCE_ROOT", None)

    def tearDown(self):
        self.temp.cleanup()

    def call(self, *args, good=True):
        result = subprocess.run([str(CLI), *map(str, args)], cwd=self.foreign_cwd,
                                env=self.env, text=True, capture_output=True)
        if good:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0)
        return result

    def pack(self, example, name, *extra):
        output = self.root / name
        self.call("package", "--manifest", PROJECT / f"examples/{example}/appcoat.toml",
                  "--platform", HOST, "--output", output, *extra)
        return output

    def test_rust_direct_launch_relocation_and_argument_contract(self):
        suffix = ".app" if HOST == "macos" else ""
        package = self.pack("resource-reader", "reader" + suffix,
                            "--artifact", PROJECT / "target/debug/appcoat_resource_reader")
        output = self.call("run", "--package", package, "--", "space inside", "日本語").stdout
        self.assertIn("Hello from an appcoat resource!", output)
        self.assertIn("arg=space inside\narg=日本語", output)
        self.assertIn(f"cwd={self.foreign_cwd}", output)
        logical_root = Path(self.call("resource-root", "--package", package).stdout.strip())
        self.assertEqual((logical_root / "greeting.txt").read_text(), "Hello from an appcoat resource!\n")
        moved = self.root / ("moved with spaces" + suffix)
        shutil.move(package, moved)
        executable = moved / ("Contents/MacOS/resource-reader" if HOST == "macos" else "bin/resource-reader")
        direct = subprocess.run([str(executable)], cwd=self.foreign_cwd, env=self.env,
                                text=True, capture_output=True)
        self.assertEqual(direct.returncode, 0, direct.stderr)
        self.assertIn(str(moved), direct.stdout)
        self.assertIn("Hello from an appcoat resource!", direct.stdout)
        if HOST == "macos":
            subprocess.run(["/usr/bin/plutil", "-lint", str(moved / "Contents/Info.plist")], check=True, capture_output=True)

    def test_non_cargo_command_builds_and_reads_resource(self):
        package = self.pack("custom-c", "custom-c.app" if HOST == "macos" else "custom-c", "--build")
        output = self.call("run", "--package", package, "--", "custom command").stdout
        self.assertIn("Hello from C, built without Cargo!", output)
        self.assertIn("arg=custom command", output)

    def test_cargo_adapter_finds_custom_target_directory(self):
        self.env["CARGO_TARGET_DIR"] = str(self.root / "separate cargo target")
        package = self.pack("resource-reader", "cargo-output.app" if HOST == "macos" else "cargo-output", "--build")
        self.assertTrue((Path(self.env["CARGO_TARGET_DIR"]) / "debug/appcoat_resource_reader").is_file())
        self.assertIn("Hello from an appcoat resource!", self.call("run", "--package", package).stdout)

    def test_consumer_build_is_opt_in_and_existing_output_is_safe(self):
        definition = self.root / "definition"
        definition.mkdir()
        (definition / "appcoat.toml").write_text('''[app]
id="dev.appcoat.test"
slug="test"
name="Test"
version="0.1.0"
executable="test"
[build]
command=["sh", "build.sh"]
artifact="program"
''')
        (definition / "build.sh").write_text("set -eu\ntouch build-ran\nprintf '#!/bin/sh\\nexit 23\\n' > program\nchmod +x program\n")
        package = self.root / ("opt-in.app" if HOST == "macos" else "opt-in")
        args = ["package", "--manifest", definition / "appcoat.toml", "--platform", HOST, "--output", package]
        self.call(*args, good=False)
        self.assertFalse((definition / "build-ran").exists())
        self.call(*args, "--build")
        result = self.call("run", "--package", package, good=False)
        self.assertEqual(result.returncode, 23)
        (definition / "build-ran").unlink()
        self.call(*args, "--build", good=False)
        self.assertFalse((definition / "build-ran").exists())
        self.assertTrue(package.exists())

    @unittest.skipUnless(sys.platform == "darwin", "requires macOS native packaging tools")
    def test_macos_library_closure_survives_removing_original_libraries(self):
        build = self.root / "original libraries"
        build.mkdir()
        (build / "one.c").write_text("int one(void) { return 41; }\n")
        (build / "two.c").write_text("extern int one(void); int two(void) { return one() + 1; }\n")
        (build / "main.c").write_text('#include <stdio.h>\nextern int two(void); int main(void) { printf("%d\\n", two()); }\n')
        one, two, main = (build / name for name in ("libone.dylib", "libtwo.dylib", "app"))
        for command in [
            ["/usr/bin/cc", "-dynamiclib", str(build / "one.c"), "-o", str(one), "-Wl,-install_name," + str(one)],
            ["/usr/bin/cc", "-dynamiclib", str(build / "two.c"), str(one), "-o", str(two), "-Wl,-install_name," + str(two)],
            ["/usr/bin/cc", str(build / "main.c"), str(two), "-o", str(main)],
        ]:
            subprocess.run([*command, "-Wl,-headerpad_max_install_names"], check=True, capture_output=True)
        manifest = self.root / "native.toml"
        manifest.write_text('[app]\nid="dev.test.native"\nslug="native"\nname="Native"\nversion="1.0.0"\nexecutable="native"\n[macos]\nbundle-libraries=true\n')
        app = self.root / "Native.app"
        self.call("package", "--manifest", manifest, "--platform", "macos", "--artifact", main, "--output", app)
        moved = self.root / "Moved Native.app"
        app.rename(moved)
        shutil.rmtree(build)
        self.assertEqual(self.call("run", "--package", moved).stdout, "42\n")
        subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(moved)], check=True, capture_output=True)
        for name in ("libone.dylib", "libtwo.dylib"):
            library = moved / "Contents/Frameworks" / name
            self.assertTrue(library.is_file())
            report = subprocess.run(["/usr/bin/otool", "-L", str(library)], check=True, capture_output=True, text=True).stdout
            self.assertNotIn(str(build), report)


    @unittest.skipUnless(sys.platform == "darwin", "requires macOS native packaging tools")
    def test_unresolved_macos_rpath_does_not_publish_or_leave_staging(self):
        source = self.root / "rpath.c"
        source.write_text("int answer(void) { return 42; }\n")
        library = self.root / "librpath.dylib"
        subprocess.run(["/usr/bin/cc", "-dynamiclib", str(source), "-o", str(library),
                        "-Wl,-install_name,@rpath/librpath.dylib"], check=True, capture_output=True)
        source.write_text("extern int answer(void); int main(void) { return answer(); }\n")
        main = self.root / "rpath-app"
        subprocess.run(["/usr/bin/cc", str(source), str(library), "-o", str(main)], check=True, capture_output=True)
        manifest = self.root / "rpath.toml"
        manifest.write_text('[app]\nid="dev.test.rpath"\nslug="rpath"\nname="Rpath"\nversion="1.0.0"\nexecutable="rpath"\n[macos]\nbundle-libraries=true\n')
        output = self.root / "Rpath.app"
        result = self.call("package", "--manifest", manifest, "--platform", "macos", "--artifact", main,
                          "--output", output, good=False)
        self.assertIn("resolve @rpath", result.stderr)
        self.assertFalse(output.exists())
        self.assertEqual(list(self.root.glob(".appcoat-stage-*")), [])

    def test_bad_cli_and_cross_platform_artifact_do_not_publish(self):
        self.call("package", "--typo", "x", good=False)
        self.call("resource-root", "--package", self.root, "--unknown", "x", good=False)
        output = self.root / "resource-reader.app"
        result = self.call("package", "--manifest", PROJECT / "examples/resource-reader/appcoat.toml",
                          "--platform", "scarlet", "--target", "aarch64-unknown-scarlet",
                          "--artifact", PROJECT / "target/debug/appcoat_resource_reader",
                          "--output", output, good=False)
        self.assertIn("native Scarlet ELF64", result.stderr)
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
