PYTHON ?= python3
TARGET ?= thumbv6m-none-eabi

.PHONY: check fmt format lint test doc boundary python-tests build example memory ui-config core-compat i2c-compat portable assets-input ui-low-memory packages independent supply-chain secrets hooks-install hooks

check: fmt lint test doc boundary python-tests example memory ui-config ui-low-memory core-compat i2c-compat portable assets-input packages independent

fmt:
	cargo fmt --all -- --check

format:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets --all-features --offline --locked -- -D warnings
	cargo clippy --workspace --lib --all-features --target $(TARGET) --offline --locked -- -D warnings

test:
	cargo test --workspace --all-targets --all-features --offline --locked
	cargo test --workspace --doc --all-features --offline --locked

doc:
	RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --offline --locked

boundary:
	$(PYTHON) scripts/check_boundary.py

python-tests:
	$(PYTHON) -m unittest discover -s scripts/tests

build:
	cargo build --workspace --lib --target $(TARGET) --release --offline --locked

example:
	$(PYTHON) -c "from pathlib import Path; Path('target').mkdir(exist_ok=True)"
	cargo run -p moteframe-ui --example offline --offline --locked -- target/moteframe-demo.ppm
	$(PYTHON) scripts/check_example.py target/moteframe-demo.ppm

ui-config:
	$(PYTHON) scripts/check_ui_configuration.py
	$(PYTHON) scripts/check_ui_memory.py

core-compat:
	$(PYTHON) scripts/check_core_compatibility.py

i2c-compat:
	$(PYTHON) scripts/check_i2c_compatibility.py

ui-low-memory:
	$(PYTHON) scripts/check_ui_low_memory.py

portable:
	$(PYTHON) scripts/check_portable_modules.py

assets-input:
	$(PYTHON) scripts/check_assets_input.py

packages:
	$(PYTHON) scripts/check_packages.py

memory:
	$(PYTHON) scripts/check_memory.py

independent:
	cargo test --manifest-path crates/moteframe-core/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-display/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-ui/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-i2c/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-storage/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-ble/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-assets/Cargo.toml --all-targets --offline --locked
	cargo test --manifest-path crates/moteframe-input/Cargo.toml --all-targets --offline --locked

supply-chain:
	cargo deny --offline --locked check --disable-fetch

secrets:
	gitleaks dir . --redact --no-banner --max-target-megabytes 1 --timeout 60

hooks-install:
	pre-commit install --hook-type pre-commit --hook-type commit-msg

hooks:
	$(PYTHON) -c "import subprocess; files = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z']).decode().split('\0'); subprocess.run(['pre-commit', 'run', '--files'] + [f for f in files if f], check=True)"
