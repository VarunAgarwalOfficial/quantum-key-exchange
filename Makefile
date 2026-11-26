.PHONY: check lint

check:
	cargo build --workspace
	cargo test --workspace

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	npx --yes @biomejs/biome@2.6.7 check
