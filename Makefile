.PHONY: build run test release clean

build:
	cargo build

run:
	cargo run

test:
	cargo test

release:
	cargo build --release --locked

clean:
	cargo clean
