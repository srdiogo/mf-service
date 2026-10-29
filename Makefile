PREFIX ?= /usr
DESTDIR ?=

.PHONY: build check install uninstall
build:
	cargo build --release --locked
check:
	cargo fmt --all --check
	cargo test --locked
	cargo clippy --all-targets --locked -- -D warnings
install:
	install -Dm755 target/release/mf-service $(DESTDIR)$(PREFIX)/bin/mf-service
	install -Dm644 packaging/mf-service.service $(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE
uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/mf-service
	rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/mf-service.service
	rm -f $(DESTDIR)$(PREFIX)/share/licenses/mf-service/LICENSE
