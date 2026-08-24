# Wordbar — build, test and install.
#
# Layout after `make install` (default PREFIX=$(HOME)/.local):
#   $(BINDIR)/wordbar-service            D-Bus activated, never autostarted
#   $(BINDIR)/wordbarctl
#   $(BINDIR)/wordbar-app
#   $(DATADIR)/dbus-1/services/…         activation file
#   $(DATADIR)/applications/…            desktop entry
#   $(EXTENSIONDIR)/wordbar@…            GNOME Shell extension

PREFIX      ?= $(HOME)/.local
BINDIR      ?= $(PREFIX)/bin
DATADIR     ?= $(PREFIX)/share
EXTENSION_UUID = wordbar@kemalbektas.dev
EXTENSIONDIR ?= $(DATADIR)/gnome-shell/extensions/$(EXTENSION_UUID)

CARGO       ?= cargo
CARGO_FLAGS ?= --release
TARGETDIR   ?= target/release

BINARIES = wordbar-service wordbarctl wordbar-app

.PHONY: all build test test-rust test-extension e2e lint install install-binaries \
        install-dbus install-desktop install-extension uninstall clean help

all: build

## build: compile every crate
build:
	$(CARGO) build $(CARGO_FLAGS)

## test: run the Rust unit and integration tests plus the extension helpers
test: test-rust test-extension

test-rust:
	$(CARGO) test --workspace

test-extension:
	./scripts/check-extension.sh
	gjs -m extension/test/terms-test.js

## e2e: prove the backend over a private session bus (Phase 10)
e2e: build
	./scripts/e2e-backend.sh $(TARGETDIR)

## lint: formatting and clippy
lint:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

install: install-binaries install-dbus install-desktop install-extension
	@echo
	@echo "Installed. Next:"
	@echo "  1. Ensure $(BINDIR) is on your PATH."
	@echo "  2. Log out and back in (or restart GNOME Shell on X11)."
	@echo "  3. gnome-extensions enable $(EXTENSION_UUID)"

install-binaries: build
	install -d $(DESTDIR)$(BINDIR)
	for binary in $(BINARIES); do \
		install -m 0755 $(TARGETDIR)/$$binary $(DESTDIR)$(BINDIR)/$$binary; \
	done

# The activation file is what makes the service start on demand. There is
# deliberately no systemd unit and no autostart entry.
install-dbus:
	install -d $(DESTDIR)$(DATADIR)/dbus-1/services
	sed 's|@BINDIR@|$(BINDIR)|g' packaging/dbus/org.wordbar.Wordbar.service.in \
		> $(DESTDIR)$(DATADIR)/dbus-1/services/org.wordbar.Wordbar.service

install-desktop:
	install -d $(DESTDIR)$(DATADIR)/applications
	install -m 0644 packaging/org.wordbar.Wordbar.desktop \
		$(DESTDIR)$(DATADIR)/applications/org.wordbar.Wordbar.desktop

install-extension:
	install -d $(DESTDIR)$(EXTENSIONDIR)/schemas
	install -m 0644 extension/*.js extension/metadata.json extension/stylesheet.css \
		$(DESTDIR)$(EXTENSIONDIR)/
	install -m 0644 extension/schemas/*.gschema.xml $(DESTDIR)$(EXTENSIONDIR)/schemas/
	glib-compile-schemas $(DESTDIR)$(EXTENSIONDIR)/schemas

uninstall:
	for binary in $(BINARIES); do rm -f $(DESTDIR)$(BINDIR)/$$binary; done
	rm -f $(DESTDIR)$(DATADIR)/dbus-1/services/org.wordbar.Wordbar.service
	rm -f $(DESTDIR)$(DATADIR)/applications/org.wordbar.Wordbar.desktop
	rm -rf $(DESTDIR)$(EXTENSIONDIR)
	@echo "Your vocabulary in ~/.local/share/wordbar was left untouched."

clean:
	$(CARGO) clean

help:
	@grep -E '^## ' Makefile | sed 's/## /  /'
