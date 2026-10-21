.PHONY: all build check test guest ubuntu kernel android-kernel apk run-demo run-ubuntu clean

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

android-kernel:
	./kernel/build-android.sh

apk:
	./kotlin/build-apk.sh

run-demo: build guest
	./target/release/drolin run \
		--kernel kernel/dist/linux-um-$$(uname -m) \
		--initramfs images/dist/initramfs.cpio.gz \
		--marker DROLIN_GUEST_OK \
		--timeout 120

run-ubuntu: build
	./target/release/drolin run \
		--kernel kernel/dist/linux-um-$$(uname -m) \
		--rootfs images/dist/ubuntu-26.04.1-arm64.ext4 \
		--init /init \
		--marker DROLIN_GUEST_OK

clean:
	cargo clean
	rm -rf images/dist images/build kernel/build kernel/build-android kernel/dist kotlin/build kotlin/dist
