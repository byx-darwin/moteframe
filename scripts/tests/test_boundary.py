"""Behavioral regressions for the portable-source publication boundary."""
from pathlib import Path
import importlib.util
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "check_boundary.py"
SPEC = importlib.util.spec_from_file_location("boundary", SCRIPT) if SCRIPT.exists() else None
BOUNDARY = importlib.util.module_from_spec(SPEC) if SPEC is not None else None
if SPEC is not None:
    SPEC.loader.exec_module(BOUNDARY)


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        if BOUNDARY is None:
            self.fail("portable-source boundary validator is not implemented")
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write("Cargo.toml", "[workspace]\n")
        self.write("README.md", "# MoteFrame\n")
        self.write("crates/moteframe-core/src/lib.rs", "#![no_std]\n#![forbid(unsafe_code)]\n")

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def metadata(self):
        packages = []
        for name in sorted(BOUNDARY.PACKAGES):
            packages.append({
                "id": name,
                "name": name,
                "manifest_path": str(self.root / "crates" / name / "Cargo.toml"),
                "dependencies": [],
                "license": "Apache-2.0",
                "source": None,
                "targets": [{"kind": ["lib"], "src_path": str(self.root / "crates" / name / "src/lib.rs")}],
            })
        return {"workspace_root": str(self.root), "workspace_members": [p["id"] for p in packages], "packages": packages,
                "resolve": {"nodes": [{"id": p["id"], "dependencies": []} for p in packages]}}

    def add_registry_package(self, metadata):
        package = {"id": "registry-hal", "name": "embedded-hal", "source": BOUNDARY.REGISTRY,
                   "manifest_path": str(self.root.parent / "registry/embedded-hal/Cargo.toml")}
        metadata["packages"].append(package)
        metadata["resolve"]["nodes"].append({"id": package["id"], "dependencies": []})
        return package

    def test_should_scan_nested_artifact_directories(self):
        for directory in ["target", ".superpowers", ".git"]:
            with self.subTest(directory=directory):
                self.write("docs/" + directory + "/private.png", "private")
                self.assertTrue(any("binary/resource" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_symlinks_under_nested_artifact_directories(self):
        for directory in ["target", ".superpowers"]:
            with self.subTest(directory=directory):
                path = self.root / "docs" / directory
                path.mkdir(parents=True)
                (path / "private.md").symlink_to(self.root.parent / "private.md")
                self.assertTrue(any("symlink" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_require_reviewed_document_paths(self):
        self.write("docs/target/private.md", "private text")
        self.assertTrue(any("outside portable scope" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_forced_tracking_of_excluded_artifacts(self):
        errors = BOUNDARY.validate_tracked(["target/private.bin", ".superpowers/private.md"])
        self.assertEqual(len(errors), 2)

    def test_should_ignore_only_the_reviewed_standalone_example_build_directory(self):
        self.write("examples/ui-low-memory/target/private.bin", "local build")
        self.assertEqual(BOUNDARY.validate_files(self.root), [])
        self.write("examples/other/target/private.bin", "unreviewed")
        self.assertTrue(BOUNDARY.validate_files(self.root))

    def test_should_reject_extra_workspace_members(self):
        metadata = self.metadata()
        metadata["workspace_members"].append("unreviewed")
        self.assertTrue(BOUNDARY.validate_metadata(metadata, self.root))

    def test_should_accept_only_reviewed_tracked_paths(self):
        self.assertEqual(BOUNDARY.validate_tracked(["README.md", "crates/moteframe-core/src/lib.rs"]), [])

    def test_should_reject_resolved_external_path_patch(self):
        metadata = self.metadata()
        package = self.add_registry_package(metadata)
        package["source"] = None
        package["manifest_path"] = str(self.root.parent / "private/Cargo.toml")
        self.assertTrue(any("resolved dependency" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_accept_resolved_approved_registry_package(self):
        metadata = self.metadata()
        self.add_registry_package(metadata)
        self.assertEqual(BOUNDARY.validate_metadata(metadata, self.root), [])

    def test_should_reject_external_target_sources_including_build_scripts(self):
        for kind in ["lib", "test", "example", "custom-build"]:
            with self.subTest(kind=kind):
                metadata = self.metadata()
                metadata["packages"][0]["targets"] = [{"kind": [kind], "src_path": str(self.root.parent / "private.rs")}]
                self.assertTrue(any("target source" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_reject_unreviewed_internal_target_source(self):
        metadata = self.metadata()
        metadata["packages"][0]["targets"][0]["src_path"] = str(self.root / "crates/moteframe-core/src/private.rs")
        self.assertTrue(any("target source" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_require_full_resolved_graph(self):
        metadata = self.metadata()
        metadata["resolve"] = None
        self.assertTrue(any("resolved graph" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_reject_unknown_resolved_dependency(self):
        metadata = self.metadata()
        metadata["resolve"]["nodes"][0]["dependencies"] = ["unknown-private-package"]
        self.assertTrue(any("resolved graph" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_accept_portable_sources_and_ignore_local_build_artifacts(self):
        self.write("target/private-input.bin", "not distributed")
        self.assertEqual(BOUNDARY.validate_files(self.root), [])

    def test_should_reject_product_directories(self):
        self.write("boards/product/src/lib.rs", "// private board\n")
        self.assertTrue(any("outside portable scope" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_binary_resources_even_inside_an_allowed_directory(self):
        self.write("crates/moteframe-core/src/resource.bin", "private resource")
        self.assertTrue(any("binary/resource" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_unreviewed_rust_modules(self):
        self.write("crates/moteframe-ui/src/product.rs", "// unreviewed module\n")
        self.assertTrue(any("outside portable scope" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_source_symlinks(self):
        (self.root / "crates/moteframe-core/src/private.rs").symlink_to(self.root.parent)
        self.assertTrue(any("symlink" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_directory_symlinks_without_following_them(self):
        (self.root / "leaked").symlink_to(self.root.parent, target_is_directory=True)
        self.assertTrue(any("symlink" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_chip_and_product_identifiers_in_portable_rust(self):
        for name in ["SYD8810", "wattch5", "JLINKARM"]:
            with self.subTest(name=name):
                self.write("crates/moteframe-core/src/lib.rs", "// " + name)
                self.assertTrue(any("private/platform identifier" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_embedded_external_resources(self):
        self.write("crates/moteframe-core/src/lib.rs", 'const X: &[u8] = include_bytes!("secret");')
        self.assertTrue(any("embedded resource" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_reject_unsafe_platform_code(self):
        self.write("crates/moteframe-core/src/lib.rs", "unsafe extern \"C\" { fn rom(); }")
        self.assertTrue(any("unsafe/platform code" in e for e in BOUNDARY.validate_files(self.root)))

    def test_should_accept_only_internal_paths_and_approved_registry_dependencies(self):
        metadata = self.metadata()
        metadata["packages"][0]["dependencies"] = [
            {"name": "embedded-hal", "path": None, "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"name": "moteframe-i2c", "path": str(self.root / "crates/moteframe-i2c"), "source": None},
        ]
        self.assertEqual(BOUNDARY.validate_metadata(metadata, self.root), [])

    def test_should_reject_an_unintended_alternative_first_party_license(self):
        metadata = self.metadata()
        metadata["packages"][0]["license"] = "MIT OR Apache-2.0"
        self.assertTrue(any("first-party license" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_reject_dependency_path_escape(self):
        metadata = self.metadata()
        metadata["packages"][0]["dependencies"] = [{"name": "moteframe-i2c", "path": str(self.root / "../private"), "source": None}]
        self.assertTrue(any("escaping dependency" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_reject_unapproved_and_git_dependencies(self):
        for dependency in [
            {"name": "private-application", "path": None, "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"name": "embedded-hal", "path": None, "source": "git+https://example.invalid/repo"},
        ]:
            with self.subTest(dependency=dependency):
                metadata = self.metadata()
                metadata["packages"][0]["dependencies"] = [dependency]
                self.assertTrue(BOUNDARY.validate_metadata(metadata, self.root))

    def test_should_reject_extra_workspace_members(self):
        metadata = self.metadata()
        metadata["packages"].append({"id": "product", "name": "product", "manifest_path": str(self.root / "product/Cargo.toml"), "dependencies": []})
        metadata["workspace_members"].append("product")
        self.assertTrue(any("workspace members" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))

    def test_should_reject_manifests_outside_the_standalone_tree(self):
        metadata = self.metadata()
        metadata["packages"][0]["manifest_path"] = str(self.root / "../private/Cargo.toml")
        self.assertTrue(any("manifest outside" in e for e in BOUNDARY.validate_metadata(metadata, self.root)))


if __name__ == "__main__":
    unittest.main()
