"""Offline original-image verifier and unqualified allocation proposal generator.

No device interface, formatting, migration or physical writes exist in this tool.
"""
import argparse
from dataclasses import asdict, dataclass
import hashlib
import json
from pathlib import Path
import sys


@dataclass(frozen=True)
class Region:
    name: str
    role: str
    start: int
    end: int
    sha256: str


SOURCE_BYTES = 4194304
SOURCE_SHA256 = 'cc7533616efdff0ebe7acd275ae5ec89d3ff890bb21ec01409c395b69b47dc29'
REGIONS = (
    Region('font-and-padding', 'preserve', 0, 0x1a3000,
           '42a1de731358af9ac6b0ca2ab359d81c9b691ed6997737e9e6fc749b17045a93'),
    Region('proposed-settings-bank-a', 'proposed-settings', 0x1a3000, 0x1a5000,
           '43e7b36c116c91570993bb836b074f34e76a610deca54e044f07a42955fa1c74'),
    Region('proposed-settings-bank-b', 'proposed-settings', 0x1a5000, 0x1a7000,
           'e29804f89d188787fc8c011721fdd38bc7886a12728ba030417636aad4e574b4'),
    Region('legacy-data', 'preserve', 0x1a7000, 0x1bf000,
           '4ed66ad97b23d556ecc94fd45fdcdf07f6b9d88445424304ccaf4f8bc1017fc3'),
    Region('logo-applet-ui', 'preserve', 0x1bf000, 0x400000,
           '3651c2cf0ab721d6c81606de3f509c3ddac4bb99d90b954571f52e7ef214f872'),
)


def verify_image(image, expected_size, expected_sha256, regions):
    """Check pinned original image and complete disjoint sector-aligned ranges."""
    if expected_size <= 0 or expected_size % 4096 or len(image) != expected_size:
        raise ValueError('original image length differs from the pinned sector extent')
    if hashlib.sha256(image).hexdigest() != expected_sha256:
        raise ValueError('original image SHA256 differs from the pinned capture')
    ordered = sorted(regions, key=lambda region: region.start)
    end = 0
    names = set()
    result = []
    for region in ordered:
        if (region.name in names or region.role not in ('preserve', 'proposed-settings')
                or region.start != end or region.end <= region.start
                or region.end > expected_size or region.start % 4096 or region.end % 4096):
            raise ValueError('overlap, gap, bounds, role or alignment in proposal regions')
        digest = hashlib.sha256(image[region.start:region.end]).hexdigest()
        if digest != region.sha256:
            raise ValueError(f'original region SHA256 differs: {region.name}')
        record = asdict(region)
        record['bytes'] = region.end - region.start
        result.append(record)
        end = region.end
        names.add(region.name)
    if end != expected_size:
        raise ValueError('proposal does not account for the entire original image')
    return result


def write_new(output, proposal):
    """Create one local audit artifact; never overwrite any existing path."""
    contents = json.dumps(proposal, indent=2, sort_keys=True) + '\n'
    with Path(output).open('x', encoding='utf-8') as stream:
        stream.write(contents)


def build_proposal(image):
    ranges = verify_image(image, SOURCE_BYTES, SOURCE_SHA256, REGIONS)
    return {
        'format': 'wattch5-settings-provisioning-proposal-v1',
        'qualification': 'unqualified',
        'provisioned': False,
        'device_io': False,
        'writer_binding': 'unavailable',
        'source': {'bytes': SOURCE_BYTES, 'sha256': SOURCE_SHA256},
        'observed_jedec': '5e4016',
        'identity_limit': 'Conventional 4MiB density only; exact part not identified.',
        'allocation': {
            'purpose': 'new-product settings only; proposed legacy CFG reassignment',
            'start': 0x1a3000, 'end': 0x1a7000, 'bank_bytes': 8192,
            'write_size': 256, 'payload_bound': 256,
            'initialization': 'none; this proposal never formats or mutates a device',
        },
        'required_evidence': [
            'exact part and complete status/lock/protection/pin state',
            'program/erase/transfer and foreground scheduling maxima',
            'ordered durability, repeated partial programming and byte-fault model',
            'exclusive product provisioning identity and legacy CFG reassignment',
            'fresh separately authorized physical preflight and verified recovery',
        ],
        'regions': ranges,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--capture', type=Path, required=True,
                        help='existing pinned original capture, read only')
    parser.add_argument('--output', type=Path, required=True,
                        help='new local JSON path; refuses an existing path')
    args = parser.parse_args(argv)
    try:
        proposal = build_proposal(args.capture.read_bytes())
        proposal['source']['path'] = str(args.capture.resolve())
        write_new(args.output, proposal)
    except (OSError, ValueError) as error:
        print(f'proposal rejected: {error}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
