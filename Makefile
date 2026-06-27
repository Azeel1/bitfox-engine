ENGINE := engine
BIN := $(ENGINE)/target/release/bitfox
FEATURES :=
FEATFLAG := $(if $(FEATURES),--features $(FEATURES))
WIN_TARGET := x86_64-pc-windows-gnu

.PHONY: release build test perft divide run windows version fmt clippy clean help

release:
	cd $(ENGINE) && cargo build --release $(FEATFLAG)

windows:
	cd $(ENGINE) && cargo build --release --target $(WIN_TARGET) $(FEATFLAG)
	@echo "built $(ENGINE)/target/$(WIN_TARGET)/release/bitfox.exe"

version:
	@cd $(ENGINE) && cargo pkgid | sed 's/.*[#@]//'

build:
	cd $(ENGINE) && cargo build $(FEATFLAG)

test:
	cd $(ENGINE) && cargo test --release $(FEATFLAG)

perft: release
	$(BIN) perft 6

divide: release
	$(BIN) divide 1

run: release
	$(BIN) uci

fmt:
	cd $(ENGINE) && cargo fmt

clippy:
	cd $(ENGINE) && cargo clippy --release $(FEATFLAG) -- -D warnings

clean:
	cd $(ENGINE) && cargo clean

help:
	@echo "release  build the optimized engine binary (strongest net)"
	@echo "test     run the perft + correctness suite"
	@echo "perft    perft(6) from startpos"
	@echo "run      start the engine in UCI mode"
	@echo "windows  cross-compile the Windows engine binary (bitfox.exe)"
	@echo "version  print the current engine version"
	@echo "fmt      cargo fmt"
	@echo "clippy   cargo clippy (deny warnings)"
	@echo "clean    remove build artifacts"
