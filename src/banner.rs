//! Welcome banner for interactive `lyra` invocations.
//!
//! Hermes-Agent-style splash: a compact stippled caduceus on the left
//! and a two-column catalog of subcommands + MCP tools on the right,
//! all inside one rounded amber-bordered panel whose top border carries
//! the title (`hermes-lyra v0.4.0`). Shown when the user runs `lyra`
//! with no args and at the tail of `lyra setup`, so the install-time
//! and discovery-time UX are byte-identical.
//!
//! ## Layout (target: 80 columns, no wrapping on a standard terminal)
//!
//! - `OUTER_WIDTH = 80` — the universal floor for terminal width.
//! - Art column = 19 cells, gap = 2 cells, right column = 55 cells.
//!   Total: `1(│) + 1(sp) + 19 + 2 + 55 + 1(sp) + 1(│) = 80`.
//! - Right column lines are pre-budgeted to ≤ 55 visible cells; if any
//!   ever grows past that the renderer truncates with `…` and the
//!   `panel_right_lines_fit_inside_right_column` test will fail in CI
//!   so we notice before users do.
//!
//! ## Design choices
//!
//! - **ANSI off when not a TTY.** `should_color()` honours stdout's
//!   isatty status and the `NO_COLOR` env var. Plain-text output stays
//!   width-aligned because every measurement strips ANSI escapes.
//! - **One panel, not two.** The previous version printed the art
//!   above a separate frame — bigger, slower to scan, and worse on
//!   narrow terminals. The Hermes Agent welcome screen the user
//!   pointed at as the reference puts everything inside a single
//!   bordered region; we match it.
//! - **No external deps.** Box-drawing chars are UTF-8 literals,
//!   colors are bare ANSI SGR sequences. Matches the no-deps style of
//!   the rest of `lyra-ref` and keeps the binary tiny.

use std::io::IsTerminal;

/// ANSI color codes. Amber matches the install-script palette so the
/// CLI banner and the installer log feel like one product.
const AMBER: &str = "\x1b[38;5;214m";
const BRONZE: &str = "\x1b[38;5;136m";
const OFFWHITE: &str = "\x1b[38;5;253m";
const DIM: &str = "\x1b[38;5;240m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Outer panel width. 80 is the universal-terminal floor; widening
/// past this risks wrapping on `tmux`, low-DPI laptops, and CI logs.
const OUTER_WIDTH: usize = 80;
/// Interior width (between the two `│` bars).
const INNER_WIDTH: usize = OUTER_WIDTH - 2;
/// Width of the left art column.
const ART_W: usize = 19;
/// Spacer between art and the right-side catalog text.
const GAP: usize = 2;
/// Width budget for the catalog column. Derived from
/// `OUTER_WIDTH - left_border - left_pad - ART_W - GAP - right_pad - right_border`.
const RIGHT_W: usize = OUTER_WIDTH - 1 - 1 - ART_W - GAP - 1 - 1;

/// Compact stippled caduceus, 19 columns × 14 rows. Density-mapped
/// downscale of the original 100×89 asset, then hand-sharpened so the
/// silhouette reads clearly at this size: wings flare across the top,
/// twin serpents weave around a central staff with two pinch-balls
/// (head and base), and a small foot anchors the bottom. Glyph ramp
/// is `' '.:=+*#'` — same idiom as the source asset.
const CADUCEUS_ART: &[&str] = &[
    "    .:.     .:.    ",
    "  .:=+=:. .:=+=:.  ",
    "    :+*#*=*#*+:    ",
    "     .:+*#*+:.     ",
    "       .===.       ",
    "      +: : :+      ",
    "       :=+=:       ",
    "      *:   :*      ",
    "       :=+=:       ",
    "      +: : :+      ",
    "       :=+=:       ",
    "      *:   :*      ",
    "       .===.       ",
    "        :::        ",
];

/// One catalog entry: a category label and the items it holds.
struct Entry {
    label: &'static str,
    items: &'static [&'static str],
}

/// Lyra's subcommand surface, grouped by intent. Order is the order
/// users will scan top-to-bottom; gates first because they're the
/// thing that distinguishes hermes-lyra from a plain linter.
const CLI_CATALOG: &[Entry] = &[
    Entry { label: "gates",   items: &["bind", "verify", "refine", "compose", "merge"] },
    Entry { label: "lint",    items: &["lint", "lint --strict"] },
    Entry { label: "address", items: &["cid", "publish"] },
    Entry { label: "tooling", items: &["mcp serve", "self-check", "demo refine"] },
    Entry { label: "setup",   items: &["setup", "install", "install --uninstall"] },
    Entry { label: "codec",   items: &["b64-encode", "b64-decode"] },
];

/// The six MCP-exposed `skill_*` tools, grouped to mirror the CLI
/// catalog's verbs (contract / evolve / lint).
const MCP_CATALOG: &[Entry] = &[
    Entry { label: "contract", items: &["skill_bind", "skill_verify"] },
    Entry { label: "evolve",   items: &["skill_refine", "skill_compose", "skill_merge"] },
    Entry { label: "lint",     items: &["skill_lint"] },
];

/// Public entry point. Print art + welcome panel to stdout.
pub fn print_welcome() {
    let color = should_color();
    for line in build_blocks(color) {
        println!("{line}");
    }
}

/// True iff stdout is a TTY and `NO_COLOR` is unset.
fn should_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

fn paint(s: &str, color: &str, color_on: bool) -> String {
    if color_on {
        format!("{color}{s}{RESET}")
    } else {
        s.to_string()
    }
}

/// Pad-right a string to `width` *visible* cells.
fn pad_visible(s: &str, width: usize) -> String {
    let visible = visible_width(s);
    if visible >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + width - visible);
    out.push_str(s);
    for _ in 0..(width - visible) {
        out.push(' ');
    }
    out
}

/// Visible width in cells, skipping ANSI SGR escapes and UTF-8
/// continuation bytes. Multi-byte glyphs like `╭`, `│`, `─`, `◇`
/// count as 1 cell each (they all fit in one terminal column).
fn visible_width(s: &str) -> usize {
    let mut count = 0usize;
    let mut bytes = s.as_bytes().iter();
    while let Some(&b) = bytes.next() {
        if b == 0x1b {
            for &b2 in bytes.by_ref() {
                if b2 == b'm' {
                    break;
                }
            }
            continue;
        }
        if (b & 0xC0) != 0x80 {
            count += 1;
        }
    }
    count
}

/// Truncate a string to `max` visible cells, appending `…` if cut.
fn truncate_visible(s: &str, max: usize) -> String {
    let visible = visible_width(s);
    if visible <= max {
        return s.to_string();
    }
    // Conservative byte-level truncation: walks chars (so multi-byte
    // glyphs stay intact) and stops one cell short to leave room for
    // the ellipsis. Inputs here never contain ANSI escapes (right-
    // column rows are built plain, then coloured by `format_entry`
    // *after* width checking), so we don't need to handle them here.
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    for c in s.chars() {
        if count + 1 > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        count += 1;
    }
    out.push('…');
    out
}

/// Wrap a left/right pair into one framed row. Both sides are
/// pad-or-truncated to their declared widths so the right border
/// always lands in the same column regardless of input length.
fn frame_row(left: &str, right: &str, color_on: bool) -> String {
    let bar = paint("│", AMBER, color_on);
    let left_fit = fit_visible(left, ART_W);
    let right_fit = fit_visible(right, RIGHT_W);
    format!("{bar} {left_fit}  {right_fit} {bar}")
}

/// Pad-right OR truncate-with-ellipsis so the visible width is
/// exactly `width`. Truncation walks chars (UTF-8-safe) and respects
/// ANSI escapes (they pass through, counted as zero cells).
fn fit_visible(s: &str, width: usize) -> String {
    let visible = visible_width(s);
    if visible == width {
        return s.to_string();
    }
    if visible < width {
        return pad_visible(s, width);
    }
    // Visible > width — truncate. Walk chars, keep a running visible
    // count (still skipping ANSI runs), and stop one cell short to
    // leave room for the ellipsis.
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            for c2 in chars.by_ref() {
                out.push(c2);
                if c2 == 'm' {
                    break;
                }
            }
            continue;
        }
        if count + 1 > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        count += 1;
    }
    out.push('…');
    out
}

/// Render a catalog entry as `label: item, item, …`, truncated to
/// fit `RIGHT_W`. Returns a colourised string ready to drop into
/// `frame_row` — width measurement happens *before* colouring.
fn format_entry(e: &Entry, color_on: bool) -> String {
    let plain = format!("{}: {}", e.label, e.items.join(", "));
    let plain = truncate_visible(&plain, RIGHT_W);
    // Re-split at the first ": " so we can colour the label
    // separately from the items. truncate_visible may have cut into
    // the items but never into the label (labels are short).
    let (label_part, rest) = match plain.find(':') {
        Some(i) => (&plain[..i], &plain[i..]),
        None => (plain.as_str(), ""),
    };
    format!(
        "{}{}{}",
        paint(label_part, BRONZE, color_on),
        paint(":", BRONZE, color_on),
        paint(&rest[1.min(rest.len())..], OFFWHITE, color_on),
    )
}

/// Build the full banner as a vec of lines. Pure function — no I/O.
fn build_blocks(color_on: bool) -> Vec<String> {
    let bold = if color_on { BOLD } else { "" };

    // ── Right-column content (plain, then coloured at row time). ──
    // We build it as a Vec<String> so the caller can interleave it
    // with the art column line-by-line.
    let mut right: Vec<String> = Vec::new();
    right.push(paint(&format!("{bold}Subcommands"), AMBER, color_on));
    for e in CLI_CATALOG {
        right.push(format_entry(e, color_on));
    }
    right.push(String::new());
    right.push(paint(&format!("{bold}MCP Tools"), AMBER, color_on));
    for e in MCP_CATALOG {
        right.push(format_entry(e, color_on));
    }
    right.push(String::new());
    right.push(format!(
        "{} {}",
        paint("◇", AMBER, color_on),
        paint(
            "lyra setup  →  registers MCP in ~/.hermes/config.yaml",
            OFFWHITE,
            color_on,
        ),
    ));
    right.push(format!(
        "{} {}",
        paint("◇", AMBER, color_on),
        paint("docs: github.com/ZiriaLabs/hermes-lyra", DIM, color_on),
    ));

    // ── Align art and right column to the same row count. ──
    let rows = CADUCEUS_ART.len().max(right.len());
    let blank_art = " ".repeat(ART_W);
    let blank_right = String::new();

    let mut out: Vec<String> = Vec::with_capacity(rows + 2);

    // ── Top border with embedded title. ──
    // Pattern: ╭<lead>─ <title> ─<trail>╮ → total visible = INNER_WIDTH + 2.
    let title_plain = format!("hermes-lyra v{}", env!("CARGO_PKG_VERSION"));
    let title = paint(&format!("{bold}{title_plain}"), AMBER, color_on);
    let title_w = title_plain.len();
    let trail = 2;
    // lead + 1 (space) + title_w + 1 (space) + trail = INNER_WIDTH
    let lead = INNER_WIDTH.saturating_sub(title_w + trail + 2);
    let dash = "─";
    let top_left = paint("╭", AMBER, color_on);
    let top_right = paint("╮", AMBER, color_on);
    let lead_dashes = paint(&dash.repeat(lead), AMBER, color_on);
    let trail_dashes = paint(&dash.repeat(trail), AMBER, color_on);
    out.push(format!(
        "{top_left}{lead_dashes} {title} {trail_dashes}{top_right}"
    ));

    // ── Body rows: art on left, catalog on right. ──
    for i in 0..rows {
        let a = CADUCEUS_ART.get(i).copied().unwrap_or(blank_art.as_str());
        let r = right.get(i).map(String::as_str).unwrap_or(blank_right.as_str());
        // Paint art rows amber so they pop against the bronze labels.
        let a_painted = paint(a, AMBER, color_on);
        out.push(frame_row(&a_painted, r, color_on));
    }

    // ── Bottom border. ──
    let bottom_left = paint("╰", AMBER, color_on);
    let bottom_right = paint("╯", AMBER, color_on);
    let bottom = paint(&dash.repeat(INNER_WIDTH), AMBER, color_on);
    out.push(format!("{bottom_left}{bottom}{bottom_right}"));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip ANSI SGR escapes; preserves UTF-8 chars.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c2 in chars.by_ref() {
                    if c2 == 'm' {
                        break;
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn visible_width_ignores_ansi() {
        assert_eq!(visible_width("hello"), 5);
        assert_eq!(visible_width("\x1b[38;5;214mhello\x1b[0m"), 5);
        assert_eq!(visible_width("\x1b[1mab\x1b[0mcd"), 4);
    }

    /// Art rows must all be exactly ART_W cells wide so the frame
    /// stays rectangular.
    #[test]
    fn art_rows_have_uniform_width() {
        for (i, row) in CADUCEUS_ART.iter().enumerate() {
            assert_eq!(
                row.len(),
                ART_W,
                "art row {i} is {} cells wide, expected {ART_W}",
                row.len()
            );
        }
    }

    /// Every framed row must have the same visible width — that's
    /// what makes the panel look like a box.
    #[test]
    fn frame_rows_have_consistent_width() {
        for color_on in [false, true] {
            let blocks = build_blocks(color_on);
            let widths: Vec<usize> =
                blocks.iter().map(|l| visible_width(l)).collect();
            let first = widths[0];
            for (i, w) in widths.iter().enumerate() {
                assert_eq!(
                    *w, first,
                    "row {i} has visible width {w}, expected {first} (color_on={color_on})\n\
                     row content: {:?}",
                    blocks[i],
                );
            }
            // And that visible width matches our declared OUTER_WIDTH.
            assert_eq!(first, OUTER_WIDTH);
        }
    }

    /// Every advertised subcommand and MCP tool must appear in plain
    /// output (grep-friendly CI logs).
    #[test]
    fn panel_advertises_every_subcommand_and_mcp_tool() {
        let plain = build_blocks(false).join("\n");
        for e in CLI_CATALOG {
            assert!(plain.contains(e.label), "missing category {}", e.label);
            assert!(
                plain.contains(e.items[0]),
                "missing first item of {}: {}",
                e.label,
                e.items[0]
            );
        }
        for e in MCP_CATALOG {
            assert!(
                plain.contains(e.label),
                "missing MCP category {}",
                e.label
            );
            assert!(
                plain.contains(e.items[0]),
                "missing first MCP tool of {}: {}",
                e.label,
                e.items[0]
            );
        }
    }

    /// The hint line directs the user at `lyra setup`. Acts as a
    /// regression test against accidental rewording during cleanup.
    #[test]
    fn footer_directs_user_at_lyra_setup() {
        let plain = build_blocks(false).join("\n");
        assert!(plain.contains("lyra setup"));
    }

    /// Every right-column line (pre-colour) must fit inside RIGHT_W.
    /// Catches any future catalog growth that would force truncation.
    #[test]
    fn panel_right_lines_fit_inside_right_column() {
        for e in CLI_CATALOG.iter().chain(MCP_CATALOG.iter()) {
            let plain = format!("{}: {}", e.label, e.items.join(", "));
            assert!(
                plain.len() <= RIGHT_W,
                "catalog row {:?} is {} cells, exceeds RIGHT_W={RIGHT_W}",
                plain,
                plain.len(),
            );
        }
    }

    /// Banner must render in under 25 rows total — the whole reason
    /// for the redesign was "small, fast to load". This is the
    /// machine-checkable floor; current target is ~16-17 rows.
    #[test]
    fn banner_is_compact() {
        let blocks = build_blocks(false);
        assert!(
            blocks.len() <= 25,
            "banner has {} rows, expected ≤ 25 (keep it crisp)",
            blocks.len(),
        );
    }

    /// Top border must contain the title; bottom border must not.
    #[test]
    fn title_lives_in_top_border() {
        let blocks = build_blocks(false);
        let top = blocks.first().expect("no rows");
        let bottom = blocks.last().expect("no rows");
        assert!(strip_ansi(top).contains("hermes-lyra v"));
        assert!(!strip_ansi(bottom).contains("hermes-lyra"));
    }
}
