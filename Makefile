SHELL := /bin/bash
.SHELLFLAGS := -euo pipefail -c

SERVER_PORT ?= 7878
INSTALL_DIR ?= /usr/local/bin

.PHONY: all stop build install install-agents install-agent prep-agents

all: stop build install

# 0. Helper: creates agents/ dir and a stub to satisfy workspace glob
prep-agents:
	@mkdir -p agents
	@if [ ! "$$(ls -A agents 2>/dev/null)" ]; then \
		mkdir -p agents/stub/src; \
		printf '[package]\nname = "osy-stub"\nversion = "0.1.0"\nedition = "2024"\npublish = false\n\n[dependencies]\nosy-share.workspace = true\n' > agents/stub/Cargo.toml; \
		touch agents/stub/src/lib.rs; \
	fi

# 1. Stopping processes on a port
stop:
	@printf "\n\033[1mStopping running services\033[0m\n"
	@if command -v fuser >/dev/null 2>&1; then \
		if [[ -w "$(INSTALL_DIR)" ]]; then \
			fuser -k -9 "$(SERVER_PORT)/tcp" >/dev/null 2>&1 || true; \
		else \
			sudo fuser -k -9 "$(SERVER_PORT)/tcp" >/dev/null 2>&1 || true; \
		fi; \
	elif command -v lsof >/dev/null 2>&1; then \
		PIDS=$$(lsof -t -i:"$(SERVER_PORT)" 2>/dev/null || true); \
		if [[ -n "$$PIDS" ]]; then \
			if [[ -w "$(INSTALL_DIR)" ]]; then \
				kill -9 $$PIDS >/dev/null 2>&1 || true; \
			else \
				sudo kill -9 $$PIDS >/dev/null 2>&1 || true; \
			fi; \
		fi; \
	fi
	@printf "  \033[32m✓\033[0m Port $(SERVER_PORT) cleared\n"

# 2. Verification of requirements
check: prep-agents
	@printf "\n\033[1mChecking requirements\033[0m\n"
	@command -v cargo >/dev/null 2>&1 || { echo "  ✗ cargo is not installed" >&2; exit 1; }
	@command -v jq >/dev/null 2>&1    || { echo "  ✗ jq is not installed" >&2; exit 1; }
	@[[ -f Cargo.toml ]]              || { echo "  ✗ Cargo.toml not found" >&2; exit 1; }
	@printf "  \033[32m✓\033[0m Environment ready\n"

# 3. Building with Cargo
build: check
	@printf "\n\033[1mBuilding binaries\033[0m\n"
	cargo build --release
	@printf "  \033[32m✓\033[0m Compilation completed\n"

# 4. Installing binaries in the system and starting the server
install: stop build
	@printf "\n\033[1mInstalling binaries\033[0m\n"
	@if [[ -w "$(INSTALL_DIR)" ]]; then \
		INSTALL_CMD="install"; \
	else \
		echo "  ℹ No write access to $(INSTALL_DIR), using sudo"; \
		INSTALL_CMD="sudo install"; \
	fi; \
	METADATA="$$(cargo metadata --format-version=1 --no-deps)"; \
	BINARIES=$$(jq -r '.packages[].targets[] | select(.kind | any(. == "bin")) | .name' <<< "$$METADATA"); \
	for bin in $$BINARIES; do \
		src="target/release/$$bin"; \
		if [[ -f "$$src" ]]; then \
			$$INSTALL_CMD -m 755 "$$src" "$(INSTALL_DIR)/$$bin"; \
			printf "  \033[32m✓\033[0m Installed $$bin -> $(INSTALL_DIR)/$$bin\n"; \
		else \
			echo "  ✗ Binary missing: $$src" >&2; \
			exit 1; \
		fi; \
	done
	@printf "\033[1;32mCompleted\033[0m Installation finished\n"
	@printf "\n───────────────────────────────────────────────────────\n"
	@osy server start

# 5. Hot-updating a SINGLE agent by name
install-agent: prep-agents
	@if [[ -z "$(name)" ]]; then \
		echo "  ✗ Error: Agent name is required. Use: make install-agent name=<agent_name>" >&2; \
		exit 1; \
	fi; \
	if [[ ! -d "agents/$(name)" ]]; then \
		echo "  ✗ Agent directory agents/$(name) not found" >&2; \
		exit 1; \
	fi; \
	if [[ -w "$(INSTALL_DIR)" ]]; then \
		INSTALL_CMD="install"; \
	else \
		INSTALL_CMD="sudo install"; \
	fi; \
	printf "\n\033[1mBuilding agent: $(name)\033[0m\n"; \
	cargo build --release --manifest-path agents/$(name)/Cargo.toml; \
	METADATA="$$(cargo metadata --format-version=1 --no-deps --manifest-path agents/$(name)/Cargo.toml)"; \
	PACKAGE_NAME=$$(jq -r '.workspace_members[]' <<< "$$METADATA" | while read -r id; do \
		pkg_dir=$$(jq -r --arg id "$$id" '.packages[] | select(.id == $$id) | .manifest_path' <<< "$$METADATA" | xargs dirname); \
		if [[ "$$pkg_dir" == *"agents/$(name)" ]]; then \
			jq -r --arg id "$$id" '.packages[] | select(.id == $$id) | .name' <<< "$$METADATA"; \
			break; \
		fi; \
	done); \
	if [[ -z "$$PACKAGE_NAME" ]]; then \
		PACKAGE_NAME="$(name)"; \
	fi; \
	BINARIES=$$(jq -r --arg pkg "$$PACKAGE_NAME" '.packages[] | select(.name == $$pkg) | .targets[] | select(.kind | any(. == "bin")) | .name' <<< "$$METADATA"); \
	for bin in $$BINARIES; do \
		src="target/release/$$bin"; \
		if [[ -f "$$src" ]]; then \
			$$INSTALL_CMD -m 755 "$$src" "$(INSTALL_DIR)/$$bin"; \
			printf "  \033[32m✓\033[0m Hot-updated binary $$bin -> $(INSTALL_DIR)/$$bin\n"; \
		else \
			echo "  ✗ Binary missing: $$src" >&2; \
			exit 1; \
		fi; \
	done; \
	printf "\033[32m✓\033[0m Agent $(name) updated successfully\n"

# 6. Hot-updating ALL agents and refreshing the system
install-agents: prep-agents
	@agents_list=(); \
	for d in agents/*/; do \
		[[ -d "$$d" ]] || continue; \
		aname=$$(basename "$$d"); \
		[[ "$$aname" == "stub" ]] && continue; \
		agents_list+=("$$aname"); \
	done; \
	if [[ $${#agents_list[@]} -eq 0 ]]; then \
		echo "  ℹ No real agents found in agents/ (stub ignored)" >&2; \
		exit 0; \
	fi; \
	for ag in "$${agents_list[@]}"; do \
		$(MAKE) install-agent name=$$ag || exit 1; \
	done
	@printf "\n───────────────────────────────────────────────────────\n"
	@osy refresh
