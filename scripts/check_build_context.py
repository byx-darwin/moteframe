#!/usr/bin/env python3
"""Validate exact reviewed Cargo graph migrations without changing cost baselines."""
import copy
import hashlib
import json
from pathlib import Path

POLICY = Path(__file__).resolve().parent / 'build-context-migrations.json'
POLICY_SHA256 = '0caedfa54ca20d5aaf6dde93d49bb22af388178b5494f45d94e9126a5c2ea6c0'


def load_policy(path=POLICY, expected_hash=None):
    expected_hash = POLICY_SHA256 if expected_hash is None else expected_hash
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected_hash:
        raise ValueError('workspace context migration checksum differs; explicit review required')
    policy = json.loads(data)
    if not isinstance(policy, dict) or policy.get('format_version') != 1 or not isinstance(policy.get('transitions'), list):
        raise ValueError('invalid workspace context migration record')
    return policy


def cargo_graph(context):
    try:
        lock = context['lock_sha256']
        manifests = context['cargo_configuration']['manifests']
    except (KeyError, TypeError) as error:
        raise ValueError('missing exact Cargo graph evidence') from error
    if not isinstance(lock, str) or not isinstance(manifests, dict) or not manifests:
        raise ValueError('invalid Cargo graph evidence')
    return {'lock_sha256': lock, 'manifests': manifests}


def remainder(context):
    cargo_graph(context)
    rest = copy.deepcopy(context)
    del rest['lock_sha256']
    del rest['cargo_configuration']['manifests']
    return rest


def validate_transition(before, after, reference_sha256, policy):
    if before == after:
        return None
    if remainder(before) != remainder(after):
        raise ValueError('compiler/profile/probe/linker/config context differs from frozen reference')
    if policy.get('format_version') != 1:
        raise ValueError('invalid migration format')
    old_graph, new_graph = cargo_graph(before), cargo_graph(after)
    for transition in policy.get('transitions', []):
        if (transition.get('reference_sha256') == reference_sha256
                and transition.get('before') == old_graph and transition.get('after') == new_graph):
            return transition['name']
    raise ValueError('unreviewed Cargo manifest/lock context transition')


def validate_context(before, after, reference_sha256):
    if before == after:
        return None
    return validate_transition(before, after, reference_sha256, load_policy())
