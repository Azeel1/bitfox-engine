PYTHON ?= python3
PROJECT := $(PYTHON) tools/bitfox.py
ENGINE := engine
BIN := $(ENGINE)/target/release/bitfox
FEATURES :=
FEATFLAG := $(if $(FEATURES),--features $(FEATURES))
WIN_TARGET := x86_64-pc-windows-gnu
EXE :=

.PHONY: release pgo build test check tools-test gui perft divide run windows version fmt clippy clean help

release:
	$(PROJECT) build $(FEATFLAG)
	$(if $(EXE),cp $(BIN) $(EXE))

pgo:
	tools/pgo-build.sh "$(CPU)" $(ENGINE)/target/bitfox-pgo
	$(if $(EXE),cp $(ENGINE)/target/bitfox-pgo $(EXE))

windows:
	$(PROJECT) build --target $(WIN_TARGET) $(FEATFLAG)
	@echo "built $(ENGINE)/target/$(WIN_TARGET)/release/bitfox.exe"

version:
	@$(PROJECT) version

build:
	$(PROJECT) build --profile dev $(FEATFLAG)

test:
	$(PROJECT) test $(FEATFLAG)

check:
	$(PROJECT) check

tools-test:
	$(PYTHON) -B -m unittest discover -s tools -p 'test_*.py'

gui:
	$(PROJECT) gui configure
	$(PROJECT) gui build

perft:
	$(PROJECT) perft 6

divide: release
	$(BIN) divide 1

run: release
	$(BIN) uci

fmt:
	cd $(ENGINE) && cargo fmt

clippy:
	cd $(ENGINE) && cargo clippy --release --locked $(FEATFLAG) -- -D warnings

clean:
	cd $(ENGINE) && cargo clean
	rm -rf gui-qt/build gui-qt/build-arm64 gui-qt/build-strict \
		gui-qt/build-linux gui-qt/build-macos gui-qt/build-windows build

help:
	@echo "release  build the optimized engine binary and cdylib"
	@echo "pgo      profile-guided release build (CPU=x86-64-v3 for AVX2)"
	@echo "build    debug build"
	@echo "test     run the perft + correctness suite"
	@echo "check    verify shared version, network, ABI and training dependency contracts"
	@echo "tools-test  run the Python tool tests"
	@echo "gui      configure and build the Qt application in build/gui"
	@echo "perft    perft(6) from startpos"
	@echo "divide   perft divide at the root"
	@echo "run      start the engine in UCI mode"
	@echo "windows  cross-compile the Windows engine binary (bitfox.exe)"
	@echo "version  print the current engine version"
	@echo "fmt      cargo fmt"
	@echo "clippy   cargo clippy (deny warnings)"
	@echo "clean    remove engine, Qt, and native build artifacts"
