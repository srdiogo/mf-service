PREFIX ?= /usr
PYTHON ?= python3
DESTDIR ?=

.PHONY: build check check-driver check-distribution check-tools arch-source install uninstall
build:
	cargo build --release --locked
check:
	cargo fmt --all --check
	cargo test --workspace --locked
	cargo clippy --workspace --all-targets --locked -- -D warnings
check-tools:
	$(PYTHON) -m unittest discover -s tools/tests -v
	bash -n distribution/install.sh
check-driver:
	$(MAKE) -C kmod $(if $(KDIR),KDIR="$(KDIR)") W=1
arch-source:
	$(PYTHON) tools/prepare_arch_package.py --output dist/arch
check-distribution:
	$(PYTHON) tools/check_distribution.py
install:
	install -Dm755 target/release/mf-service "$(DESTDIR)$(PREFIX)/bin/mf-service"
	install -Dm644 packaging/mf-service.service.in "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	$(PYTHON) tools/render_unit.py "$(PREFIX)/bin/mf-service" "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	install -Dm755 tools/minifuse_doctor.py "$(DESTDIR)$(PREFIX)/bin/minifuse-doctor"
	install -Dm644 crates/mf-core/LICENSE "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/core-LICENSE"
	install -Dm644 LICENSE "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE"
uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/mf-service"
	rm -f "$(DESTDIR)$(PREFIX)/bin/minifuse-doctor"
	rm -f "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/core-LICENSE"
	rm -f "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	rm -f "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE"
