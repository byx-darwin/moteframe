"""Compile-fail acceptance must prove the actual compiler error and its owner."""
from pathlib import Path
import importlib.util
import json
import subprocess
import unittest

SPEC = importlib.util.spec_from_file_location('ui_configuration', Path(__file__).resolve().parents[1] / 'check_ui_configuration.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
SOURCE = Path('/fixture/consumer/src/lib.rs')


def message(code='E0432', text='unresolved import `ui::LayoutSettings`', level='error', source=SOURCE):
    return {'reason': 'compiler-message', 'target': {'src_path': str(source)},
            'message': {'level': level, 'code': {'code': code} if code else None,
                        'message': text, 'rendered': 'source snippet mentions LayoutSettings',
                        'spans': [{'file_name': str(source), 'is_primary': True}], 'children': []}}


def result(*messages, status=101, stderr=''):
    return subprocess.CompletedProcess([], status, '\n'.join(json.dumps(m) for m in messages), stderr)


class DiagnosticAcceptance(unittest.TestCase):
    def validate(self, output, success=False):
        return MODULE.validate_build(output, SOURCE, success, ('E0432', 'LayoutSettings'))

    def test_warning_keyword_cannot_mask_an_unrelated_error(self):
        output = result(message(None, 'unused import: `ui::LayoutSettings`', 'warning'),
                        message('E0425', 'cannot find function `missing`'))
        with self.assertRaises(RuntimeError):
            self.validate(output)

    def test_accepts_only_the_expected_error_owned_by_the_consumer(self):
        errors = self.validate(result(message()))
        self.assertEqual(errors, [{'code': 'E0432', 'message': 'unresolved import `ui::LayoutSettings`'}])

    def test_relative_spans_are_resolved_against_the_consumers_manifest(self):
        diagnostic = message()
        diagnostic['manifest_path'] = '/fixture/consumer/Cargo.toml'
        diagnostic['message']['spans'][0]['file_name'] = 'src/lib.rs'
        self.assertEqual(len(self.validate(result(diagnostic))), 1)

    def test_an_expected_error_does_not_hide_an_additional_unrelated_error(self):
        with self.assertRaises(RuntimeError):
            self.validate(result(message(), message('E0308', 'mismatched types')))

    def test_source_snippet_does_not_count_as_the_error_subject(self):
        with self.assertRaises(RuntimeError):
            self.validate(result(message('E0432', 'unresolved import `other::Thing`')))

    def test_dependency_error_does_not_prove_the_consumer_was_rejected(self):
        with self.assertRaises(RuntimeError):
            self.validate(result(message(source=Path('/fixture/dependency/src/lib.rs'))))

    def test_other_file_primary_span_cannot_prove_this_use_failed(self):
        diagnostic = message()
        diagnostic['message']['spans'][0]['file_name'] = '/fixture/dependency/src/lib.rs'
        with self.assertRaises(RuntimeError):
            self.validate(result(diagnostic))

    def test_compiler_crash_or_missing_tool_is_not_a_compile_fail_success(self):
        for output in [result(stderr='rustc crashed: LayoutSettings'), result(message(None, 'compilation aborted'))]:
            with self.assertRaises(RuntimeError):
                self.validate(output)

    def test_success_requires_a_successful_exit_and_no_error_diagnostic(self):
        self.assertEqual(self.validate(result(status=0), True), [])
        for output in [result(status=101), result(message(), status=0)]:
            with self.assertRaises(RuntimeError):
                self.validate(output, True)

    def test_malformed_json_is_not_ignored(self):
        with self.assertRaises(RuntimeError):
            self.validate(subprocess.CompletedProcess([], 101, 'not a Cargo JSON record', ''))
        with self.assertRaises(RuntimeError):
            self.validate(subprocess.CompletedProcess([], 101, '[1]', ''))
