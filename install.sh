#!/usr/bin/env bash
# hermes-lyra one-line installer
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ZiriaLabs/hermes-lyra/main/install.sh | bash
#
# What it does:
#   1. Verifies prerequisites (rustc 1.83+, cargo, git).
#   2. Clones github.com/ZiriaLabs/hermes-lyra (or pulls if already cloned).
#   3. Builds the `lyra` binary in release mode.
#   4. Copies it into $HOME/.local/bin (and warns if that's not on PATH).
#   5. Runs `lyra setup` to register the MCP server in ~/.hermes/config.yaml.
#
# Design choices:
#   * No sudo. We never touch system directories. The binary goes to
#     $HOME/.local/bin (XDG-standard for per-user binaries).
#   * Idempotent. Re-running this script updates an existing checkout
#     and re-runs setup, which is itself idempotent.
#   * Honest about what it can't do. Pre-built binaries aren't hosted
#     yet, so we build from source. If rustc/cargo isn't present we
#     point at https://rustup.rs/ instead of trying to install Rust
#     ourselves — that's a separate trust decision the user should make
#     explicitly.
#   * Strict mode. `set -euo pipefail` so any failed step aborts loudly
#     instead of silently leaving the user with a broken install.

set -euo pipefail

# ── Pretty output helpers. Amber/bronze palette matches the banner. ──
# Only emit color if stdout is a TTY.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    AMBER=$'\033[38;5;214m'
    BRONZE=$'\033[38;5;136m'
    GREEN=$'\033[38;5;035m'
    RED=$'\033[38;5;160m'
    DIM=$'\033[38;5;240m'
    RESET=$'\033[0m'
else
    AMBER=""; BRONZE=""; GREEN=""; RED=""; DIM=""; RESET=""
fi

step() { echo "${AMBER}◇${RESET} $1"; }
info() { echo "  ${DIM}$1${RESET}"; }
ok()   { echo "  ${GREEN}✓${RESET} $1"; }
warn() { echo "  ${BRONZE}!${RESET} $1"; }
die()  { echo "${RED}✗${RESET} $1" >&2; exit 1; }

REPO_URL="${LYRA_REPO_URL:-https://github.com/ZiriaLabs/hermes-lyra.git}"
INSTALL_DIR="${LYRA_INSTALL_DIR:-$HOME/.local/share/hermes-lyra}"
BIN_DIR="${LYRA_BIN_DIR:-$HOME/.local/bin}"
BRANCH="${LYRA_BRANCH:-main}"

echo
echo "${AMBER}  hermes-lyra installer${RESET}"
echo "${DIM}  ─────────────────────${RESET}"
echo

# ── Step 1: prereq check. Fail-fast with actionable hints. ──
step "Checking prerequisites"
for cmd in git cargo rustc; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        case "$cmd" in
            cargo|rustc)
                die "$cmd not found. Install Rust 1.83+ from https://rustup.rs/ and re-run."
                ;;
            git)
                die "$cmd not found. Install git from your package manager and re-run."
                ;;
        esac
    fi
done
RUSTC_VERSION=$(rustc --version | awk '{print $2}')
# Crude version compare: rust 1.83+ required. Doesn't handle nightly
# qualifiers but those satisfy semver-ge against 1.83 anyway.
if ! printf '1.83.0\n%s\n' "$RUSTC_VERSION" | sort -V -C 2>/dev/null; then
    warn "rustc $RUSTC_VERSION may be too old (need 1.83+). Continuing — build will fail loudly if so."
fi
ok "rustc $RUSTC_VERSION, cargo, git present"

# ── Step 2: source. Clone fresh or fast-forward existing checkout. ──
step "Fetching source from $REPO_URL"
mkdir -p "$(dirname "$INSTALL_DIR")"
if [ -d "$INSTALL_DIR/.git" ]; then
    info "Existing checkout at $INSTALL_DIR — updating"
    git -C "$INSTALL_DIR" fetch --quiet origin
    git -C "$INSTALL_DIR" checkout --quiet "$BRANCH"
    git -C "$INSTALL_DIR" pull --quiet --ff-only origin "$BRANCH"
else
    info "Cloning into $INSTALL_DIR"
    git clone --quiet --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
fi
ok "source ready at $INSTALL_DIR"

# ── Step 3: build. Release mode for reasonable startup cost. ──
step "Building lyra (release)"
( cd "$INSTALL_DIR" && cargo build --release --bin lyra --quiet )
SRC_BIN="$INSTALL_DIR/target/release/lyra"
[ -x "$SRC_BIN" ] || die "build succeeded but $SRC_BIN is missing — open an issue"
ok "built $SRC_BIN"

# ── Step 4: install into PATH. No sudo, always user-scope. ──
step "Installing into $BIN_DIR"
mkdir -p "$BIN_DIR"
cp "$SRC_BIN" "$BIN_DIR/lyra"
chmod +x "$BIN_DIR/lyra"
ok "installed $BIN_DIR/lyra"

# Warn if BIN_DIR isn't on PATH — without this hint, the next step
# would fail with `lyra: command not found` and the user would have
# no way to recover.
case ":$PATH:" in
    *":$BIN_DIR:"*) : ;;
    *)
        warn "$BIN_DIR is NOT on your PATH"
        case "${SHELL##*/}" in
            zsh)  RC="$HOME/.zshrc" ;;
            bash) RC="$HOME/.bashrc" ;;
            fish) RC="$HOME/.config/fish/config.fish" ;;
            *)    RC="your shell config file" ;;
        esac
        info "Add this to $RC:"
        info "  export PATH=\"$BIN_DIR:\$PATH\""
        info "Then open a new shell, or run: export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac

# ── Step 5: register MCP server + install /lyra slash command. ──
# `lyra setup` runs three idempotent operations in one shot:
#   1. Splice mcp_servers.lyra into ~/.hermes/config.yaml so the
#      five skill_* MCP tools become available to every Hermes
#      session (CLI, TUI, gateway).
#   2. Drop ~/.hermes/skills/lyra/SKILL.md so /lyra appears as a
#      native Hermes slash command in the CLI/TUI tab-complete.
#   3. Print the welcome panel.
# All three steps are byte-comparison idempotent — re-running this
# installer is a safe no-op past the first run.
step "Registering MCP server + /lyra slash command (lyra setup)"
"$BIN_DIR/lyra" setup

echo
echo "${GREEN}✓${RESET} Done."
echo "  ${DIM}Type${RESET} ${AMBER}lyra${RESET} ${DIM}any time to see the panel above.${RESET}"
echo "  ${DIM}In any Hermes session (CLI/TUI), type${RESET} ${AMBER}/lyra${RESET} ${DIM}for the agent menu.${RESET}"
echo
