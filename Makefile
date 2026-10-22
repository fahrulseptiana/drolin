.PHONY: all build check test guest ubuntu kernel android-kernel passt passt-android apk run-demo run-ubuntu clean

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

passt:
	./net/build-passt.sh

passt-android:
	MODE=android ./net/build-passt.sh

apk:
	./android/build-apk.sh

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
		--init /sbin/init \
		--net --passt net/dist/passt-host \
		--marker DROLIN_GUEST_OK

clean:
	cargo clean
	rm -rf images/dist images/build kernel/build kernel/build-android kernel/dist android/build android/dist
