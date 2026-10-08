# Builds go outside ~/Documents: iCloud would otherwise sync thousands of build files.
export CARGO_TARGET_DIR := $(HOME)/Library/Caches/sift/target
BIN := $(HOME)/.local/bin

.PHONY: build test lint install app icon

build:
	cargo build --release

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

# Puts `sift` in ~/.local/bin. Then `sift install` starts it at login.
install:
	cargo install --path . --root $(HOME)/.local --force
	@echo "Installed $(BIN)/sift"

# Sign with my Apple Development certificate when there is one, so macOS treats
# each rebuild as the same app and keeps its Downloads/Desktop permission.
SIGN := $(or $(shell security find-identity -v -p codesigning | awk '/Apple Development/ {print $$2; exit}'),-)
BUNDLE := $(CARGO_TARGET_DIR)/release/bundle/macos/sift.app

# Builds the app and puts it in ~/Applications.
app:
	cd app/src-tauri && cargo tauri build --bundles app
	codesign --force --deep --sign $(SIGN) $(BUNDLE)
	-pkill -x sift-app
	rm -rf $(HOME)/Applications/sift.app
	ditto $(BUNDLE) $(HOME)/Applications/sift.app
	@echo "Installed ~/Applications/sift.app"

# Redraws the icon, then has Tauri make every size from it.
icon:
	swift app/icon/make-icon.swift app/icon/icon.png
	cd app/src-tauri && cargo tauri icon ../icon/icon.png
	cd app/src-tauri/icons && rm -rf android ios Square*Logo.png StoreLogo.png icon.ico 64x64.png
	cp app/src-tauri/icons/128x128@2x.png app/ui/icon.png
