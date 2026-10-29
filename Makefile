PREFIX ?= /usr
PYTHON ?= python3
DESTDIR ?=

.PHONY: build check check-distribution install uninstall
build:
	cargo build --release --locked
check:
	cargo fmt --all --check
	cargo test --locked
	cargo clippy --all-targets --locked -- -D warnings
check-distribution:
	$(PYTHON) tools/check_distribution.py
install:
	install -Dm755 target/release/mf-service "$(DESTDIR)$(PREFIX)/bin/mf-service"
	install -Dm644 packaging/mf-service.service.in "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	$(PYTHON) tools/render_unit.py "$(PREFIX)/bin/mf-service" "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	install -Dm644 LICENSE "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE"
uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/mf-service"
	rm -f "$(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service"
	rm -f "$(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE"
