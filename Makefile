# reel: pega un enlace, elige un formato, y la cola hace el resto.
#
# make            lo mismo que `make help`
# make setup      instala lo que hace falta y deja el repo listo
# make run        corre la app
#
# Las recetas de release estan al final y necesitan `gh`.

SHELL      := /bin/bash
.SHELLFLAGS:= -eu -o pipefail -c
.DEFAULT_GOAL := help

BIN        := reel
VERSION    := $(shell grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGET_DIR := target
RELEASE_BIN:= $(TARGET_DIR)/release/$(BIN)
DEBUG_BIN  := $(TARGET_DIR)/debug/$(BIN)
PREFIX     ?= $(HOME)/.local
SLUG       := reel

# Lo que la app necesita en el sistema, no en cargo.
RUNTIME_PKGS := yt-dlp ffmpeg wl-clipboard
# Lo que eframe necesita para compilar en Wayland y X11.
BUILD_PKGS   := rustup pkgconf libxkbcommon wayland libx11 libxcursor libxrandr libxi mesa

# Colores solo si la salida es una terminal.
ifneq (,$(findstring xterm,$(TERM)))
  C_OK   := \033[32m
  C_WARN := \033[33m
  C_DIM  := \033[2m
  C_OFF  := \033[0m
else
  C_OK   :=
  C_WARN :=
  C_DIM  :=
  C_OFF  :=
endif

define say
	@printf '$(C_OK)==>$(C_OFF) %s\n' $(1)
endef

define warn
	@printf '$(C_WARN)==>$(C_OFF) %s\n' $(1)
endef

## ---------------------------------------------------------------- ayuda ----

.PHONY: help
help: ## muestra esta ayuda
	@printf '\n  $(C_OK)reel$(C_OFF) $(C_DIM)v$(VERSION)$(C_OFF)\n\n'
	@grep -hE '^[a-zA-Z0-9_.-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'
	@printf '\n'

## -------------------------------------------------------------- arranque ----

.PHONY: setup
setup: deps toolchain fetch ## deja el repo listo para `make run`
	$(call say,"listo. ahora: make run")

.PHONY: deps
deps: ## instala yt-dlp, ffmpeg y las librerias que pide eframe (Arch)
ifeq ($(shell command -v pacman 2>/dev/null),)
	$(call warn,"esto no es Arch: instala a mano $(RUNTIME_PKGS)")
else
	sudo pacman -S --needed --noconfirm $(RUNTIME_PKGS) $(BUILD_PKGS)
endif

.PHONY: toolchain
toolchain: ## instala el toolchain de rust-toolchain.toml
ifeq ($(shell command -v rustup 2>/dev/null),)
	$(call warn,"no hay rustup: https://rustup.rs")
	@exit 1
else
	rustup show active-toolchain || rustup toolchain install stable
	rustup component add rustfmt clippy
endif

.PHONY: fetch
fetch: ## baja las dependencias, incluidas las crates de fastframe
	cargo fetch --locked || cargo fetch

.PHONY: doctor
doctor: ## dice que falta en el sistema para que reel funcione
	@printf '\n'
	@for tool in cargo yt-dlp ffmpeg wl-paste; do \
		if command -v $$tool >/dev/null 2>&1; then \
			printf '  $(C_OK)ok$(C_OFF)    %-12s %s\n' "$$tool" "$$($$tool --version 2>&1 | head -1)"; \
		else \
			printf '  $(C_WARN)falta$(C_OFF) %-12s\n' "$$tool"; \
		fi; \
	done
	@if [ -d "$$HOME/.config/omarchy" ] && [ -d "$$HOME/.local/state/omarchy/current" ]; then \
		printf '  $(C_OK)ok$(C_OFF)    %-12s el tema se va a seguir solo\n' "omarchy"; \
	else \
		printf '  $(C_DIM)--$(C_OFF)    %-12s sin omarchy: se usan las paletas propias\n' "omarchy"; \
	fi
	@printf '\n'

## ------------------------------------------------------------ desarrollo ----

.PHONY: run
run: ## corre la app en debug
	cargo run

.PHONY: dev
dev: ## recompila y relanza al guardar (necesita cargo-watch)
ifeq ($(shell command -v cargo-watch 2>/dev/null),)
	$(call warn,"instala cargo-watch: cargo install cargo-watch")
	@exit 1
else
	cargo watch -x run
endif

.PHONY: build
build: ## compila en debug
	cargo build

.PHONY: release
release: ## compila optimizado
	cargo build --release
	$(call say,"$(RELEASE_BIN)")

.PHONY: check
check: ## compila sin generar binario, rapido
	cargo check --all-targets

.PHONY: test
test: ## corre las pruebas
	cargo test --features selfcheck

.PHONY: selfcheck
selfcheck: ## pruebas de la cola con un yt-dlp falso: de a uno, tope, cancelacion
	cargo test --features selfcheck --test cola -- --test-threads=1

.PHONY: selfcheck-net
selfcheck-net: ## lo mismo contra el yt-dlp de verdad (necesita internet)
	cargo build --features selfcheck
	./$(DEBUG_BIN) --download-selfcheck descarga-real
	./$(DEBUG_BIN) --download-selfcheck volver-a-bajar

.PHONY: fmt
fmt: ## formatea
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## revisa el formato sin tocar nada; falla si hay algo pendiente
	cargo fmt --all --check

.PHONY: lint
lint: ## clippy con los warnings como errores
	cargo clippy --all-targets --features selfcheck -- -D warnings

.PHONY: verify
verify: fmt-check lint test ## lo que tiene que pasar antes de un commit
	$(call say,"todo limpio")

.PHONY: site-serve
site-serve: ## sirve el sitio de documentacion en local (site/)
	cd site && bundle config set --local path vendor/bundle
	cd site && bundle check >/dev/null || bundle install
	cd site && bundle exec jekyll serve --livereload

.PHONY: clean
clean: ## borra target/
	cargo clean

## ----------------------------------------------------------------- temas ----

.PHONY: theme-install
theme-install: ## instala la plantilla de Omarchy sin pisar la tuya
	@mkdir -p $$HOME/.config/omarchy/themed
	@if [ -e "$$HOME/.config/omarchy/themed/$(SLUG).json.tpl" ]; then \
		printf '  $(C_DIM)ya existe, no la toco$(C_OFF)\n'; \
	else \
		cp contrib/omarchy/$(SLUG).json.tpl $$HOME/.config/omarchy/themed/; \
		printf '  $(C_OK)instalada$(C_OFF)\n'; \
	fi

.PHONY: theme-show
theme-show: ## dice que tema de Omarchy esta activo ahora
	@readlink -f $$HOME/.local/state/omarchy/current/theme 2>/dev/null \
		|| printf '  sin omarchy\n'

## --------------------------------------------------------------- instalar ----

.PHONY: install
install: release ## copia el binario a $(PREFIX)/bin y pone el .desktop
	install -Dm755 $(RELEASE_BIN) $(PREFIX)/bin/$(BIN)
	install -Dm644 contrib/$(SLUG).desktop $(PREFIX)/share/applications/$(SLUG).desktop
	$(call say,"$(PREFIX)/bin/$(BIN)")

.PHONY: uninstall
uninstall: ## quita lo que puso install
	rm -f $(PREFIX)/bin/$(BIN) $(PREFIX)/share/applications/$(SLUG).desktop

## --------------------------------------------------------------- release ----

.PHONY: tag
tag: verify ## etiqueta la version de Cargo.toml y la sube
	git tag -a v$(VERSION) -m "reel $(VERSION)"
	git push origin v$(VERSION)
	$(call say,"v$(VERSION)")

.PHONY: publish
publish: release ## crea el release de GitHub con el binario y checksums
	@mkdir -p dist
	cp $(RELEASE_BIN) dist/$(BIN)
	cd dist && tar czf $(BIN)-$(VERSION)-x86_64-linux.tar.gz $(BIN)
	cd dist && sha256sum *.tar.gz > checksums.txt
	gh release create v$(VERSION) dist/*.tar.gz dist/checksums.txt \
		--title "reel $(VERSION)" --generate-notes
