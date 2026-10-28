# A thin wrapper: every target is one `cargo` command, which also works
# directly where `make` is not installed (Windows).
CARGO ?= cargo

.PHONY: run publish-check hooks duo

run:
	$(CARGO) run -p launcher

publish-check:
	$(CARGO) xtask publish-check

hooks:
	$(CARGO) xtask hooks

# A host and a player as two processes over 127.0.0.1 (see xtask/src/duo.rs).
duo:
	$(CARGO) xtask duo
