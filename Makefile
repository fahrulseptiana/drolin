.PHONY: all build check test guest kernel run-demo clean

all: check

build:
	cargo build --release

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

guest:
	./images/mkinitramfs.sh

ubuntu:
	./images/build-ubuntu.sh

kernel:
	./kernel/build.sh

run-demo: build guest
	./target/release/drolin run \
		--kernel kernel/dist/linux-um-$$(uname -m) \
		--initramfs images/dist/initramfs.cpio.gz \
		--marker DROLIN_GUEST_OK \
		--timeout 120

clean:
	cargo clean
	rm -rf images/dist kernel/build kernel/dist
